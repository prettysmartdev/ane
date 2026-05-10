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

### Exec flow (in `main.rs` and `src/frontend/cli_frontend.rs`)

Replace the ad-hoc exec branch in `main.rs` with a call to `CliFrontend::run_exec(path, chord_str) -> Result<()>`. This function owns the full pipeline:

1. **Buffer load** — `Buffer::from_file(&path)`, capture `original = buffer.content()` before any changes.
2. **Chord parse** — `ChordEngine::parse(&chord_str)` → `ParsedChord`. Return `ChordParseError` to stderr on failure. The chord string uses explicit inline parameters since there is no cursor: `cifb(some_fn, "new body")`. After parsing, if any parameter value is the sentinel `-`, read the full contents of stdin now (before LSP startup or any I/O) and substitute it as that parameter's value. Fail immediately if stdin is a TTY when `-` is used.
3. **LSP lifecycle** — if `parsed_chord.spec.requires_lsp()`:
   a. `LSPEngine::start_for_context(&path)` — detects language, starts server.
   b. `lsp.await_ready(timeout)` — block until `ServerState::Running` or error.
   c. On `ServerState::Missing`: print install hint to stderr, exit 1.
   d. On timeout: print timeout message to stderr, exit 1.
4. **Execute** — `ChordEngine::execute(&mut buffer, &parsed_chord, &mut lsp_engine, &dyn ChordFrontend)`, passing a `CliFrontend` instance as the trait object.
5. **Write** — `buffer.write()` on success; do not write on error.
6. **Diff** — `diff::unified_diff(&path, &original, &buffer.content())` → print to stdout. If empty (no changes), print nothing and exit 0.
7. **Shutdown** — `lsp_engine.shutdown_all()` regardless of success or failure (run in `Drop` or explicit call).

### Frontend trait contract (`src/frontend/traits.rs`)

`ChordEngine` calls back into the frontend for anything action-specific. The existing traits (`ChangeFrontend`, `DeleteFrontend`, etc.) should be extended so each method receives the `ParsedChord` and a mutable `Buffer` reference and returns `Result<()>` (mutations go directly to `Buffer`; the engine handles diff generation after):

```
fn execute_change(&mut self, buffer: &mut Buffer, chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<()>
fn execute_delete(&mut self, buffer: &mut Buffer, chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<()>
fn execute_insert(&mut self, buffer: &mut Buffer, chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<()>
fn execute_read  (&self,     buffer: &Buffer,     chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<String>
fn execute_move  (&mut self, buffer: &mut Buffer, chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<()>
fn execute_select(&mut self, buffer: &mut Buffer, chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<()>
fn execute_yank  (&self,     buffer: &Buffer,     chord: &ParsedChord, symbols: &[ResolvedSymbol]) -> Result<String>
```

`ResolvedSymbol` is the output of the Resolver stage (from 0002), carrying the byte/line range for the matched scope. For Line scope, the resolver produces a trivial symbol derived from `chord.target` (the line number or pattern). `ChordFrontend` dispatches to the correct method based on `chord.spec.action`.

### CliFrontend implementation (`src/frontend/cli_frontend.rs`)

All parameters (target symbol name, replacement text) are sourced from the chord's inline argument list — e.g., `cifb(some_fn, "new body")` where `some_fn` is the target and `"new body"` is the text. If a parameter is the sentinel `-`, it was already resolved to stdin content during the parse step; by the time `CliFrontend` methods are called, `chord.text` and `chord.target` are fully populated strings. No interactive prompting is allowed in CLI mode. Specific behavior per action:

- **Change**: replace the range covered by `symbol` with `chord.text`. Error if `chord.text` is empty.
- **Delete**: remove the range covered by `symbol`. `chord.text` unused.
- **Insert** (positional matters): insert `chord.text` before/after/inside the symbol range based on `chord.spec.positional`.
- **Read**: return `buffer` content for the symbol range as a `String` (printed to stdout by caller, not as diff).
- **Yank**: same as Read (for clipboard in TUI; for CLI, returns content).
- **Move** / **Select**: in CLI mode, these have no meaningful headless implementation — return `Err(anyhow!("move/select not supported in exec mode"))`.

### Error handling and exit codes

All errors print a human-readable message to **stderr** and exit with code **1**. Stdout must remain clean for diff consumers.

| Failure | Stderr message pattern |
|---|---|
| File not found | `error: file not found: <path>` |
| Invalid chord string | `error: invalid chord '<chord>': <reason>` |
| LSP server missing | `error: no LSP server for <language>. Run: ane lsp install <language>` |
| LSP start timeout | `error: LSP server did not start within <N>s` |
| No symbol match | `error: no <Scope> found at <target> in <file>` |
| Ambiguous match | `error: ambiguous match — did you mean:\n  <list of candidates>` |
| Empty chord.text for Change/Insert | `error: chord requires replacement text (e.g. cifb(some_fn, "new body"))` |
| `-` used but stdin is a TTY | `error: chord parameter '-' requires piped input on stdin` |
| Stdin read error when `-` used | `error: failed to read stdin: <io error>` |

Use `eprintln!` for all error output; never mix errors into stdout.

### `main.rs` changes

Remove inline exec logic. The exec branch becomes:

```rust
Some(Command::Exec { path, chord }) => {
    if let Err(e) = CliFrontend::run_exec(path, chord) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
```

### Module additions

- `src/frontend/cli_frontend.rs` — add `run_exec` associated function, expand trait implementations to cover all scopes using `ResolvedSymbol`.
- No new modules required; the LSPEngine and ChordEngine modules are owned by 0001/0002.


