# Work Item: Feature

Title: Implement LSPEngine — centralized LSP lifecycle manager
Issue: N/A

## Summary:
- Build the LSPEngine component as described in `aspec/architecture/lsp-engine.md`
- Replaces the current ad-hoc `LspClient` in `src/commands/lsp/client.rs` with a state-machine-driven engine that owns the full LSP lifecycle
- Only Rust (rust-analyzer) is implemented as the first language, but the architecture must support adding new languages by adding a registry entry and (if needed) a language-specific adapter
- The engine is the sole component through which all of ane interacts with language servers

## User Stories

### User Story 1:
As a: Developer

I want to:
Open a Rust project in ane and have the LSP server automatically detected, installed if needed, and started in the background

So I can:
Use language-aware chords (e.g., `cifb` — ChangeInsideFunctionBody) as soon as the server is ready, without manual setup

### User Story 2:
As a: Code Agent

I want to:
Execute an LSP-dependent chord via `ane exec` and have the engine block until the LSP is ready (with a timeout), or receive a clear error if the LSP is unavailable

So I can:
Reliably run language-aware chords in headless mode without managing LSP processes myself

### User Story 3:
As a: Developer

I want to:
See the LSP server's current state (starting, running, failed, etc.) in the TUI status bar and receive clear error messages when something goes wrong

So I can:
Understand why a chord isn't working and take corrective action (e.g., install the language server)

## Implementation Details:

### Research Phase
- Study the LSP specification (https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) to understand the full initialize handshake, capabilities negotiation, document synchronization, and shutdown protocol
- Research existing Rust LSP client libraries: evaluate `tower-lsp`, `lsp-types`, and `lsp-server` crates for suitability. Determine whether to use `lsp-types` for type definitions while implementing transport manually (preferred for control) or adopt a higher-level client framework
- Study how rust-analyzer specifically behaves: its startup time, required capabilities, initialization options, and common failure modes
- Research JSON-RPC 2.0 framing: `Content-Length` header parsing, message batching, notification vs request/response distinction
- Understand document synchronization models: full sync vs incremental sync, and which rust-analyzer prefers
- Research process management patterns in Rust: `std::process::Child` lifecycle, pipe buffering, detecting process crashes, graceful shutdown

### Architecture (Layer 0 — Data)
- Expand `src/data/lsp/types.rs`:
  - Add `ServerState` enum with states: `Undetected`, `Missing`, `Available`, `Installing`, `Starting`, `Running`, `Stopped`, `Failed`
  - Add `DocumentSymbol` struct with `name`, `kind: SymbolKind`, `range: SymbolRange`, `children: Vec<DocumentSymbol>`
  - Add `SymbolKind` enum: `Function`, `Variable`, `Struct`, `Enum`, `Impl`, `Const`, `Field`, `Method`, `Module`, `Other(String)`
  - Add `SymbolRange` struct with `start_line`, `start_col`, `end_line`, `end_col`
  - Add `LspEvent` enum for state changes, diagnostics, errors
  - Add `CompletionItem`, `HoverInfo`, `Location` types as needed for API
  - Decide whether to use the `lsp-types` crate for protocol types and convert at the boundary, or define ane-native types throughout
- Keep `LspServerInfo` and `Language` enum in their current location but extend as needed
- Extend `src/data/lsp/registry.rs` to support language-specific initialization options

### Architecture (Layer 1 — Commands)
- Create `src/commands/lsp_engine/` module directory with:
  - `mod.rs` — re-exports `LspEngine` and public types
  - `engine.rs` — core `LspEngine` struct, lifecycle methods, server management
  - `transport.rs` — JSON-RPC framing (Content-Length header encode/decode, message serialization/deserialization)
  - `detector.rs` — language detection logic (delegates to `data::lsp::registry`)
  - `installer.rs` — installation logic (evolved from current `lsp/install.rs`)
  - `health.rs` — process health monitoring, timeout detection
