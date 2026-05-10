# LSPEngine

Layer: 1 (commands)
Module: `src/commands/lsp_engine/`
Dependencies: Layer 0 (`data::lsp::types`, `data::lsp::registry`)

## Purpose

The LSPEngine is ane's centralized LSP lifecycle manager. It is the sole component through which all other parts of ane discover, install, launch, configure, query, and monitor language servers. No other component interacts with LSP processes directly — they go through LSPEngine's public API.

## Design Principles

### Single Responsibility
LSPEngine owns the entire LSP lifecycle: detection, installation, startup, health monitoring, request dispatch, and shutdown. Other components never spawn LSP processes, parse JSON-RPC, or check installation status themselves.

### State Machine
Each managed LSP server follows a strict state machine with well-defined transitions. The engine enforces these transitions — no component can force a server into an invalid state.

### Opaque API
Callers ask semantic questions ("give me the symbols in this file") and receive structured Rust types. They never construct JSON-RPC messages, manage process handles, or know which LSP server binary is running.

### Multi-Language
LSPEngine manages zero or more LSP servers concurrently, one per detected language. A workspace with both Rust and TypeScript files would have two servers tracked independently.

## State Machine

Each LSP server instance managed by the engine transitions through these states:

```
                    ┌─────────────┐
                    │  Undetected │ (no language detected for context)
                    └──────┬──────┘
                           │ language detected
                           ▼
                    ┌─────────────┐
          ┌────────│   Missing   │ (server binary not found)
          │        └──────┬──────┘
          │               │ binary found
          │               ▼
          │        ┌─────────────┐
          │        │  Available  │ (installed but not running)
          │        └──────┬──────┘
          │               │ start requested
          │               ▼
          │        ┌─────────────┐
          │        │  Starting   │ (process spawned, initialize handshake in progress)
          │        └──────┬──────┘
          │               │ initialized response received
          │               ▼
          │        ┌─────────────┐
          │        │   Running   │ (ready to handle requests)
          │        └──────┬──────┘
          │               │ shutdown/crash
          │               ▼
          │        ┌─────────────┐
          │        │   Stopped   │ (clean shutdown or killed)
          │        └─────────────┘
          │
          │ install triggered
          ▼
   ┌─────────────┐
   │ Installing  │ (install command running)
   └──────┬──────┘
          │ success → Available
          │ failure → Failed
          ▼
   ┌─────────────┐
   │   Failed    │ (install failed, start failed, or runtime crash)
   └─────────────┘
```

### Transition Rules
- `Missing → Installing`: only when auto-install is enabled or user confirms
- `Installing → Available`: install command exits 0
- `Installing → Failed`: install command exits non-zero
- `Available → Starting`: engine calls start
- `Starting → Running`: LSP initialize/initialized handshake completes
- `Starting → Failed`: process crashes or handshake times out
- `Running → Stopped`: clean shutdown sequence (shutdown request → exit notification → process wait)
- `Running → Failed`: process crashes, pipe broken, or unrecoverable error
- `Failed → Available`: retry after fix (e.g., user installs binary manually)
- Any state → `Stopped`: on ane exit (engine drops all servers)

## Architecture

### Core Struct: `LspEngine`

```
LspEngine
├── servers: HashMap<Language, ServerInstance>
├── config: LspEngineConfig
└── event_tx: Sender<LspEvent>

ServerInstance
├── language: Language
├── state: ServerState
├── server_info: &'static LspServerInfo
├── process: Option<Child>
├── transport: Option<LspTransport>
├── capabilities: Option<ServerCapabilities>
└── pending_requests: HashMap<RequestId, PendingRequest>

LspTransport
├── writer: BufWriter<ChildStdin>
├── reader: BufReader<ChildStdout>
└── next_id: AtomicI64
```

### Internal Components

**Detector**: Scans the working directory and open files to determine which languages are present. Uses `data::lsp::registry` for file extension mapping and project marker detection (e.g., `Cargo.toml` → Rust).

**Installer**: Checks if the required binary exists. If missing, runs the registered install command. Reports progress via `LspEvent` channel.

**Transport**: Handles JSON-RPC 2.0 framing over stdin/stdout pipes. Encodes `Content-Length` headers, serializes requests, deserializes responses. Handles notification dispatch (server-initiated messages).

**Health Monitor**: Watches for process exit, broken pipes, and request timeouts. Transitions server state to `Failed` on anomalies and emits diagnostic events.

## Public API

The API is organized into lifecycle management and LSP queries. All methods take `&mut self` or `&self` and return `Result<T>`.

### Lifecycle

