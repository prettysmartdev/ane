use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;

pub(crate) fn encode_lsp_message(body: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = header.into_bytes();
    out.extend_from_slice(body);
    out
}

pub(crate) fn decode_lsp_message<R: BufRead>(reader: &mut R) -> Result<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            bail!("LSP server closed connection");
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length = Some(len_str.parse()?);
        }
    }
    let length = content_length
        .ok_or_else(|| anyhow::anyhow!("missing Content-Length header in LSP message"))?;
    if length > 16 * 1024 * 1024 {
        bail!("LSP message exceeds 16 MiB limit");
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body)?;
    Ok(value)
}

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: i64,
    method: String,
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: String,
    params: Value,
}

type Response = std::result::Result<Value, String>;
type Pending = Arc<Mutex<HashMap<i64, mpsc::SyncSender<Response>>>>;

/// Cloneable request endpoint. Dedicated threads own both pipes, so neither a
/// silent stdout nor a full stdin pipe can trap a caller beyond its deadline.
#[derive(Clone)]
pub struct LspTransport {
    outgoing: mpsc::SyncSender<Value>,
    pending: Pending,
    next_id: Arc<AtomicI64>,
    closed: Arc<AtomicBool>,
    timeout: Duration,
    cancellation: Option<Arc<AtomicBool>>,
}

fn fail_pending(pending: &Pending, error: String) {
    let requests = std::mem::take(&mut *pending.lock().unwrap());
    for (_, sender) in requests {
        let _ = sender.try_send(Err(error.clone()));
    }
}

impl LspTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        let (outgoing, rx) = mpsc::sync_channel::<Value>(128);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let writer_pending = Arc::clone(&pending);
        let writer_closed = Arc::clone(&closed);
        std::thread::spawn(move || {
            let mut writer = BufWriter::new(stdin);
            while let Ok(value) = rx.recv() {
                let result = serde_json::to_vec(&value)
                    .map_err(anyhow::Error::from)
                    .and_then(|body| {
                        writer.write_all(&encode_lsp_message(&body))?;
                        writer.flush()?;
                        Ok(())
                    });
                if let Err(error) = result {
                    writer_closed.store(true, Ordering::Release);
                    fail_pending(&writer_pending, error.to_string());
                    break;
                }
            }
        });
        let reader_pending = Arc::clone(&pending);
        let reader_closed = Arc::clone(&closed);
        let replies = outgoing.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let message = match decode_lsp_message(&mut reader) {
                    Ok(message) => message,
                    Err(error) => {
                        reader_closed.store(true, Ordering::Release);
                        fail_pending(&reader_pending, error.to_string());
                        break;
                    }
                };
                // Server requests have their own ID space. Never mistake one
                // for a response, even if its numeric ID matches ours.
                if let Some(method) = message.get("method").and_then(Value::as_str) {
                    if let Some(id) = message.get("id") {
                        let result = match method {
                            "workspace/configuration" => {
                                let count = message
                                    .pointer("/params/items")
                                    .and_then(Value::as_array)
                                    .map_or(0, Vec::len);
                                serde_json::json!({"jsonrpc":"2.0", "id": id, "result": vec![Value::Null; count]})
                            }
                            "client/registerCapability"
                            | "client/unregisterCapability"
                            | "window/workDoneProgress/create"
                            | "workspace/workspaceFolders" => {
                                serde_json::json!({"jsonrpc":"2.0", "id": id, "result": null})
                            }
                            _ => serde_json::json!({"jsonrpc":"2.0", "id": id,
                                "error": {"code": -32601, "message": "Unsupported client request"}}),
                        };
                        if replies.try_send(result).is_err() {
                            reader_closed.store(true, Ordering::Release);
                            fail_pending(&reader_pending, "LSP writer queue full".into());
                            break;
                        }
                    }
                    continue;
                }
                if let Some(id) = message.get("id").and_then(Value::as_i64)
                    && let Some(sender) = reader_pending.lock().unwrap().remove(&id)
                {
                    let response = if let Some(error) = message.get("error") {
                        Err(format!("LSP error: {error}"))
                    } else {
                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                    };
                    let _ = sender.try_send(response);
                }
            }
        });
        Self {
            outgoing,
            pending,
            next_id: Arc::new(AtomicI64::new(0)),
            closed,
            timeout: Duration::from_secs(5),
            cancellation: None,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn with_cancellation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn cancel_pending(&self) {
        fail_pending(&self.pending, "LSP session closed".into());
    }

    pub(crate) fn start_request(&mut self, method: &str, params: Value) -> Result<PendingRequest> {
        if self.closed.load(Ordering::Acquire) {
            bail!("LSP connection closed");
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(id, tx);
        let request = serde_json::to_value(JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        })?;
        if let Err(error) = self.outgoing.try_send(request) {
            self.pending.lock().unwrap().remove(&id);
            bail!("LSP request queue unavailable: {error}");
        }
        Ok(PendingRequest {
            transport: self.clone(),
            id,
            receiver: rx,
            deadline: Instant::now() + self.timeout,
            completed: false,
            _timing: crate::commands::diagnostics::Timing::new("lsp_request"),
        })
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.start_request(method, params)?.wait()
    }

    pub fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            bail!("LSP connection closed");
        }
        let notification = serde_json::to_value(JsonRpcNotification {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        })?;
        self.outgoing
            .try_send(notification)
            .map_err(|e| anyhow::anyhow!("LSP notification queue unavailable: {e}"))
    }
}

