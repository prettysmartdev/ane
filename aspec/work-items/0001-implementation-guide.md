# Work Item 0001: Implementation Guide
## Implement LSPEngine — centralized LSP lifecycle manager

**Last Updated:** 2026-05-10  
**Status:** Implementation Guide  
**Target:** Rust / rust-analyzer (initial implementation)

---

## Overview

This guide provides a step-by-step implementation plan for **Work Item 0001**: building the LSPEngine component. The LSPEngine is a centralized, state-machine-driven manager for the full LSP (Language Server Protocol) lifecycle in ane.

This implementation replaces the current ad-hoc `LspClient` in `src/commands/lsp/client.rs` with a production-grade engine that handles detection, installation, startup, health monitoring, request dispatch, and shutdown for any supported language.

### Key Goals

1. **Centralized Control**: LSPEngine is the sole component through which ane interacts with language servers
2. **State Machine Rigor**: Explicit, validated state transitions prevent invalid server states
3. **Multi-Language Foundation**: Architecture supports adding new languages without engine rewrites
4. **Rust-First**: Initial implementation targets rust-analyzer with patterns extensible to TypeScript, Python, etc.
5. **Headless-Ready**: Designed for both interactive TUI and headless CLI (`ane exec`) modes

---

## Architecture Rules (From CLAUDE.md)

The ane project enforces a **three-layer architecture with strict dependency direction**:

- **Layer 0** (`src/data/`): Filesystem I/O, state definitions, LSP types, registry, schemas. No imports from `commands` or `frontend`.
- **Layer 1** (`src/commands/`): Chord logic, LSP operations, LSP client, installation. Imports from `data` only.
- **Layer 2** (`src/frontend/`): CLI + TUI. Imports from `data` and `commands`.

**LSPEngine placement**: Layer 1 (`src/commands/lsp_engine/`)

**Critical Rule**: Lower layers NEVER depend on higher layers. Violating this is a build-breaking error.

---

## Implementation Phases

### Phase 1: Foundation (Layer 0)
Establish the data types and infrastructure that LSPEngine will use.

#### Phase 1.1: Expand LSP Type Definitions

**File**: `src/data/lsp/types.rs`

**Add the following enums and structs**:

```rust
// Server state machine
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerState {
    Undetected,   // No language detected
    Missing,      // Language detected, server binary not found
    Available,    // Server installed, not running
    Installing,   // Install command in progress
    Starting,     // Process spawned, initialize handshake in progress
    Running,      // Fully initialized, ready for requests
    Stopped,      // Clean shutdown
    Failed,       // Install/start failed or runtime crash
}

impl ServerState {
    /// Check if server is ready for requests
    pub fn is_ready(&self) -> bool {
        matches!(self, ServerState::Running)
    }

    /// Check if server is in a transient state (may change soon)
    pub fn is_pending(&self) -> bool {
        matches!(self, ServerState::Installing | ServerState::Starting)
    }
}

// Symbol kinds (from LSP SymbolKind enum)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Struct,
    Enum,
    Impl,
    Const,
    Field,
    Method,
    Module,
    Other, // Catch-all for unrecognized kinds
}

impl SymbolKind {
    /// Convert from LSP numeric symbol kind to ane's SymbolKind
    pub fn from_lsp_kind(kind: u32) -> Self {
        match kind {
            1 => SymbolKind::Module,
            5 => SymbolKind::Class,
            6 => SymbolKind::Struct,
            11 => SymbolKind::Function,
            12 => SymbolKind::Variable,
            13 => SymbolKind::Const,
            23 => SymbolKind::Enum,
            25 => SymbolKind::Method,
            // ... map all LSP kinds
            _ => SymbolKind::Other,
        }
    }
}

// Symbol range in a file
#[derive(Clone, Debug)]
pub struct SymbolRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl SymbolRange {
    pub fn contains_position(&self, line: usize, col: usize) -> bool {
        (line > self.start_line || (line == self.start_line && col >= self.start_col))
            && (line < self.end_line || (line == self.end_line && col <= self.end_col))
    }
}

// Document symbol (with children for nested scopes)
#[derive(Clone, Debug)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: SymbolRange,
    pub children: Vec<DocumentSymbol>,
}

impl DocumentSymbol {
    /// Find the innermost symbol containing the given position
    pub fn find_at_position(&self, line: usize, col: usize) -> Option<&DocumentSymbol> {
        if !self.range.contains_position(line, col) {
            return None;
        }

        // Check children (most specific scope first)
        for child in &self.children {
            if let Some(found) = child.find_at_position(line, col) {
                return Some(found);
            }
        }

        // No child contains it, return self
        Some(self)
    }
}

// Hover information
#[derive(Clone, Debug)]
pub struct HoverInfo {
    pub content: String, // Markdown or plain text
    pub range: Option<SymbolRange>,
}

// Completion item
#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String, // "Function", "Variable", etc.
    pub detail: Option<String>,
    pub documentation: Option<String>,
}

// Location (file + range)
#[derive(Clone, Debug)]
pub struct Location {
    pub file_path: PathBuf,
    pub range: SymbolRange,
}

// LSP events for the frontend
#[derive(Clone, Debug)]
pub enum LspEvent {
    StateChanged {
        language: Language,
        old: ServerState,
        new: ServerState,
    },
    DiagnosticsReceived {
        file_path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
    ServerMessage {
        language: Language,
        message: String,
    },
    Error {
        language: Language,
        error: String,
    },
}

// Diagnostic (from LSP)
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub range: SymbolRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}
```

