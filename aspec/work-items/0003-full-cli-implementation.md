# Work Item: Feature

Title: full CLI implementation
Issue: issuelink

## Summary:
- Wire together the full `ane exec <file> <chord>` flow: buffer loading → LSPEngine lifecycle → ChordEngine execution → buffer write → git-compatible diff to stdout, with helpful structured errors to stderr. CLI-specific interaction (how the frontend supplies text, confirms actions, reports results) is encoded in traits that ChordEngine calls through, implemented by CliFrontend. This work item assumes 0001 (LSPEngine) and 0002 (ChordEngine) are complete.

## User Stories

### User Story 1:
As a: code agent or script

I want to: run `ane exec ./src/lib.rs cifb(some_fn, "return Ok(());")` and receive a unified diff on stdout, or pipe replacement text via `echo "return Ok(());" | ane exec ./src/lib.rs cifb(some_fn, -)`

So I can: apply precise, structure-aware code edits from CI pipelines and automation without a human in the loop

### User Story 2:
As a: developer

I want to: receive a clear, actionable error message on stderr (with a non-zero exit code) when a chord fails — whether from a bad chord string, a missing LSP server, no symbol match, or an ambiguous match

So I can: debug failing scripts quickly and handle errors programmatically by exit code

### User Story 3:
As a: developer

I want to: LSP-requiring scopes (Function, Variable, Struct, Impl, Enum) to be resolved automatically when I run `ane exec` against a file, with the LSP server started, awaited, and shut down without any manual setup

So I can: use the full chord grammar from the command line the same way as in the TUI


## Implementation Details:

### Exec flow (in `src/commands/chord.rs` → `execute_chord`)

The `execute_chord(path, chord)` function owns the full pipeline. It is called from `main.rs`'s exec branch after parsing the chord string. The function handles buffer loading, LSP lifecycle, chord execution, and file write — the caller (`main.rs`) only handles diff output and error display.

1. **Buffer load** — `Buffer::from_file(&path)`, capture `original = buffer.content()` before any changes. Canonicalize the path via `std::fs::canonicalize(path)` so the LSP engine receives absolute paths for workspace root detection and URI generation. Use the canonicalized path as the buffer HashMap key.
2. **Chord parse** — already done by the caller via `ChordEngine::parse(&chord_str)` → `ChordQuery`. The parser sets `chord.requires_lsp` based on the scope (`Scope::requires_lsp()`). Return `ChordParseError` to stderr on failure. The chord string uses explicit inline parameters since there is no cursor: `cifb(some_fn, "new body")`. After parsing, if any parameter value is the sentinel `-`, read the full contents of stdin now (before LSP startup or any I/O) and substitute it as that parameter's value. Fail immediately if stdin is a TTY when `-` is used.
3. **LSP lifecycle** — if `chord.requires_lsp`:
   a. Determine the root path from the canonicalized file's parent directory.
   b. Create the `LspEngine` with `LspEngineConfig::default()` which has `auto_install: true` (see below). This means the startup thread will automatically install the language server if it is missing (e.g., `rustup component add rust-analyzer` for Rust).
   c. `lsp.start_for_context(root, &[&abs_path])` — detects language from file extension and directory markers, spawns the LSP server on a background thread. The startup thread handles the full sequence: detect → auto-install if missing → start → initialize handshake → Running.
   d. Detect the language via `registry::detect_language_from_path(&abs_path)` — bail with a clear error if no language is detected (e.g., file is not `.rs`).
   e. `lsp.await_ready(lang, Duration::from_secs(30))` — block until the server reaches a terminal state (`Running`, `Failed`, or `Stopped`). The 30s timeout covers the entire startup pipeline including auto-installation.
   f. Check the returned `ServerState`:
      - `Running` → proceed to execute.
      - `Failed` → bail with `"LSP server for {lang} failed to start"`. This covers install failures, spawn failures, and handshake failures — the engine emits `LspEvent::Error` with a specific reason.
      - Any other state (timeout) → bail with `"LSP server for {lang} did not become ready within 30s"`.
   g. The chord engine receives the now-ready `&mut LspEngine` and calls `lsp.document_symbols()` etc. directly — no backoff, no retry, no readiness checks inside the chord engine.