```rust
impl LspEngine {
    pub fn new(config: LspEngineConfig) -> Self;

    // Scan context and start servers for all detected languages.
    // Non-blocking: spawns servers in background, returns immediately.
    pub fn start_for_context(&mut self, root_path: &Path, files: &[&Path]) -> Result<()>;

    // Get current state for a specific language's server.
    pub fn server_state(&self, lang: Language) -> ServerState;

    // Wait until the server for `lang` reaches Running or Failed.
    // Returns the final state. Respects timeout.
    pub fn await_ready(&self, lang: Language, timeout: Duration) -> Result<ServerState>;

    // Check if any server is in a pending state (Starting, Installing).
    pub fn any_pending(&self) -> bool;

    // Drain pending LspEvents (status changes, diagnostics, errors).
    pub fn poll_events(&mut self) -> Vec<LspEvent>;

    // Gracefully shut down all servers.
    pub fn shutdown_all(&mut self);

    // Attempt to install a missing server.
    pub fn install_server(&mut self, lang: Language) -> Result<()>;

    // Get a summary of all managed servers and their states.
    pub fn status_summary(&self) -> Vec<(Language, ServerState)>;
}
```

### LSP Queries

These methods abstract over the JSON-RPC protocol entirely. Callers provide file paths and positions; they receive structured Rust types.

```rust
impl LspEngine {
    // Get document symbols (functions, structs, variables, etc.)
    pub fn document_symbols(&mut self, file_path: &Path) -> Result<Vec<DocumentSymbol>>;

    // Find the symbol at a given position (line, col) in a file.
    pub fn symbol_at_position(&mut self, file_path: &Path, line: usize, col: usize) -> Result<Option<DocumentSymbol>>;

    // Get the full range (start line/col, end line/col) of a named symbol.
    pub fn symbol_range(&mut self, file_path: &Path, symbol_name: &str) -> Result<Option<SymbolRange>>;

    // Notify the server that a document was opened/changed.
    pub fn notify_document_open(&mut self, file_path: &Path, content: &str) -> Result<()>;
    pub fn notify_document_change(&mut self, file_path: &Path, content: &str, version: i32) -> Result<()>;

    // Request completions at a position.
    pub fn completions(&mut self, file_path: &Path, line: usize, col: usize) -> Result<Vec<CompletionItem>>;

    // Request hover info at a position.
    pub fn hover(&mut self, file_path: &Path, line: usize, col: usize) -> Result<Option<HoverInfo>>;

    // Go to definition.
    pub fn goto_definition(&mut self, file_path: &Path, line: usize, col: usize) -> Result<Option<Location>>;
}
```

### Return Types

All query methods return ane-native types defined in `data::lsp::types`, not raw JSON. The engine handles deserialization internally.

```rust
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: SymbolRange,
    pub children: Vec<DocumentSymbol>,
}

pub struct SymbolRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

pub enum SymbolKind {
    Function, Variable, Struct, Enum, Impl, Const, Field, Method, Module, Other(String),
}
```

## Event System

LSPEngine emits events via a channel so the frontend can react to state changes without polling.

```rust
pub enum LspEvent {
    StateChanged { language: Language, old: ServerState, new: ServerState },
    DiagnosticsReceived { file_path: PathBuf, diagnostics: Vec<Diagnostic> },
    ServerMessage { language: Language, message: String },
    Error { language: Language, error: String },
}
```

The TUI status bar subscribes to these events to update the LSP status indicator. The CLI mode checks state synchronously before executing LSP-dependent chords.

## Concurrency Model

- The engine itself is single-threaded from the caller's perspective — all public methods are `&mut self`.
- LSP server processes run as child processes with piped stdin/stdout.
- Background startup uses `std::thread::spawn` internally: the engine spawns a thread that performs the initialize handshake, then updates `ServerState` atomically. The caller can poll via `server_state()` or block via `await_ready()`.
- Request/response correlation uses monotonically increasing integer IDs per server.

## Error Handling

- All public methods return `anyhow::Result<T>`.
- Server crashes transition to `Failed` state and emit an `LspEvent::Error`.
- Request timeouts return `Err` to the caller without crashing the engine.
- Install failures transition to `Failed` with a descriptive message including the install command that was attempted.
- The engine never panics — all internal errors are captured and surfaced through the API or event channel.

## Integration Points

### ChordEngine (Layer 1)
The ChordEngine's resolver stage calls `LspEngine::document_symbols()` and `LspEngine::symbol_range()` to locate constructs in buffers. If the engine reports `ServerState::Missing` or `Failed`, the resolver returns a descriptive error. If `Starting`, the resolver waits via `await_ready()`.

### Frontend (Layer 2)
- **TUI**: Subscribes to `LspEvent` channel. Status bar shows current server state. The TUI calls `start_for_context()` at launch and `shutdown_all()` on exit.
- **CLI**: Calls `start_for_context()` synchronously, then `await_ready()` with a timeout before executing LSP-dependent chords.

### Data Layer (Layer 0)
LSPEngine reads from `data::lsp::registry` for server definitions and `data::lsp::types` for shared type definitions. It does not write to Layer 0.
