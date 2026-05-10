use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};

use crate::data::lsp::registry;
use crate::data::lsp::types::{Language, LspInitParams, LspStatus};

pub struct LspClient {
    pub language: Language,
    pub status: Arc<Mutex<LspStatus>>,
    process: Option<Child>,
    request_id: i64,
}

impl LspClient {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            status: Arc::new(Mutex::new(LspStatus::Unknown)),
            process: None,
            request_id: 0,
        }
    }

    pub fn get_status(&self) -> LspStatus {
        *self.status.lock().unwrap()
    }

    fn set_status(&self, status: LspStatus) {
        *self.status.lock().unwrap() = status;
    }

    pub fn start(&mut self, params: &LspInitParams) -> Result<()> {
        let server = match registry::server_for_language(params.language) {
            Some(s) => s,
            None => {
                self.set_status(LspStatus::Failed);
                bail!("no LSP server registered for {:?}", params.language);
            }
        };

        if !super::install::is_installed(server) {
            self.set_status(LspStatus::NotInstalled);
            bail!(
                "{} is not installed. Run: {}",
                server.server_name,
                server.install_command
            );
        }

        self.set_status(LspStatus::Starting);

        let mut cmd = Command::new(server.binary_name);
        cmd.args(server.default_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let child = cmd.spawn()?;
        self.process = Some(child);

        self.send_initialize(&params.root_path)?;

        self.set_status(LspStatus::Ready);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(ref mut child) = self.process {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.process = None;
        self.set_status(LspStatus::Unknown);
    }

    fn next_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    fn send_initialize(&mut self, root_path: &std::path::Path) -> Result<()> {
        let id = self.next_id();
        let root_uri = format!("file://{}", root_path.display());

        let params = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"initialize","params":{{"processId":{},"rootUri":"{}","capabilities":{{}}}}}}"#,
            id,
            std::process::id(),
            root_uri,
        );

        self.send_message(&params)?;
        self.read_response()?;

        let initialized = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
        self.send_message(initialized)?;

        Ok(())
    }

    fn send_message(&mut self, content: &str) -> Result<()> {
        let child = self.process.as_mut().unwrap();
        let stdin = child.stdin.as_mut().unwrap();
        let msg = format!("Content-Length: {}\r\n\r\n{}", content.len(), content);
        stdin.write_all(msg.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<String> {
        let child = self.process.as_mut().unwrap();
        let stdout = child.stdout.as_mut().unwrap();
        let mut reader = BufReader::new(stdout);

        let mut content_length: usize = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            if let Some(len_str) = header.strip_prefix("Content-Length: ") {
                content_length = len_str.parse()?;
            }
        }

        let mut body = vec![0u8; content_length];
        std::io::Read::read_exact(&mut reader, &mut body)?;
        Ok(String::from_utf8(body)?)
    }

    pub fn request_document_symbols(&mut self, file_uri: &str) -> Result<String> {
        let id = self.next_id();
        let msg = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"textDocument/documentSymbol","params":{{"textDocument":{{"uri":"{}"}}}}}}"#,
            id, file_uri,
        );
        self.send_message(&msg)?;
        self.read_response()
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_initial_status() {
        let client = LspClient::new(Language::Rust);
        assert_eq!(client.get_status(), LspStatus::Unknown);
    }

    #[test]
    fn client_status_arc_shared() {
        let client = LspClient::new(Language::Rust);
        let status = Arc::clone(&client.status);
        assert_eq!(*status.lock().unwrap(), LspStatus::Unknown);
    }
}