**Testing**: Add unit tests for helper methods like `SymbolRange::contains_position()` and `DocumentSymbol::find_at_position()`.

#### Phase 1.2: Extend LSP Registry

**File**: `src/data/lsp/registry.rs`

Add language-specific initialization options to the `LspServerInfo` struct (or create a new `ServerConfig` type):

```rust
pub struct LspServerInfo {
    pub language: Language,
    pub name: &'static str,
    pub binary_name: &'static str,
    pub version_check_args: &'static [&'static str], // e.g., ["--version"]
    pub install_command: &'static str, // shell command to install
    pub initialization_options: Option<serde_json::Value>,
}
```

For **rust-analyzer** specifically:

```rust
pub const RUST_ANALYZER_INFO: LspServerInfo = LspServerInfo {
    language: Language::Rust,
    name: "rust-analyzer",
    binary_name: "rust-analyzer",
    version_check_args: &["--version"],
    install_command: "rustup component add rust-analyzer",
    initialization_options: Some(json!({
        "checkOnSave": false, // ane handles checking
    })),
};
```

---

### Phase 2: Core Engine Implementation (Layer 1)

Create the LSPEngine module with all subcomponents.

#### Phase 2.1: Module Structure

**Create** `src/commands/lsp_engine/` directory with:

```
src/commands/lsp_engine/
├── mod.rs              (public re-exports)
├── engine.rs           (LspEngine struct, lifecycle)
├── transport.rs        (JSON-RPC framing)
├── detector.rs         (language detection)
├── installer.rs        (binary installation)
├── health.rs           (process monitoring)
└── types.rs            (internal types)
```

Update `src/commands/mod.rs` to include:
```rust
pub mod lsp_engine;
```

#### Phase 2.2: Transport Layer (`transport.rs`)

Implement JSON-RPC 2.0 framing over pipes.