- Implement the state machine with explicit transition validation — invalid transitions should be compile-time or runtime errors, never silently ignored
- Implement background startup: `start_for_context()` spawns a thread that performs detection → install check → process spawn → initialize handshake → state update
- Implement the public query API: `document_symbols()`, `symbol_at_position()`, `symbol_range()`, etc.
- All JSON-RPC serialization should use `serde_json` — do not hand-format JSON strings as the current implementation does
- Add `lsp-types` crate to `Cargo.toml` for LSP protocol type definitions (version field values, capability structs, etc.)
- Deprecate and remove `src/commands/lsp/client.rs` and `src/commands/lsp/install.rs` once the engine is complete

### API Design Details
- `document_symbols()` must return a tree of `DocumentSymbol` (functions contain parameters, structs contain fields, etc.) — not a flat list
- `symbol_at_position()` must walk the symbol tree to find the innermost symbol containing the given position
- `symbol_range()` must support querying by name with disambiguation (e.g., if two functions share a name in different scopes)
- All methods should accept `&Path` for file identification and handle `file://` URI conversion internally
- The engine must handle `textDocument/didOpen` and `textDocument/didChange` notifications to keep the server's view in sync with ane's buffers

### Concurrency
- Use `std::sync::mpsc` for the event channel (single producer from background thread, single consumer in main thread)
- Use `Arc<Mutex<ServerState>>` for state that the main thread polls while the background thread updates
- The background thread owns the process handle and transport during startup; once `Running`, ownership transfers back to the engine for synchronous request/response
- Consider using `std::thread::JoinHandle` to detect if the background startup thread panicked

### Rust / rust-analyzer Specifics (Initial Implementation)
- rust-analyzer initialization options: configure `checkOnSave: false` (ane is an editor, not a build system)
- rust-analyzer requires `rootUri` pointing to the Cargo.toml's parent directory
- rust-analyzer may take several seconds to index on first launch — the engine must handle this gracefully with `Starting` state
- Install via `rustup component add rust-analyzer` — verify with `rust-analyzer --version`

## Edge Case Considerations:
- rust-analyzer crashes mid-session: detect via broken pipe or process exit, transition to `Failed`, emit event, allow retry
- Multiple Rust workspaces (workspace with multiple Cargo.toml): detect root workspace, start one server
- No language detected: engine starts with zero servers, reports `Undetected` for all queries — not an error
- Install command requires network but machine is offline: install fails, state → `Failed` with descriptive error
- Binary exists but is wrong version or corrupt: `Starting` → `Failed` when handshake times out or returns unexpected response
- ane exits while LSP is mid-startup: `Drop` implementation kills the process and waits
- File opened that doesn't match any registered language: no LSP started for that file, non-LSP chords work normally
- Very large project (thousands of files): document_symbols per-file, not whole project — should remain fast
- Concurrent chord requests: engine is `&mut self`, so requests are serialized — no race conditions

## Test Considerations:
- Unit tests for state machine transitions: verify all valid transitions succeed and invalid transitions error
- Unit tests for JSON-RPC framing: round-trip encode/decode of Content-Length headers and message bodies
- Unit tests for language detection: file extensions, project markers, ambiguous cases
- Integration test with a mock LSP server (a simple process that speaks JSON-RPC): verify initialize handshake, document symbol request/response, shutdown
- Test install detection with a binary that does/doesn't exist
- Test timeout behavior: mock server that never responds to initialize
- Test event emission: verify StateChanged events fire on transitions
- Test graceful shutdown: verify shutdown request is sent before killing the process
- All tests must work without rust-analyzer installed (use mocks for the server process)

## Codebase Integration:
- Follow established conventions, best practices, testing, and architecture patterns from the project's aspec
- Layer 0 types must not import from Layer 1
- LSPEngine lives in Layer 1 and imports only from Layer 0
- Frontend (Layer 2) consumes LSPEngine via method calls and event polling
- Use `anyhow::Result` for all public methods
- Use `serde` and `serde_json` for all JSON serialization (add to Cargo.toml if not present)
- Consider adding `lsp-types` crate for protocol-level type definitions
- Remove the existing `src/commands/lsp/` module once LSPEngine fully replaces it