4. **Execute** — `ChordEngine::resolve(&chord, &buffers, &mut lsp)` then `ChordEngine::patch(&resolved, &buffers)`. The chord engine is stateless: it receives a ready LSP engine and calls through to it. If LSP is not required, the engine is created but never started — LSP-unrelated scopes (Line, Buffer) never touch it.
5. **Write** — `std::fs::write(&abs_path, &modified)` on success when content differs; do not write on error or when unchanged.
6. **Diff** — `diff::unified_diff(&path, &original, &modified)` → printed to stdout by `main.rs`. If empty (no changes), print nothing and exit 0.
7. **Shutdown** — `LspEngine` implements `Drop` which calls `shutdown_all()`, so cleanup happens automatically when `lsp` goes out of scope regardless of success or failure.

### Frontend trait contract (`src/frontend/traits.rs`)

The existing `ApplyChordAction` trait in `traits.rs` is the frontend abstraction. Both `CliFrontend` and `TuiFrontend` implement it. The trait receives a `ChordAction` (output of the chord engine's patch phase) and applies it to `EditorState`:

```
fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<String>
```

`ChordAction` contains the diff, yanked content, cursor destination, and mode transition. The CLI frontend applies the diff to the buffer and returns the modified content (or yanked text). The TUI frontend applies cursor/mode changes for interactive editing.

The chord engine itself (`ChordEngine`) is a stateless three-phase pipeline: **parse → resolve → patch**. It takes `&mut LspEngine` as a parameter for the resolve phase but does no LSP lifecycle management. The engine uses `lsp.document_symbols(path)` to resolve LSP-scoped symbols and expects the call to succeed — all startup/readiness logic lives in the caller (`execute_chord`).

### CliFrontend implementation (`src/frontend/cli_frontend.rs`)

`CliFrontend` implements `ApplyChordAction`. It applies the chord engine's output (`ChordAction`) to the buffer without any interactive prompting. All parameters (target symbol name, replacement text) are sourced from the chord's inline argument list — e.g., `cifb(some_fn, "new body")` where `some_fn` is the target and `"new body"` is the text via `ChordArgs`. If a parameter value is the sentinel `-`, it was already resolved to stdin content during the parse step.

The `CliFrontend::apply` method:
- If the action contains a diff: update buffer lines to the modified content, return the modified text.
- If the action contains yanked content (Yank action): return it (printed to stdout by `main.rs`, not as a diff).
- Otherwise: return empty (no changes).

No Move/Select actions exist in the chord grammar; the existing actions (Change, Delete, Append, Prepend, Insert, Replace, Yank) all work headlessly via the chord argument system.

### Error handling and exit codes

All errors print a human-readable message to **stderr** and exit with code **1**. Stdout must remain clean for diff consumers.

| Failure | Stderr message pattern |
|---|---|
| File not found | `error: file not found: <path>` |
| Invalid chord string | `error: invalid chord '<chord>': <reason>` |
| No language detected | `error: no language server available for <path>` |
| LSP install failed | `error: LSP server for <language> failed to start` (engine auto-installs; install failure surfaces as `Failed` state with `LspEvent::Error` detail) |
| LSP start failed | `error: LSP server for <language> failed to start` |
| LSP start timeout | `error: LSP server for <language> did not become ready within 30s` |
| No symbol match | `error: no <Scope> found at <target> in <file>` (from `ChordError::resolve`) |
| Ambiguous match | `error: ambiguous match — <list of candidates>` (from `ChordError::resolve_with_symbols`) |
| Empty chord.text for Change/Insert | `error: chord requires replacement text (e.g. cifb(some_fn, "new body"))` |
| `-` used but stdin is a TTY | `error: chord parameter '-' requires piped input on stdin` |
| Stdin read error when `-` used | `error: failed to read stdin: <io error>` |

Use `eprintln!` for all error output; never mix errors into stdout.

### `main.rs` changes

The exec branch in `main.rs` stays minimal — it parses the chord, calls `chord::execute_chord`, and handles output/errors. The LSP lifecycle is internal to `execute_chord`:

```rust
Some(Command::Exec { chord: chord_str, path }) => {
    let parsed = chord::parse_chord(&chord_str)?;
    let result = chord::execute_chord(&path, &parsed)?;
    // ... output diff or yanked content, same as current
}
```

### `LspEngineConfig::default()` change

Change `auto_install` from `false` to `true` in `LspEngineConfig::default()`. The LSP engine already has full auto-install support in the startup thread: when `auto_install` is true and the server binary is not found, it runs the `install_command` from the server registry (e.g., `rustup component add rust-analyzer`), transitions through `Installing → Available`, and then proceeds to start the server. With this default, calling `start_for_context` followed by `await_ready` handles the entire lifecycle — detect, install, start, handshake — without the caller needing to handle `Missing` state at all.

The TUI should also use `auto_install: true` (same default), but may choose to show the install progress in the status bar via `LspEvent::StateChanged` events.

### Module changes

- `src/commands/chord.rs` — update `execute_chord` to add LSP lifecycle (start, await, error handling) before calling `ChordEngine::resolve`.
- `src/commands/lsp_engine/engine.rs` — change `LspEngineConfig::default()` to set `auto_install: true`.
- No new modules required; the LSPEngine and ChordEngine modules are owned by 0001/0002.


## Edge Case Considerations:

- **No diff / idempotent chord**: the exec command should exit 0 and print nothing to stdout. Do not print an empty diff header.
- **Read/Yank in exec mode**: these produce output (the symbol content), not a diff. Print the content to stdout as-is. Do not run `buffer.write()`.
- **Move/Select in exec mode**: return a clear error rather than silently doing nothing.
- **LSP not needed but server present**: skip the LSP lifecycle entirely; do not start a server unnecessarily. The check is `chord.requires_lsp` (set by the parser based on scope). Non-LSP scopes (Line, Buffer) never trigger `start_for_context`.
- **File with no trailing newline**: `Buffer::from_file` and `buffer.write()` must preserve the original trailing-newline behavior to avoid spurious diff noise.
- **Binary files**: `Buffer::from_file` should error early if the file is not valid UTF-8, with a message like `error: file is not valid UTF-8: <path>`.
- **Chord target out of range**: if `chord.target` specifies line N and the file has fewer than N lines, error with `error: line <N> out of range (file has <M> lines)`.
- **Multiple symbols match**: when the Resolver returns more than one candidate (e.g., two functions with similar names), print the ambiguous-match error with the full list of candidates so the user can narrow the target.
- **Write failure**: if `buffer.write()` fails (permissions, disk full), report to stderr and exit 1. The original file is unchanged because `write()` uses atomic replace or in-place overwrite — if it fails mid-write, document the risk in a comment.
- **Ctrl-C / SIGINT during LSP startup**: `LspEngine` implements `Drop` which calls `shutdown_all()`, killing child processes and reaping zombies. No additional signal handling is needed.
- **Chord with LSP scope on non-code file** (e.g., `.md`): `registry::detect_language_from_path` returns `None` for unknown extensions — `execute_chord` bails with `"no language server available for <path>"` before starting the engine.
- **`-` sentinel with no piped input (stdin is a TTY)**: detect with `std::io::stdin().is_terminal()` (from `std::io::IsTerminal`, stable since 1.70) and fail immediately with a clear error before any buffer I/O.
- **`-` used for both parameters**: only one stdin read ever happens; the same content is used for both. Document this constraint with a comment.
- **Stdin contains trailing newline**: strip exactly one trailing newline from the stdin read so that `echo "foo" | ane exec ...` and a file containing `foo` produce identical parameter values.
- **Very large stdin input**: no size limit is imposed; the full content is buffered into `String`. This is acceptable for code edits; document the assumption.
- **Chord syntax without parens (old form)**: the parser must reject bare `cifb` with no argument list and return a helpful error explaining the CLI requires explicit parameters: `error: exec mode requires explicit parameters, e.g. cifb(fn_name, "body")`.
- **Auto-install failure**: if the install command fails (e.g., no network, `rustup` not found), the startup thread transitions to `Failed` and emits `LspEvent::Error` with the install error message. `await_ready` returns `Failed` promptly. The exec flow surfaces this as `"LSP server for {lang} failed to start"`. The `LspEvent::Error` detail can be logged to stderr for debugging.
- **Path canonicalization**: `execute_chord` must canonicalize the file path before using it as a buffer HashMap key and passing it to LSP APIs. The LSP engine needs absolute paths for proper `file://` URI generation and workspace root detection (`workspace_root_for_dir` walks parent directories). Relative paths would cause mismatches between the buffer key and the path used in LSP requests.


## Test Considerations:

- **Unit: CliFrontend::apply** — test that `apply` correctly updates buffer lines from a `ChordAction` diff, and that yanked content is returned without modifying the buffer.
- **Unit: chord parse sets requires_lsp** — assert that LSP scopes (Function, Variable, Struct, Member) set `requires_lsp = true` and Line/Buffer set it to `false`. (Already covered by existing tests in `chord.rs` and `parser.rs`.)
- **Unit: error message format** — assert the exact stderr strings for each failure mode so they don't silently regress.
- **Unit: auto_install true by default** — assert `LspEngineConfig::default().auto_install == true`.
- **Integration: Line-scope exec without LSP** — run `execute_chord` against a temp file with a Line-scope chord; assert the returned diff is correct and the file on disk is updated. Verify that no LSP server is started (LSP engine has no registered servers after the call).
- **Integration: LSP-scope exec with mock server** — use `LspEngineConfig::with_server_override` pointing to the `mock_lsp_server` binary; run an LSP-scoped chord (e.g., `cefs(target:some_fn, value:"new body")`); assert the correct buffer range is modified and the file is updated. This validates the full flow: parse → LSP start → await ready → resolve → patch → write.
- **Integration: LSP-scope exec with install failure** — use a server override with a non-existent binary and a failing check command; run an LSP-scoped chord with `auto_install: true`; assert `execute_chord` returns an error containing "failed to start" and returns promptly (no 30s wait).
- **Integration: no-change idempotent exec** — run a chord whose application produces no diff; assert stdout is empty and exit code is 0.
- **Integration: binary file rejection** — pass a binary file path; assert exit code 1 and appropriate stderr.
- **Integration: out-of-range target** — pass a line number beyond EOF; assert exit code 1 and line-range error.
- **Integration: Yank action** — assert content is written to stdout, not as a diff, and file is not modified.
- **End-to-end smoke test** — run the built binary via `std::process::Command` against a fixture `.rs` file with a known chord; assert stdout matches a golden diff and the fixture is updated correctly.
- **Integration: stdin sentinel** — run `execute_chord` with a chord containing `-` as the text parameter and a pre-set stdin pipe; assert the piped content is used as the replacement text.
- **Unit: stdin-is-TTY rejection** — assert that passing `-` as a parameter when stdin is not piped produces the TTY error, without touching the file.
- **Unit: trailing-newline strip** — assert that stdin content `"foo\n"` and `"foo"` both produce parameter value `"foo"`.


## Codebase Integration:

- Follow the strict 3-layer rule: `execute_chord` lives in Layer 1 (`src/commands/chord.rs`) and may call other Layer 1 engines (ChordEngine, LspEngine) and Layer 0 types. The frontend trait (`ApplyChordAction`) lives in Layer 2. The chord engine (Layer 1) does NOT import or call back into Layer 2 — it returns `ChordAction` values that the frontend applies.
- The chord engine (`ChordEngine`) remains stateless and LSP-lifecycle-unaware. It receives `&mut LspEngine` as a parameter and calls query methods (e.g., `document_symbols`) directly. All LSP startup, readiness waiting, and error handling lives in the caller (`execute_chord`). The chord engine must never contain backoff/retry/polling logic.
- All error handling uses `anyhow::Result` / `anyhow::bail!` / `anyhow::anyhow!`. Avoid `unwrap` outside tests.
- Diff output uses the existing `diff::unified_diff` helper in `src/commands/diff.rs` — do not introduce a new diff dependency.
- Tests use `#[cfg(test)] mod tests` blocks inside each source file, using `tempfile` for temp directories (already in Cargo.toml).
- The `CliFrontend` struct must remain the single implementor of the `ApplyChordAction` trait; do not split into per-action structs.
- File write (`std::fs::write`) must only happen after a successful `ChordEngine::resolve` + `ChordEngine::patch` sequence. Use `?` propagation so errors bail before any disk mutation.
- `LspEngine` implements `Drop` which calls `shutdown_all()` — no explicit cleanup is needed in error paths. The engine variable going out of scope handles it.