```rust
use std::io::{BufReader, BufWriter, Write, Read};
use std::process::{Child, Stdio};
use serde_json::{json, Value};

pub struct LspTransport {
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspTransport {
    pub fn new(child: &mut Child) -> anyhow::Result<Self> {
        let stdin = child.stdin.take().ok_or(anyhow::anyhow!("No stdin"))?;
        let stdout = child.stdout.take().ok_or(anyhow::anyhow!("No stdout"))?;

        Ok(LspTransport {
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// Allocate a unique request ID
    pub fn next_request_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Send a JSON-RPC request with Content-Length framing
    pub fn send_request(&mut self, method: &str, params: Value, id: i64) -> anyhow::Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id,
        });

        self.send_raw(&request)
    }

    /// Send a JSON-RPC notification (no ID, no response expected)
    pub fn send_notification(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        self.send_raw(&notification)
    }

    /// Internal: encode with Content-Length and send
    fn send_raw(&mut self, message: &Value) -> anyhow::Result<()> {
        let content = serde_json::to_string(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        self.writer.write_all(header.as_bytes())?;
        self.writer.write_all(content.as_bytes())?;
        self.writer.flush()?;

        Ok(())
    }

    /// Read the next JSON-RPC message from the server (blocking)
    pub fn read_message(&mut self) -> anyhow::Result<Value> {
        // Parse Content-Length header
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line)?;
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }

        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .and_then(|s| s.trim().parse::<usize>().ok())
            })
            .ok_or(anyhow::anyhow!("No Content-Length header"))?;

        // Read exactly content_length bytes
        let mut buffer = vec![0u8; content_length];
        self.reader.read_exact(&mut buffer)?;

        let message = serde_json::from_slice(&buffer)?;
        Ok(message)
    }
}
```

**Testing**: Unit tests for Content-Length encoding/decoding, round-trip JSON serialization.

#### Phase 2.3: Detector (`detector.rs`)

Identify which languages are present in a context.

```rust
use std::path::Path;
use crate::data::lsp::Language;
use crate::data::lsp::registry; // Provides file extension mappings

pub struct LanguageDetector;

impl LanguageDetector {
    /// Detect all languages present in a directory and file list
    pub fn detect_languages(root: &Path, files: &[&Path]) -> anyhow::Result<Vec<Language>> {
        let mut languages = std::collections::HashSet::new();

        // Check project markers (Cargo.toml for Rust, package.json for TypeScript, etc.)
        if root.join("Cargo.toml").exists() {
            languages.insert(Language::Rust);
        }

        // Check file extensions
        for file in files {
            if let Some(ext) = file.extension() {
                if let Some(lang) = registry::language_by_extension(ext) {
                    languages.insert(lang);
                }
            }
        }

        Ok(languages.into_iter().collect())
    }

    /// Detect language from a single file path
    pub fn detect_from_file(path: &Path) -> Option<Language> {
        path.extension()
            .and_then(|ext| registry::language_by_extension(ext))
    }
}
```

#### Phase 2.4: Installer (`installer.rs`)

Check and install language server binaries.

```rust
use std::path::Path;
use std::process::Command;
use crate::data::lsp::{Language, registry};

pub struct ServerInstaller;

impl ServerInstaller {
    /// Check if the server binary is installed
    pub fn is_installed(lang: Language) -> anyhow::Result<bool> {
        let info = registry::get_server_info(lang)?;
        let output = Command::new("which")
            .arg(info.binary_name)
            .output()?;

        Ok(output.status.success())
    }

    /// Install the server binary (runs the install command)
    pub fn install(lang: Language) -> anyhow::Result<()> {
        let info = registry::get_server_info(lang)?;

        let output = Command::new("sh")
            .arg("-c")
            .arg(info.install_command)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Installation of {} failed: {}",
                info.name,
                stderr
            );
        }

        Ok(())
    }
}
```

#### Phase 2.5: Health Monitor (`health.rs`)

Watch for process crashes and timeout detection.

```rust
use std::time::{Duration, Instant};
use std::process::Child;

pub struct HealthMonitor {
    start_time: Instant,
    startup_timeout: Duration,
}

impl HealthMonitor {
    pub fn new(startup_timeout: Duration) -> Self {
        HealthMonitor {
            start_time: Instant::now(),
            startup_timeout,
        }
    }

    /// Check if startup has timed out
    pub fn is_startup_timeout(&self) -> bool {
        self.start_time.elapsed() > self.startup_timeout
    }

    /// Try to detect if child process exited (non-blocking)
    pub fn process_exited(child: &mut Child) -> anyhow::Result<bool> {
        match child.try_wait()? {
            Some(status) => Ok(!status.success()),
            None => Ok(false),
        }
    }
}
```