## Edge Case Considerations:

- **No diff / idempotent chord**: the exec command should exit 0 and print nothing to stdout. Do not print an empty diff header.
- **Read/Yank in exec mode**: these produce output (the symbol content), not a diff. Print the content to stdout as-is. Do not run `buffer.write()`.
- **Move/Select in exec mode**: return a clear error rather than silently doing nothing.
- **LSP not needed but server present**: skip the LSP lifecycle entirely; do not start a server unnecessarily.
- **File with no trailing newline**: `Buffer::from_file` and `buffer.write()` must preserve the original trailing-newline behavior to avoid spurious diff noise.
- **Binary files**: `Buffer::from_file` should error early if the file is not valid UTF-8, with a message like `error: file is not valid UTF-8: <path>`.
- **Chord target out of range**: if `chord.target` specifies line N and the file has fewer than N lines, error with `error: line <N> out of range (file has <M> lines)`.
- **Multiple symbols match**: when the Resolver returns more than one candidate (e.g., two functions with similar names), print the ambiguous-match error with the full list of candidates so the user can narrow the target.
- **Write failure**: if `buffer.write()` fails (permissions, disk full), report to stderr and exit 1. The original file is unchanged because `write()` uses atomic replace or in-place overwrite — if it fails mid-write, document the risk in a comment.
- **Ctrl-C / SIGINT during LSP startup**: the `LSPEngine::shutdown_all()` in the `Drop` path should handle cleanup so no zombie LSP processes are left running.
- **Chord with LSP scope on non-code file** (e.g., `.md`): LSPEngine will return `ServerState::Missing` for unknown languages — surface the same "no LSP server" error.
- **`-` sentinel with no piped input (stdin is a TTY)**: detect with `std::io::stdin().is_terminal()` (from `std::io::IsTerminal`, stable since 1.70) and fail immediately with a clear error before any buffer I/O.
- **`-` used for both parameters**: only one stdin read ever happens; the same content is used for both. Document this constraint with a comment.
- **Stdin contains trailing newline**: strip exactly one trailing newline from the stdin read so that `echo "foo" | ane exec ...` and a file containing `foo` produce identical parameter values.
- **Very large stdin input**: no size limit is imposed; the full content is buffered into `String`. This is acceptable for code edits; document the assumption.
- **Chord syntax without parens (old form)**: the parser must reject bare `cifb` with no argument list and return a helpful error explaining the CLI requires explicit parameters: `error: exec mode requires explicit parameters, e.g. cifb(fn_name, "body")`.


## Test Considerations:

- **Unit: CliFrontend trait methods** — test each action (change, delete, insert, read) against a pre-populated `Buffer` and a synthetic `ResolvedSymbol` for both Line and Function scopes. Assert buffer mutations and return values.
- **Unit: run_exec argument parsing** — test that malformed chord strings produce errors before any LSP or buffer I/O.
- **Unit: error message format** — assert the exact stderr strings for each failure mode so they don't silently regress.
- **Integration: Line-scope exec without LSP** — run `run_exec` against a temp file with a Line-scope chord; assert the returned diff is correct and the file on disk is updated.
- **Integration: LSP-scope exec** — use a mock `LSPEngine` (trait object or test double) that returns synthetic `ResolvedSymbol` values; assert the correct buffer range is modified.
- **Integration: no-change idempotent exec** — run a chord whose application produces no diff; assert stdout is empty and exit code is 0.
- **Integration: binary file rejection** — pass a binary file path; assert exit code 1 and appropriate stderr.
- **Integration: out-of-range target** — pass a line number beyond EOF; assert exit code 1 and line-range error.
- **Integration: Read action** — assert content is written to stdout, not as a diff, and file is not modified.
- **End-to-end smoke test** — run the built binary via `std::process::Command` against a fixture `.rs` file with a known chord; assert stdout matches a golden diff and the fixture is updated correctly.
- **Integration: stdin sentinel** — run `run_exec` with a chord containing `-` as the text parameter and a pre-set stdin pipe; assert the piped content is used as the replacement text.
- **Unit: stdin-is-TTY rejection** — assert that passing `-` as a parameter when stdin is not piped produces the TTY error, without touching the file.
- **Unit: trailing-newline strip** — assert that stdin content `"foo\n"` and `"foo"` both produce parameter value `"foo"`.


## Codebase Integration:

- Follow the strict 3-layer rule: `run_exec` lives in Layer 2 (`src/frontend/`) and may call Layer 1 engines and Layer 0 types, but Layer 1 must never call back into Layer 2. The `ChordFrontend` trait in `traits.rs` is Layer 2 — ChordEngine (Layer 1) must accept it as a `&dyn ChordFrontend` parameter, not import the concrete types.
- All error handling uses `anyhow::Result` / `anyhow::bail!` / `anyhow::anyhow!`. Avoid `unwrap` outside tests.
- Diff output uses the existing `diff::unified_diff` helper in `src/commands/diff.rs` — do not introduce a new diff dependency.
- Tests use `#[cfg(test)] mod tests` blocks inside each source file, using `tempfile` for temp directories (already in Cargo.toml or add it as a dev-dependency).
- The `CliFrontend` struct must remain the single implementor of all frontend traits; do not split into per-action structs.
- `buffer.write()` must only be called after a successful `ChordEngine::execute`; wrap the entire execute-then-write sequence in a single `?`-propagating block so partial writes cannot happen on error.
- `LSPEngine::shutdown_all()` should be called in a `defer`-style pattern (implement `Drop` on a guard or call it explicitly before every return path) to prevent leaked server processes during error exits.
