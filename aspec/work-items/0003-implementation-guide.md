# Work Item 0003: Implementation Guide
## Full CLI Implementation — `ane exec` end-to-end pipeline

**Last Updated:** 2026-05-10  
**Status:** Implementation Guide  
**Prerequisites:** Work Item 0001 (LSPEngine) and 0002 (ChordEngine) completed  
**Target:** Rust / All supported languages

---

## Overview

This guide provides step-by-step implementation instructions for **Work Item 0003**: completing the `ane exec` command and its supporting infrastructure. The exec command is the headless interface that enables code agents to apply chords programmatically.

The implementation wires together the full pipeline:

1. **Parse** — Convert a chord string to a `ChordQuery`
2. **Load** — Read the target file into a `Buffer`
3. **LSP Lifecycle** — Start, wait for, and manage the LSP engine (if needed)
4. **Resolve** — Use the LSP engine to resolve symbols (if needed)
5. **Patch** — Apply the chord to the buffer
6. **Write** — Persist changes to disk
7. **Diff** — Output a unified diff to stdout

### Key Goals

1. **Headless-Ready**: The exec command requires no interactive prompting; all parameters come from the chord string or stdin
2. **Declarative LSP**: The caller specifies a file and chord; LSP startup, language detection, and server installation happen automatically
3. **Structured Errors**: Clear, actionable error messages on stderr; non-zero exit codes for failures
4. **Agent-Friendly**: Output is machine-readable (unified diff or yanked content); no status messages pollute stdout
5. **Backward Compatible**: The TUI and existing CLI functionality remain unchanged

---

## Architecture Rules (From CLAUDE.md)

The ane project enforces a **three-layer architecture with strict dependency direction**:

- **Layer 0** (`src/data/`): All filesystem I/O, buffer state, chord type definitions, LSP data (registry, schemas). No imports from `commands` or `frontend`.
- **Layer 1** (`src/commands/`): Chord logic, LSP operations, LSP engine lifecycle. May import from `data`. No imports from `frontend`.
- **Layer 2** (`src/frontend/`): CLI + TUI + frontend action traits. May import from `data` and `commands`.

**Key Components**:
- `execute_chord(path, chord)` — Layer 1, orchestrates the full pipeline
- `CliFrontend::apply(state, action)` — Layer 2, applies actions to the buffer
- `ChordEngine` — Layer 1, stateless 3-stage pipeline (parse → resolve → patch)
- `LspEngine` — Layer 1, centralized LSP lifecycle manager

**Critical Rule**: Lower layers NEVER depend on higher layers. Violating this is a build-breaking error.

---

## Implementation Phases

### Phase 1: Infrastructure and Frontend Traits

Establish the command-layer functions and traits that will be used by the main entry point.

#### Phase 1.1: Implement `CliFrontend` Struct and Trait

**File**: `src/frontend/cli_frontend.rs`

**Goal**: Create a stateless CLI frontend that implements the `ApplyChordAction` trait (existing in `src/frontend/traits.rs`).

**Steps**:

1. Define the `CliFrontend` struct:
   ```rust
   pub struct CliFrontend;
   ```

2. Implement `ApplyChordAction` for `CliFrontend`:
   ```rust
   impl ApplyChordAction for CliFrontend {
       fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> anyhow::Result<String> {
           // Implementation follows below
       }
   }
   ```

3. Implement the `apply` method:
   - If the action is a `Diff` variant: apply the diff to the buffer's lines and return the modified content as a `String`
   - If the action is a `Yank` variant: return the yanked content as-is without modifying the buffer
   - For all other action types: return an empty string (no modification)
   - Use `anyhow::bail!` to return errors; never use `unwrap`

4. Helper method: `apply_diff(lines, diff)`:
   - Iterate through the diff hunks
   - Remove lines marked with `-` (deletion)
   - Insert lines marked with `+` (insertion)
   - Leave unchanged lines (`~`) as-is
   - Return the joined content as `String`

**Test coverage**:
- Unit test: `apply` with a Diff action updates all buffer lines correctly
- Unit test: `apply` with a Yank action returns yanked content without modifying the buffer
- Unit test: `apply` with no diff returns empty string

**Key Invariants**:
- The frontend never modifies the buffer's line count without corresponding diff hunks
- Yank actions never trigger file writes (the caller checks this)
- The frontend returns the modified content, not the diff itself (the caller handles diff generation)