#### Phase 2.6: Engine Core (`engine.rs`)

The main `LspEngine` struct orchestrating all components.

```rust
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::process::{Command, Child};

use crate::data::lsp::{Language, ServerState, LspEvent, DocumentSymbol, SymbolRange};
use super::transport::LspTransport;
use super::detector::LanguageDetector;
use super::installer::ServerInstaller;
use super::health::HealthMonitor;

pub struct LspEngineConfig {
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    pub auto_install: bool,
}

impl Default for LspEngineConfig {
    fn default() -> Self {
        LspEngineConfig {
            startup_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            auto_install: true,
        }
    }
}

pub struct ServerInstance {
    language: Language,
    state: ServerState,
    process: Option<Child>,
    transport: Option<LspTransport>,
    capabilities: Option<serde_json::Value>, // ServerCapabilities
}

pub struct LspEngine {
    servers: HashMap<Language, ServerInstance>,
    config: LspEngineConfig,
    event_tx: Sender<LspEvent>,
    event_rx: Receiver<LspEvent>,
}

impl LspEngine {
    pub fn new(config: LspEngineConfig) -> Self {
        let (tx, rx) = channel();
        LspEngine {
            servers: HashMap::new(),
            config,
            event_tx: tx,
            event_rx: rx,
        }
    }

    /// Scan context and start servers for detected languages
    pub fn start_for_context(&mut self, root_path: &Path, files: &[&Path]) -> anyhow::Result<()> {
        let languages = LanguageDetector::detect_languages(root_path, files)?;

        for lang in languages {
            self.start_server(lang)?;
        }

        Ok(())
    }

    /// Start a specific language server (non-blocking)
    fn start_server(&mut self, lang: Language) -> anyhow::Result<()> {
        // Check if installed
        if !ServerInstaller::is_installed(lang)? {
            // Try auto-install
            if self.config.auto_install {
                self.state_transition(lang, ServerState::Installing)?;
                ServerInstaller::install(lang)
                    .map_err(|e| {
                        self.emit_event(LspEvent::Error {
                            language: lang,
                            error: format!("Installation failed: {}", e),
                        });
                        e
                    })?;
            } else {
                self.state_transition(lang, ServerState::Missing)?;
                return Ok(());
            }
        }

        self.state_transition(lang, ServerState::Available)?;
        self.state_transition(lang, ServerState::Starting)?;

        // Spawn process in background (simplified; real impl uses threads)
        let registry_info = crate::data::lsp::registry::get_server_info(lang)?;
        let mut child = Command::new(registry_info.binary_name)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;

        let mut transport = LspTransport::new(&mut child)?;

        // Send initialize request
        let init_request = serde_json::json!({
            "processId": std::process::id(),
            "rootPath": "/workspace", // TODO: use actual root
            "capabilities": {},
        });

        let request_id = transport.next_request_id();
        transport.send_request("initialize", init_request, request_id)?;

        // TODO: Read initialize response in background thread

        let instance = ServerInstance {
            language: lang,
            state: ServerState::Starting,
            process: Some(child),
            transport: Some(transport),
            capabilities: None,
        };

        self.servers.insert(lang, instance);

        // TODO: Spawn background thread to handle handshake

        Ok(())
    }

    /// Get server state
    pub fn server_state(&self, lang: Language) -> ServerState {
        self.servers
            .get(&lang)
            .map(|s| s.state)
            .unwrap_or(ServerState::Undetected)
    }

    /// Emit an LspEvent
    fn emit_event(&self, event: LspEvent) {
        let _ = self.event_tx.send(event); // Ignore send errors
    }

    /// Transition server state (with validation)
    fn state_transition(&mut self, lang: Language, new_state: ServerState) -> anyhow::Result<()> {
        let entry = self.servers.entry(lang).or_insert(ServerInstance {
            language: lang,
            state: ServerState::Undetected,
            process: None,
            transport: None,
            capabilities: None,
        });

        let old_state = entry.state;

        // Validate transition
        self.validate_transition(old_state, new_state)?;

        entry.state = new_state;
        self.emit_event(LspEvent::StateChanged {
            language: lang,
            old: old_state,
            new: new_state,
        });

        Ok(())
    }

    /// Validate state machine transitions
    fn validate_transition(&self, old: ServerState, new: ServerState) -> anyhow::Result<()> {
        let valid = matches!(
            (old, new),
            (ServerState::Undetected, ServerState::Missing)
                | (ServerState::Undetected, ServerState::Available)
                | (ServerState::Missing, ServerState::Installing)
                | (ServerState::Missing, ServerState::Available)
                | (ServerState::Available, ServerState::Starting)
                | (ServerState::Installing, ServerState::Available)
                | (ServerState::Installing, ServerState::Failed)
                | (ServerState::Starting, ServerState::Running)
                | (ServerState::Starting, ServerState::Failed)
                | (ServerState::Running, ServerState::Stopped)
                | (ServerState::Running, ServerState::Failed)
                | (ServerState::Failed, ServerState::Available) // Retry
                | (_, ServerState::Stopped) // Any state can stop
        );

        if !valid {
            anyhow::bail!("Invalid state transition: {:?} -> {:?}", old, new);
        }

        Ok(())
    }

    /// Drain pending events
    pub fn poll_events(&self) -> Vec<LspEvent> {
        self.event_rx.try_iter().collect()
    }

    /// Get status summary
    pub fn status_summary(&self) -> Vec<(Language, ServerState)> {
        self.servers
            .iter()
            .map(|(lang, instance)| (*lang, instance.state))
            .collect()
    }

    /// Shut down all servers gracefully
    pub fn shutdown_all(&mut self) -> anyhow::Result<()> {
        for (lang, instance) in self.servers.iter_mut() {
            // Send shutdown request if running
            if instance.state == ServerState::Running {
                if let Some(transport) = instance.transport.as_mut() {
                    let id = transport.next_request_id();
                    let _ = transport.send_request("shutdown", serde_json::json!({}), id);
                }
            }

            // Kill process
            if let Some(mut child) = instance.process.take() {
                let _ = child.kill();
                let _ = child.wait();
            }

            self.state_transition(*lang, ServerState::Stopped)?;
        }

        Ok(())
    }

    // Placeholder query methods (to be implemented with actual LSP requests)
    pub fn document_symbols(&mut self, _file_path: &Path) -> anyhow::Result<Vec<DocumentSymbol>> {
        // TODO: Send textDocument/documentSymbol request
        Ok(vec![])
    }

    pub fn symbol_at_position(&mut self, _file_path: &Path, _line: usize, _col: usize) -> anyhow::Result<Option<DocumentSymbol>> {
        // TODO: Implement using document_symbols + search
        Ok(None)
    }

    pub fn symbol_range(&mut self, _file_path: &Path, _symbol_name: &str) -> anyhow::Result<Option<SymbolRange>> {
        // TODO: Send textDocument/documentSymbol and find by name
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_state_transitions() {
        let engine = LspEngine::new(LspEngineConfig::default());

        // Test a valid transition path
        assert!(engine.validate_transition(ServerState::Undetected, ServerState::Missing).is_ok());
        assert!(engine.validate_transition(ServerState::Missing, ServerState::Installing).is_ok());
        assert!(engine.validate_transition(ServerState::Installing, ServerState::Available).is_ok());
        assert!(engine.validate_transition(ServerState::Available, ServerState::Starting).is_ok());
        assert!(engine.validate_transition(ServerState::Starting, ServerState::Running).is_ok());
    }

    #[test]
    fn test_invalid_state_transitions() {
        let engine = LspEngine::new(LspEngineConfig::default());

        // Test invalid transitions
        assert!(engine.validate_transition(ServerState::Running, ServerState::Undetected).is_err());
        assert!(engine.validate_transition(ServerState::Installing, ServerState::Running).is_err());
    }
}
```

