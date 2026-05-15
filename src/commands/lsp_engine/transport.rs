use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{ChildStdin, ChildStdout};

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

pub struct LspTransport {
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 0,
        }
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let content = serde_json::to_string(&request)?;
        self.write_message(content.as_bytes())?;

        loop {
            let msg = self.read_message()?;
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                if let Some(error) = msg.get("error") {
                    let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    bail!("LSP error {}: {}", code, message);
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    pub fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        };
        let content = serde_json::to_string(&notification)?;
        self.write_message(content.as_bytes())?;
        Ok(())
    }

    fn next_id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn write_message(&mut self, content: &[u8]) -> Result<()> {
        self.writer.write_all(&encode_lsp_message(content))?;
        self.writer.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<Value> {
        decode_lsp_message(&mut self.reader)
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