---

### Phase 2: Chord Parsing and Parameter Resolution

Implement chord parsing that handles the CLI-specific argument syntax and stdin sentinel.

#### Phase 2.1: Update `parse_chord()` in `src/commands/chord.rs`

**File**: `src/commands/chord.rs`

**Goal**: Ensure `parse_chord(&chord_str)` returns a `ChordQuery` with `requires_lsp` set correctly based on the scope.

**Steps**:

1. Verify the existing parser handles the full 4-part chord grammar (action, positional, scope, component)
2. Ensure `ChordQuery::requires_lsp()` returns `true` only for LSP-scoped chords (Function, Variable, Struct, Impl, Enum, Member)
3. Ensure `ChordQuery::requires_lsp()` returns `false` for non-LSP scopes (Line, Buffer)
4. Add validation: reject bare chords without an argument list (e.g., `cifb` without parameters):
   - Error message: `"exec mode requires explicit parameters, e.g. cifb(fn_name, \"body\")"`

**Test coverage**:
- Unit test: LSP scopes set `requires_lsp = true`
- Unit test: Non-LSP scopes set `requires_lsp = false`
- Unit test: Bare chord without parameters is rejected with helpful error

**Key Invariants**:
- The parser is called by `main.rs` before `execute_chord` is invoked
- The `requires_lsp` flag is immutable after parsing
- Parse errors are returned to `main.rs` for display on stderr

---

### Phase 3: Core Execution Pipeline

Implement the main `execute_chord()` function that orchestrates the entire flow.

#### Phase 3.1: Implement `execute_chord()` in `src/commands/chord.rs`

**File**: `src/commands/chord.rs`

**Function signature**:
```rust
pub fn execute_chord(path: &Path, chord: &ChordQuery) -> anyhow::Result<ChordResult> {
    // Full implementation
}
```

**Supporting struct** — add to `src/commands/chord.rs`:
```rust
pub struct ChordResult {
    pub modified: String,  // Modified file content or yanked text
    pub original: String,  // Original file content (for diff generation)
}
```

**Steps**:

1. **Path canonicalization**:
   - Canonicalize the file path: `let abs_path = std::fs::canonicalize(path)?`
   - Use the canonicalized path for all subsequent operations (buffer key, LSP URIs)
   - If canonicalization fails, return an error with context: `context("failed to resolve file path")`

2. **Buffer load**:
   - Call `Buffer::from_file(&abs_path)` to load the file
   - Capture the original content: `let original = buffer.content().to_string()`
   - Handle errors: if the file is binary (not UTF-8), `Buffer::from_file` will return an error; propagate it with `?`
   - If the file doesn't exist, `std::fs::canonicalize` will fail; propagate with `?`

3. **Parameter substitution** (stdin sentinel handling):
   - Check if any parameter in `chord.args` is the sentinel value `"-"`
   - If found, check `std::io::stdin().is_terminal()`:
     - If true (no piped input): `bail!("chord parameter '-' requires piped input on stdin")`
     - If false (piped input available): read the full stdin content
   - Substitute the piped content for the `"-"` sentinel
   - Strip exactly one trailing newline from the stdin content if present: `content.trim_end_matches('\n')`
   - Document: "If `-` is used for multiple parameters, all are replaced with the same stdin content"

4. **LSP lifecycle** (if `chord.requires_lsp`):
   - Create a new `LspEngine`: `let mut lsp = LspEngine::new()`
   - Call `lsp.start_for_context(&abs_path.parent().unwrap(), &[&abs_path])`
     - The engine detects language from file extension and directory markers
     - The engine auto-installs the language server if missing (via `auto_install: true` in the config default)
   - Call `lsp.await_ready(lang, Duration::from_secs(30))` to block until ready
   - Check the returned `ServerState`:
     - `Running` → proceed to resolve
     - `Failed` → `bail!("LSP server for {} failed to start", lang.name())`
     - Other states → `bail!("LSP server for {} did not become ready within 30s", lang.name())`
   - If language detection returns `None`: `bail!("no language server available for {}", path.display())`
   - The LSP engine implements `Drop` which calls `shutdown_all()` automatically, so no explicit cleanup is needed