#### Phase 2.7: Public Module Interface (`mod.rs`)

```rust
pub mod engine;
pub mod transport;
pub mod detector;
pub mod installer;
pub mod health;

pub use engine::{LspEngine, LspEngineConfig, ServerInstance};
pub use transport::LspTransport;
pub use detector::LanguageDetector;
pub use installer::ServerInstaller;
pub use health::HealthMonitor;
```

---

### Phase 3: Integration & Testing

#### Phase 3.1: Update Cargo.toml

Add required dependencies:

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
lsp-types = "0.95"  # LSP protocol types (optional but recommended)
anyhow = "1.0"
```

#### Phase 3.2: Integration with Frontend

**In `src/frontend/`** (Layer 2), add code to use LSPEngine:

```rust
// src/frontend/tui.rs or src/frontend/cli.rs
use crate::commands::lsp_engine::LspEngine;

pub struct Editor {
    lsp_engine: LspEngine,
    // ... other fields
}

impl Editor {
    pub fn new() -> anyhow::Result<Self> {
        let lsp_config = LspEngineConfig::default();
        let lsp_engine = LspEngine::new(lsp_config);

        Ok(Editor {
            lsp_engine,
            // ...
        })
    }

    pub fn on_file_open(&mut self, file_path: &Path) -> anyhow::Result<()> {
        self.lsp_engine.start_for_context(file_path.parent().unwrap_or(Path::new(".")), &[file_path])?;
        Ok(())
    }
}
```

#### Phase 3.3: Unit Tests

Create comprehensive unit tests:

**`src/commands/lsp_engine/engine.rs` tests**:
- State machine validation
- Valid and invalid transitions
- Event emission
- Server instance lifecycle

**`src/commands/lsp_engine/transport.rs` tests**:
- Content-Length encoding/decoding
- JSON serialization round-trip
- Message framing

**`src/commands/lsp_engine/detector.rs` tests**:
- Language detection by file extension
- Project marker detection (Cargo.toml, etc.)
- Edge cases (unknown extensions, nested projects)

**`src/commands/lsp_engine/installer.rs` tests**:
- Mock binary checks
- Mock install commands
- Error handling for missing binaries

#### Phase 3.4: Integration Tests

Create a mock LSP server for testing without rust-analyzer:

**`tests/mock_lsp_server.rs`**:
```rust
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Child, Stdio};

