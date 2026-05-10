use std::io::{BufRead, BufReader, Write};

fn read_msg(reader: &mut impl BufRead) -> Option<String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length = len_str.parse().ok();
        }
    }
    let length = content_length?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

fn write_msg(out: &mut impl Write, content: &str) {
    write!(out, "Content-Length: {}\r\n\r\n{}", content.len(), content).unwrap();
    out.flush().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version") {
        println!("mock-lsp-server 0.0.1");
        return;
    }

    if args.iter().any(|a| a == "--hang") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    while let Some(msg) = read_msg(&mut reader) {
        let value: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let method = value
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let id = value.get("id").cloned();

        match method.as_str() {
            "initialize" => {
                if let Some(id) = id {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "capabilities": {
                                "documentSymbolProvider": true,
                                "textDocumentSync": 1
                            }
                        }
                    });
                    write_msg(&mut writer, &resp.to_string());
                }
            }
            "textDocument/documentSymbol" => {
                if let Some(id) = id {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": [
                            {
                                "name": "main",
                                "kind": 12,
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 10, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 0, "character": 3},
                                    "end": {"line": 0, "character": 7}
                                },
                                "children": []
                            }
                        ]
                    });
                    write_msg(&mut writer, &resp.to_string());
                }
            }
            "shutdown" => {
                if let Some(id) = id {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": null
                    });
                    write_msg(&mut writer, &resp.to_string());
                }
            }
            "exit" => break,
            _ => {
                if let Some(id) = id {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": null
                    });
                    write_msg(&mut writer, &resp.to_string());
                }
            }
        }
    }
}