5. **Chord execution**:
   - Call `ChordEngine::resolve(&chord, &buffers, &mut lsp)` if LSP is needed; otherwise pass a dummy/empty engine
   - Call `ChordEngine::patch(&resolved, &buffers)` to get the `ChordAction`
   - If resolution fails: propagate the error from `ChordEngine::resolve`
   - Handle no-match errors: `"no <Scope> found at <target> in <file>"`
   - Handle ambiguous-match errors: `"ambiguous match — <list of candidates>"`

6. **Frontend application**:
   - Create a `CliFrontend` instance
   - Create an `EditorState` with the current buffer
   - Call `frontend.apply(&mut state, &action)` to apply the action to the buffer
   - Capture the modified content: `let modified = frontend.apply(...)?`

7. **File write** (conditional):
   - Only write if the content has changed: `if modified != original { std::fs::write(&abs_path, &modified)?; }`
   - Do NOT write if the action is a Yank (yanked content is not file content)
   - If write fails, propagate the error; the original file remains unchanged (atomic semantics)
   - Error message: use anyhow's error context to surface any IO errors

8. **Return result**:
   ```rust
   Ok(ChordResult {
       modified,
       original,
   })
   ```

**Error handling**:
- All errors use `anyhow::Result<T>` and `anyhow::bail!`
- Errors are displayed by `main.rs` on stderr
- Never use `unwrap`; always use `?` or `bail!`
- Add context to errors for debugging: `context("while loading buffer")`

**Key Invariants**:
- The function is stateless: it receives a chord and a path, not an editor state
- LSP startup is synchronous and happens inline; no background threads are created by the caller
- File writes only happen after successful patch; errors leave the file unchanged
- The function returns the modified content and original content for diff generation

**Test coverage**:
- Unit test: Path canonicalization (absolute vs relative paths)
- Unit test: Buffer load failure (missing file)
- Unit test: Stdin sentinel handling (TTY check, piped content)
- Unit test: Non-UTF-8 file rejection
- Integration test: Full line-scope exec without LSP
- Integration test: Full LSP-scope exec with mock server
- Integration test: LSP startup failure (nonexistent binary)
- Integration test: Idempotent (no-change) exec
- Integration test: Out-of-range line number
- Integration test: Yank action (no file write)
- Integration test: End-to-end smoke test (diff output and file update)
- See `tests/work_item_0003.rs` for comprehensive test cases

---

#### Phase 3.2: Add `execute_chord_with_config()` for Testing

**File**: `src/commands/chord.rs`

**Function signature**:
```rust
pub fn execute_chord_with_config(
    path: &Path,
    chord: &ChordQuery,
    config: LspEngineConfig,
) -> anyhow::Result<ChordResult> {
    // Like execute_chord but uses the provided config instead of default
}
```

**Purpose**: Allow tests to override LSP configuration (e.g., use mock servers, set custom timeouts).

**Implementation**:
- Same as `execute_chord` but uses the provided `LspEngineConfig` instead of `LspEngineConfig::default()`
- Call this from `execute_chord`: `execute_chord_with_config(path, chord, LspEngineConfig::default())`

---

### Phase 4: Main Entry Point and Output Handling

Wire the exec command into the CLI's main entry point.

#### Phase 4.1: Update `main.rs` for Exec Command

**File**: `src/main.rs`

**Steps**:

1. Add a clap subcommand for `exec`:
   ```rust
   #[derive(Subcommand)]
   enum Command {
       #[command(about = "Execute a chord in headless mode")]
       Exec {
           #[arg(short, long)]
           chord: String,
           
           path: String,
       },
       // ... other commands
   }
   ```

2. Handle the `Exec` command in `main()`:
   ```rust
   Some(Command::Exec { chord: chord_str, path }) => {
       let parsed = chord::parse_chord(&chord_str)?;
       let result = chord::execute_chord(Path::new(&path), &parsed)?;
       
       // Output handling (see below)
       if result.modified != result.original {
           let diff = diff::unified_diff(&path, &result.original, &result.modified);
           println!("{}", diff);
       }
   }
   ```