/// A minimal mock LSP server for testing
pub struct MockLspServer {
    child: Child,
}

impl MockLspServer {
    pub fn start() -> std::io::Result<Self> {
        let child = Command::new("node")
            .arg("tests/mock_lsp.js") // Simple Node.js mock
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        Ok(MockLspServer { child })
    }

    pub fn stop(mut self) -> std::io::Result<()> {
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::lsp_engine::LspEngine;

    #[test]
    fn test_initialize_handshake() {
        // Start mock server
        let _server = MockLspServer::start().expect("Failed to start mock server");

        // Test LSPEngine initialization
        // (Requires connecting engine to mock server process)
    }

    #[test]
    fn test_document_symbols_request() {
        // Similar integration test structure
    }
}
```

---

## Concurrency & Async Patterns

### Current Approach: Synchronous Core

The initial LSPEngine is **synchronous from the caller's perspective**:
- All public methods take `&mut self` (single-threaded)
- Callers block on operations or poll for status

### Background Startup (Future Enhancement)

For non-blocking startup:

```rust
impl LspEngine {
    /// Start in background, return immediately
    pub fn start_for_context_async(&mut self, root_path: &Path, files: &[&Path]) -> anyhow::Result<()> {
        let (servers, config) = (/* clone state */);

        std::thread::spawn(move || {
            // Perform startup in background
            // Update shared Arc<Mutex<ServerState>> on completion
        });

        Ok(())
    }