/// Enqueue under a short document-ordering lock, then wait after releasing it.
pub(crate) struct PendingRequest {
    transport: LspTransport,
    id: i64,
    receiver: mpsc::Receiver<Response>,
    deadline: Instant,
    completed: bool,
    _timing: crate::commands::diagnostics::Timing,
}
impl PendingRequest {
    pub fn wait(mut self) -> Result<Value> {
        loop {
            if self
                .transport
                .cancellation
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
            {
                bail!("LSP request cancelled");
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("LSP request timed out after {:?}", self.transport.timeout);
            }
            match self
                .receiver
                .recv_timeout(remaining.min(Duration::from_millis(10)))
            {
                Ok(response) => {
                    self.completed = true;
                    return response.map_err(anyhow::Error::msg);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => bail!("LSP connection closed"),
            }
        }
    }
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.transport.pending.lock().unwrap().remove(&self.id);
        if !self.completed {
            let _ = self.transport.outgoing.try_send(serde_json::json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":self.id}}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encode_produces_content_length_header() {
        let body = b"hello";
        let encoded = encode_lsp_message(body);
        let s = String::from_utf8(encoded).unwrap();
        assert!(s.starts_with("Content-Length: 5\r\n\r\n"));
        assert!(s.ends_with("hello"));
    }

    #[test]
    fn encode_empty_body() {
        let encoded = encode_lsp_message(b"");
        assert_eq!(encoded, b"Content-Length: 0\r\n\r\n");
    }

    #[test]
    fn encode_length_matches_body() {
        let body = b"abc";
        let encoded = encode_lsp_message(body);
        let s = String::from_utf8_lossy(&encoded);
        assert!(s.contains("Content-Length: 3\r\n\r\n"));
    }

    #[test]
    fn decode_well_formed_message() {
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let raw = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = BufReader::new(Cursor::new(raw.as_bytes()));
        let value = decode_lsp_message(&mut reader).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], 1);
    }

    #[test]
    fn decode_ignores_unknown_headers() {
        let body = r#"{"id":2}"#;
        let raw = format!(
            "Content-Type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut reader = BufReader::new(Cursor::new(raw.as_bytes()));
        let value = decode_lsp_message(&mut reader).unwrap();
        assert_eq!(value["id"], 2);
    }

    #[test]
    fn decode_missing_content_length_errors() {
        let raw = b"Content-Type: application/json\r\n\r\n{}";
        let mut reader = BufReader::new(Cursor::new(raw));
        let err = decode_lsp_message(&mut reader).unwrap_err();
        assert!(err.to_string().contains("Content-Length"));
    }

    #[test]
    fn decode_empty_input_errors() {
        let mut reader = BufReader::new(Cursor::new(b"" as &[u8]));
        let err = decode_lsp_message(&mut reader).unwrap_err();
        assert!(err.to_string().contains("closed"));
    }

    #[test]
    fn round_trip_json_rpc_request() {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "textDocument/documentSymbol",
            "params": {"textDocument": {"uri": "file:///test.rs"}}
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let encoded = encode_lsp_message(&bytes);
        let mut reader = BufReader::new(Cursor::new(encoded));
        let decoded = decode_lsp_message(&mut reader).unwrap();
        assert_eq!(decoded["id"], 42);
        assert_eq!(decoded["method"], "textDocument/documentSymbol");
        assert_eq!(decoded["params"]["textDocument"]["uri"], "file:///test.rs");
    }

    #[test]
    fn round_trip_large_body() {
        let large_string = "x".repeat(10_000);
        let body = serde_json::json!({"data": large_string});
        let bytes = serde_json::to_vec(&body).unwrap();
        let encoded = encode_lsp_message(&bytes);

        let header_end = encoded.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let header = String::from_utf8(encoded[..header_end].to_vec()).unwrap();
        assert!(header.contains(&format!("Content-Length: {}", bytes.len())));

        let mut reader = BufReader::new(Cursor::new(encoded));
        let decoded = decode_lsp_message(&mut reader).unwrap();
        assert_eq!(decoded["data"].as_str().unwrap().len(), 10_000);
    }

    #[test]
    fn decode_multiple_sequential_messages() {
        let msg1 = serde_json::json!({"id": 1, "result": "first"});
        let msg2 = serde_json::json!({"id": 2, "result": "second"});
        let b1 = serde_json::to_vec(&msg1).unwrap();
        let b2 = serde_json::to_vec(&msg2).unwrap();
        let mut stream = encode_lsp_message(&b1);
        stream.extend(encode_lsp_message(&b2));

        let mut reader = BufReader::new(Cursor::new(stream));
        let v1 = decode_lsp_message(&mut reader).unwrap();
        let v2 = decode_lsp_message(&mut reader).unwrap();
        assert_eq!(v1["id"], 1);
        assert_eq!(v2["id"], 2);
    }
}