3. **Output semantics**:
   - If the action is a Yank: print the yanked content to stdout (already returned as `modified`)
   - If the action is a Diff and content changed: generate and print a unified diff
   - If the action is a Diff and no changes: print nothing to stdout
   - All errors are printed to stderr by `main.rs` (via `Result` propagation and anyhow's Display impl)
   - Exit code 0 on success, 1 on any error

4. **Error handling in main**:
   - Use `?` to propagate errors from `execute_chord`
   - Let anyhow's error handling chain print errors to stderr
   - Exit with code 1 automatically (Rust's default for `main` returning `Err`)

**Key Invariants**:
- stdout is clean and machine-readable (diff or yanked content only)
- stderr contains all error messages and human-readable status
- Exit codes: 0 for success (including no-change idempotent), 1 for any failure

**Test coverage**:
- CLI test: `ane exec --chord "cels(line:0, value:replaced)" file.rs` produces diff and exits 0
- CLI test: `ane exec` with missing file exits 1 with error on stderr
- CLI test: `ane exec` with binary file exits 1 with error on stderr
- CLI test: `ane exec` with out-of-range line exits 1 with line-range error
- CLI test: `ane exec` with no changes exits 0 with empty stdout
- CLI test: Yank action prints content to stdout and exits 0

---

### Phase 5: LSP Configuration and Auto-Installation

Update the LSP engine's default configuration to enable auto-installation.

#### Phase 5.1: Update `LspEngineConfig::default()` in `src/commands/lsp_engine/engine.rs`

**File**: `src/commands/lsp_engine/engine.rs`

**Steps**:

1. Locate the `LspEngineConfig` struct definition
2. Change `auto_install` from `false` to `true` in `LspEngineConfig::default()`:
   ```rust
   impl Default for LspEngineConfig {
       fn default() -> Self {
           Self {
               auto_install: true,  // CHANGED from false
               // ... other fields remain unchanged
           }
       }
   }
   ```

3. Document the change in a comment:
   ```rust
   // auto_install: When true, the startup thread automatically installs
   // missing language servers (e.g., via `rustup component add rust-analyzer`).
   // This enables `ane exec` to work without manual LSP setup.
   auto_install: true,
   ```

4. Verify existing behavior:
   - The startup thread already has full auto-install support via the `install_command` in the server registry
   - With `auto_install: true`, the engine transitions through `Detecting → Installing → Available → Running`
   - If installation fails, the engine transitions to `Failed` and emits `LspEvent::Error`

5. TUI note: The TUI may also use `auto_install: true` and optionally display install progress via `LspEvent::StateChanged` events

**Key Invariants**:
- The default applies to both CLI and TUI usage
- The configuration is immutable after `LspEngine` creation
- Tests can override the default via `LspEngineConfig::with_server_override()`

---

### Phase 6: Error Messages and Exit Codes

Ensure all error paths produce the correct stderr messages and exit codes.

#### Phase 6.1: Error Message Standardization

**File**: `src/commands/chord.rs` and `src/main.rs`

**Error patterns** (from the work item spec):

| Failure | Stderr message pattern |
|---|---|
| File not found | `error: file not found: <path>` |
| Invalid chord string | `error: invalid chord '<chord>': <reason>` |
| No language detected | `error: no language server available for <path>` |
| LSP install failed | `error: LSP server for <language> failed to start` |
| LSP start failed | `error: LSP server for <language> failed to start` |
| LSP start timeout | `error: LSP server for <language> did not become ready within 30s` |
| No symbol match | `error: no <Scope> found at <target> in <file>` |
| Ambiguous match | `error: ambiguous match — <list of candidates>` |
| Empty chord.text for Change/Insert | `error: chord requires replacement text (e.g. cifb(some_fn, "new body"))` |
| `-` used but stdin is a TTY | `error: chord parameter '-' requires piped input on stdin` |
| Stdin read error when `-` used | `error: failed to read stdin: <io error>` |
| Binary file rejection | `error: file is not valid UTF-8: <path>` |
| Out-of-range line | `error: line <N> out of range (file has <M> lines)` |

**Implementation**:
- Use `anyhow::bail!()` with exact error strings (no extra context that would change the message)
- Let anyhow add the "error: " prefix automatically when displayed
- All errors are propagated to `main.rs` and printed to stderr via error handling

**Test coverage**:
- Unit test: Each error message format matches the expected pattern
- Unit test: Error messages don't regress between test runs
- CLI test: Verify full error paths via the binary

---

## Testing Strategy

The test suite validates all phases of the implementation via unit and integration tests.

### Test File Location

**File**: `tests/work_item_0003.rs`

This file already contains comprehensive test cases; the implementation should pass all of them.

### Test Categories

#### 1. **Line-scope exec without LSP** (lines 18–48)
- **Test**: `line_scope_exec_updates_file_and_does_not_start_lsp`
- **Validates**: Non-LSP chords don't start a server, even with a poisoned config
- **Key assertion**: File is updated correctly without touching LSP infrastructure

#### 2. **LSP-scope exec with mock server** (lines 50–91)
- **Test**: `lsp_scope_exec_with_mock_server_modifies_file`
- **Validates**: Full pipeline with LSP (parse → start → await → resolve → patch → write)
- **Key assertion**: File is updated with the mock server's symbol resolution

#### 3. **LSP-scope exec with startup failure** (lines 93–123)
- **Test**: `lsp_scope_exec_with_nonexistent_binary_returns_error_promptly`
- **Validates**: Error handling when LSP server binary doesn't exist
- **Key assertion**: Returns error quickly (under 8 seconds) instead of timing out

#### 4. **Idempotent (no-change) exec** (lines 125–151)
- **Test**: `no_change_chord_produces_empty_stdout_and_zero_exit`
- **Validates**: Chords that produce no changes exit 0 with empty stdout
- **Key assertion**: No spurious diff is printed for idempotent operations

#### 5. **Binary file rejection** (lines 153–204)
- **Tests**: `binary_file_rejected_with_nonzero_exit_and_stderr` (and via binary)
- **Validates**: Non-UTF-8 files are rejected before any processing
- **Key assertion**: Exit code 1 and error message on stderr

#### 6. **Out-of-range target** (lines 206–245)
- **Tests**: `out_of_range_line_returns_error` (and via binary)
- **Validates**: Line numbers beyond EOF are rejected with clear errors
- **Key assertion**: Error message mentions "out of range"

#### 7. **Yank action** (lines 247–278)
- **Test**: `yank_writes_content_to_stdout_not_to_file`
- **Validates**: Yank actions output content to stdout, not a diff, and don't modify the file
- **Key assertion**: File remains unchanged after yank

#### 8. **End-to-end smoke test** (lines 280–326)
- **Test**: `end_to_end_smoke_test_diff_output_and_file_update`
- **Validates**: Full pipeline from binary invocation to diff output and file update
- **Key assertion**: Diff shows removed and added lines; file is updated correctly

#### 9. **Stdin sentinel handling** (lines 328–369)
- **Test**: `stdin_sentinel_piped_content_replaces_value_parameter`
- **Validates**: The `-` sentinel is replaced with piped stdin content
- **Key assertion**: Piped content replaces the parameter value; old content is gone

### Running the Tests

```bash
cargo test work_item_0003
```

All tests should pass with the implementation complete.

---

## Edge Cases and Invariants

### File Handling

1. **No diff / idempotent chord**: Exit 0, print nothing to stdout
2. **File with no trailing newline**: Preserve the original trailing-newline behavior
3. **Binary files**: Reject early with error before any processing
4. **Path canonicalization**: Use absolute paths for buffer keys and LSP URIs
5. **File write atomicity**: Use `std::fs::write` which provides atomic semantics; if write fails, the original is unchanged

### LSP Lifecycle

1. **No LSP needed**: Skip the entire LSP block if `chord.requires_lsp` is false
2. **Auto-install**: The engine installs missing servers automatically; no caller logic needed
3. **Startup timeout**: Block for up to 30 seconds; longer timeouts indicate problems
4. **Server shutdown**: `LspEngine` implements `Drop`; cleanup happens automatically

### Chord Parameters

1. **`-` sentinel with TTY stdin**: Detect with `is_terminal()` and fail immediately
2. **`-` used for multiple parameters**: All are replaced with the same stdin content
3. **Stdin contains trailing newline**: Strip exactly one trailing `\n`
4. **Very large stdin**: No size limit; full content is buffered (acceptable for code edits)
5. **Bare chord syntax**: Reject old form (e.g., `cifb` without parameters) with helpful error

### Error Messages

1. All errors are human-readable and sent to stderr
2. Exit code is always 1 on failure
3. Error messages are consistent and testable

---

## Codebase Integration Checklist

- [ ] **Layer 0**: No changes; uses existing `Buffer`, LSP types, and registry
- [ ] **Layer 1**: Add `execute_chord()` and `execute_chord_with_config()` to `src/commands/chord.rs`
- [ ] **Layer 1**: Change `auto_install: false` → `true` in `LspEngineConfig::default()`
- [ ] **Layer 2**: Add `CliFrontend` struct and `ApplyChordAction` implementation in `src/frontend/cli_frontend.rs`
- [ ] **Layer 2**: Update `main.rs` to add `exec` command and output handling
- [ ] **Architecture**: Verify no circular imports between layers (use `cargo check`)
- [ ] **Error handling**: All errors use `anyhow::Result` / `bail!` / `?`
- [ ] **Tests**: All tests in `tests/work_item_0003.rs` pass
- [ ] **Code style**: `cargo fmt --check` and `cargo clippy -- -D warnings`
- [ ] **Documentation**: Update CLAUDE.md if any commands or APIs change

---

## Implementation Checklist

### Phase 1: Infrastructure
- [ ] Create `src/frontend/cli_frontend.rs` with `CliFrontend` struct
- [ ] Implement `ApplyChordAction` for `CliFrontend`
- [ ] Add `apply_diff()` helper method
- [ ] Unit tests for `apply()` with Diff, Yank, and no-op actions

### Phase 2: Chord Parsing
- [ ] Verify `parse_chord()` handles full grammar
- [ ] Verify `requires_lsp` is set correctly
- [ ] Add validation for bare chords (reject without parameters)
- [ ] Unit tests for `requires_lsp` on LSP vs non-LSP scopes
- [ ] Unit test for bare chord rejection

### Phase 3: Core Pipeline
- [ ] Add `ChordResult` struct to `src/commands/chord.rs`
- [ ] Implement `execute_chord()` function
  - [ ] Path canonicalization
  - [ ] Buffer load
  - [ ] Stdin sentinel handling
  - [ ] LSP lifecycle (if needed)
  - [ ] Chord execution
  - [ ] Frontend application
  - [ ] File write (conditional)
- [ ] Implement `execute_chord_with_config()` for testing
- [ ] All tests in phases 1-9 pass

### Phase 4: Main Entry Point
- [ ] Add `exec` command to clap subcommands
- [ ] Handle `Exec` variant in `main()`
- [ ] Output diff or yanked content to stdout
- [ ] Error messages to stderr
- [ ] Verify exit codes

### Phase 5: LSP Configuration
- [ ] Change `auto_install: true` in `LspEngineConfig::default()`
- [ ] Add documentation comment
- [ ] Verify TUI and CLI use the new default

### Phase 6: Error Messages
- [ ] Implement all error message patterns from the spec
- [ ] Verify error messages are consistent
- [ ] Unit tests for each error format

### Final Verification
- [ ] `cargo check` — no architecture violations
- [ ] `cargo test work_item_0003` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no lints
- [ ] `cargo fmt --check` — code is formatted
- [ ] Manual smoke test: `ane exec --chord "cels(line:0, value:replaced)" test.rs`

---

## Glossary

- **Chord**: A 4-part grammar (action, positional, scope, component) that specifies an edit operation
- **Yank**: An operation that extracts content without modifying the file
- **Patch**: The result of resolving a chord and computing the diff; represented as a `ChordAction`
- **Diff**: A unified diff showing removed and added lines
- **LSP scope**: A scope that requires language server support (Function, Variable, Struct, etc.)
- **Non-LSP scope**: A scope that doesn't need LSP (Line, Buffer)
- **Auto-install**: The LSP engine's ability to automatically install missing language servers
- **TTY**: A terminal input device; `is_terminal()` checks if stdin is interactive
- **Sentinel**: A special value (like `-`) that triggers special handling (reading from stdin)

---

## References

- **Work Item 0001**: Implement LSPEngine — `aspec/work-items/0001-lsp-engine.md`
- **Work Item 0002**: Implement ChordEngine — `aspec/work-items/0002-chord-engine.md`
- **Work Item Spec**: `aspec/work-items/0003-full-cli-implementation.md`
- **Chord Engine Design**: `aspec/architecture/chord-engine.md`
- **LSP Engine Design**: `aspec/architecture/lsp-engine.md`
- **Test Suite**: `tests/work_item_0003.rs`
- **Foundation**: `aspec/foundation.md`
- **CLI Design**: `aspec/uxui/cli.md`