    /// Block until server reaches Ready or Failed state
    pub fn await_ready(&self, lang: Language, timeout: Duration) -> anyhow::Result<ServerState> {
        let start = std::time::Instant::now();
        loop {
            let state = self.server_state(lang);
            if state.is_ready() || matches!(state, ServerState::Failed) {
                return Ok(state);
            }
            if start.elapsed() > timeout {
                anyhow::bail!("Server startup timeout");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
```

---

## Error Handling Strategy

All public methods return `anyhow::Result<T>`:

```rust
pub fn start_for_context(&mut self, root_path: &Path, files: &[&Path]) -> anyhow::Result<()> {
    // Detection errors → clear message
    let languages = LanguageDetector::detect_languages(root_path, files)
        .context("Failed to detect languages")?;

    for lang in languages {
        // Install errors → include which language
        self.start_server(lang)
            .with_context(|| format!("Failed to start {} server", lang))?;
    }

    Ok(())
}
```

Server-side errors (crashes, handshake failures) are **not** propagated as exceptions; instead, they:
1. Transition the server to `Failed` state
2. Emit an `LspEvent::Error` for the frontend to display

---

## Deprecation & Cleanup

Once LSPEngine is complete and integrated:

1. **Remove** `src/commands/lsp/client.rs` (replaced by engine)
2. **Remove** `src/commands/lsp/install.rs` (replaced by installer submodule)
3. **Update** any direct uses of old LSP client to go through LSPEngine
4. **Verify** all tests pass with the new implementation

---

## Implementation Checklist

### Layer 0 (Data)
- [ ] Expand `src/data/lsp/types.rs` with ServerState, DocumentSymbol, etc.
- [ ] Extend `src/data/lsp/registry.rs` with language-specific init options
- [ ] Add `serde`, `serde_json`, `lsp-types` to Cargo.toml
- [ ] Unit tests for type conversions and helpers

### Layer 1 (Engine)
- [ ] Create `src/commands/lsp_engine/` module structure
- [ ] Implement `transport.rs` with JSON-RPC framing
- [ ] Implement `detector.rs` with language detection
- [ ] Implement `installer.rs` with binary checks and install
- [ ] Implement `health.rs` with process monitoring
- [ ] Implement `engine.rs` with state machine and core logic
- [ ] Implement `mod.rs` with public exports
- [ ] State transition validation tests
- [ ] Transport encoding/decoding tests
- [ ] Detector language detection tests
- [ ] Integration test with mock LSP server

### Layer 2 (Frontend Integration)
- [ ] Integrate LSPEngine into TUI startup
- [ ] Subscribe to LspEvent channel in status bar
- [ ] CLI mode: call `start_for_context()` and `await_ready()`
- [ ] Error display for failed servers

### Cleanup
- [ ] Remove old `src/commands/lsp/client.rs`
- [ ] Remove old `src/commands/lsp/install.rs`
- [ ] Update documentation
- [ ] Run full test suite

---

## Success Criteria

1. **Builds cleanly** with no clippy warnings
2. **All unit tests pass** (state machine, transport, detection)
3. **Integration test passes** with mock LSP server
4. **TUI status bar** shows server state (Undetected → Running)
5. **CLI mode** can execute LSP-dependent chords with proper await/timeout
6. **Rust-analyzer** launches, initializes, and responds to document symbol requests
7. **Edge cases handled**:
   - Missing binary → clear error message
   - Timeout during handshake → `Failed` state
   - Process crash → `Failed` state with recovery option
   - No language detected → `Undetected` state (not an error)

---

## Future Extensions

Once the initial Rust implementation is solid:

1. **Add TypeScript** via `typescript-language-server`
2. **Add Python** via `pylance` or `pyright`
3. **Request queueing** for concurrent chord operations
4. **Incremental document sync** instead of full sync
5. **Workspace folder support** for monorepos
6. **Diagnostic rendering** in the editor

---

## References

- **LSP Specification**: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
- **rust-analyzer Manual**: https://rust-analyzer.github.io/
- **Project Architecture**: `/workspace/aspec/architecture/lsp-engine.md`
- **Work Item**: `/workspace/aspec/work-items/0001-lsp-engine.md`
- **CLAUDE.md**: Project rules and conventions

