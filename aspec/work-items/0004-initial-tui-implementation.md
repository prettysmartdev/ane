# Work Item: Feature

Title: initial TUI implementation
Issue: issuelink

## Summary

Implement the initial interactive TUI for ane across three main components: the editor pane (file display, edit/chord mode, basic Rust syntax highlighting, title bar and status bar), the chord box (floating rounded rect with auto-submit logic and clear visual state machine), and the file tree pane (expandable directory tree, row-highlighted navigation, unsaved-changes guard on open).

## User Stories

### User Story 1
As a: developer

I want to: open a file or directory in ane and edit it in a terminal UI with proper cursor navigation, Ctrl-S to save, and a clear unsaved-changes indicator

So I can: make edits and run chord transformations without leaving my terminal or using a separate editor

### User Story 2
As a: developer

I want to: type short-form chords (e.g. `cifn`) in chord mode and have them auto-submit the moment they are unambiguously complete, with my current cursor position injected automatically

So I can: trigger code transformations with minimal keystrokes and without manually constructing argument syntax

### User Story 3
As a: developer

I want to: open a project directory and navigate its file tree with arrow keys, expand/collapse folders, and open files into the editor pane

So I can: quickly move between files without remembering paths

---

## Implementation Details

### State additions — `src/data/state.rs` (Layer 0)

Extend `EditorState` with:

```rust
pub chord_cursor_col: usize,          // cursor position within chord_input string
pub chord_error: bool,                // chord box shows red border
pub chord_running: bool,              // chord is executing (grey text, yellow border)
pub pre_tree_mode: Mode,              // mode that was active before Ctrl-T focused the tree
pub pending_open_path: Option<PathBuf>, // file the user wants to open while buffer is dirty
pub tree_view: Vec<FileEntry>,                          // cached flat list of currently-visible tree entries
pub lsp_state: Arc<Mutex<LspSharedState>>,              // written by async LSP tasks, read by render loop
```

`LspSharedState` is a plain struct (Layer 0, `src/data/lsp/types.rs`) holding:

```rust
pub struct LspSharedState {
    pub status: ServerState,
    pub semantic_tokens: Vec<SemanticToken>,
}
```

`SemanticToken` is also in `src/data/lsp/types.rs` and carries at minimum: `line`, `start_col`, `length`, `token_type: String`.

Both `status` and `semantic_tokens` live behind the same mutex so a token fetch and a status update never produce a torn read in the render loop. The render loop acquires the lock briefly at the top of each draw call, clones the relevant fields into locals, and releases it before doing any rendering work.

`tree_view` is the single source of truth for what the tree pane renders. It is a flat, ordered list of `FileEntry` values that are currently visible — collapsed directories are present but none of their descendants are. It is never recomputed during rendering; it is only mutated by expand/collapse operations and on initial tree load. Remove `tree_collapsed` — the expansion state is implicit in `tree_view` (if a dir's children are absent, it is collapsed).

Reset `chord_cursor_col`, `chord_error`, and `chord_running` together whenever `chord_input` is cleared.

### Chord auto-submit logic — `src/commands/chord_engine/` (Layer 1)

Add a public function to `ChordEngine`:

```rust
pub fn try_auto_submit_short(input: &str, cursor_line: usize, cursor_col: usize) -> Option<ChordQuery>
```

- Returns `None` silently on any parse error (caller must not show error state).
- Only attempts if `input.len() == 4` and `input.chars().next()` is lowercase.
- Constructs `format!("{}(cursor_pos[{},{}])", input, cursor_line, cursor_col)` and calls `ChordEngine::parse()`.
- If `Ok`, returns the `ChordQuery` with `args.cursor_pos` set. Otherwise `None`.

Long-form auto-submit: when `chord_input` ends with `)` and the first char is uppercase, call `ChordEngine::parse(&chord_input)`. If `Ok`, submit. If `Err`, do not show error (user may still be typing). Only show error when the user presses Enter.

### TUI layout — `src/frontend/tui/app.rs` (Layer 2)

The draw function performs two sequential layout splits on every frame.

**Horizontal split** (applied first, full terminal area):
- If `state.file_tree.is_some() && state.focus_tree_open` (tree pane is visible): `[Constraint::Percentage(25), Constraint::Percentage(75)]`
- Otherwise: `[Constraint::Length(0), Constraint::Percentage(100)]` — zero-width left column so the right slot still gives the editor 100% of the width without a conditional branch in downstream render calls

The left slot is passed to `tree_pane::render`; the right slot is the `editor_area`.

**Vertical split** (applied to `editor_area`):
```
[Constraint::Length(1)]   → title_bar area
[Constraint::Min(0)]      → editor_pane area
[Constraint::Length(1)]   → status_bar area
```

These are always derived from `editor_area`, so when the tree opens or closes the right column's `editor_area` shrinks or expands and the title bar, editor, status bar, and chord box all reflow automatically with no extra logic.

The chord box is a floating overlay rendered last (so it draws on top of the editor pane). Compute its `Rect` from `editor_area`:

- width = `editor_area.width.saturating_sub(2)`
- height = 3 (border top + content + border bottom)
- x = `editor_area.x + 1`
- y = `editor_area.bottom().saturating_sub(4)` (1 row above the status bar)

If `editor_area.width < 4` or `editor_area.height < 5`, skip rendering the chord box entirely.

Render the chord box only when `state.mode == Mode::Chord`.

### Editor pane — `src/frontend/tui/editor_pane.rs` (Layer 2)

**Cursor rendering:**
- In `Mode::Edit`: call `frame.set_cursor_position(Position { x, y })` at the character cell for `(cursor_line - scroll_offset, cursor_col + line_num_width)`. The terminal blinks this cursor natively.
- In `Mode::Chord`: do NOT call `set_cursor_position`; instead render the character at the cursor position with `Style::default().bg(Color::Blue)` (static, no blink). This makes the chord-target position visible without cursor movement.

**Syntax highlighting:**
- Syntax highlighting is driven exclusively by LSP semantic tokens. There is no static fallback highlighter.
- When `state.lsp_status != ServerState::Running`, render every line as unstyled text. No partial or heuristic coloring.
- When `state.lsp_status == ServerState::Running`, request semantic tokens from the LSP for the active buffer on file open and after each save. Cache the token list in `EditorState` (e.g. `semantic_tokens: Vec<SemanticToken>`). On each draw call, map token ranges to ratatui `Span` styles in `editor_pane::render`.
- Semantic token type → color mapping lives in `editor_pane.rs` as a simple `match` on the token type string (e.g. `"keyword"` → `Color::Blue`, `"string"` → `Color::Green`, `"comment"` → `Color::DarkGray`, `"type"` → `Color::Cyan`, etc.). This is the only language-specific logic needed — the LSP tells us the token types, ane just assigns colors.

**Title bar — `src/frontend/tui/title_bar.rs` (new file, Layer 2):**

Render a 1-row `Paragraph` (no border) showing:
- Left: filename (basename of active buffer path, or root dir name if no buffer)
- Right: `[+]` in yellow when `buf.dirty`, checkmark or empty otherwise

**Status bar — `src/frontend/tui/status_bar.rs` (Layer 2):**

Currently renders mode + file + position + LSP. Keep as-is but ensure the LSP label updates from `state.lsp_status` on every frame (already the case).

### Chord box — `src/frontend/tui/chord_box.rs` (rename from `command_bar.rs`, Layer 2)

Visual state machine:

| State | Border color | Text color |
|---|---|---|
| Normal (idle) | Blue | White |
| Running (`chord_running`) | Yellow | DarkGray |
| Error (`chord_error`) | Red | White |

Render the chord cursor (blinking block `█`) at `chord_cursor_col` inside the input text. Use `frame.set_cursor_position()` to place it so the terminal blinks it natively; this cursor must disappear when focus is in the tree or the editor is in edit mode (do not call `set_cursor_position` then).

Left/right arrow keys in chord mode adjust `chord_cursor_col`, clamped to `[0, chord_input.len()]`. Typed characters insert at `chord_cursor_col`; backspace deletes the character before it.

### File tree pane — `src/frontend/tui/tree_pane.rs` (Layer 2)

The pane renders inside a rounded-border block (white border), with the title set to `state.file_tree.as_ref().map(|t| t.root.display().to_string())`.

**Rendering:** iterate `state.tree_view` directly — no filtering, no ancestor checks. Every entry in the slice is visible by definition. Rendering is O(visible rows), not O(total tree size).

**Determining expansion state for icons:** a directory entry in `tree_view` is considered expanded if its immediate next entry (if any) has a greater depth. Use this to pick `▸` (collapsed) vs `▾` (expanded).

**Selection highlight:** render the row at index `state.tree_selected` with `Style::default().bg(Color::DarkGray)` spanning the full row width.

**Indentation:** prefix each entry with `"  ".repeat(entry.depth)`, then the icon, then the entry name.

**`tree_view` mutation operations** (called from `app.rs` key handler, not from the render function):

- **Expand** (Right arrow on a collapsed dir at index `i`):
  1. Look up the dir's path in `state.file_tree` to find its direct children (entries in `file_tree.entries` whose `path.parent() == dir.path` and `depth == dir.depth + 1`).
  2. Insert those children into `tree_view` starting at position `i + 1`, preserving their original order from `file_tree.entries`.
  3. Do not recurse — only immediate children are added; their sub-trees remain absent until the user expands them individually.

- **Collapse** (Left arrow on an expanded dir at index `i`):
  1. Find the contiguous run of entries in `tree_view` starting at `i + 1` whose `depth > dir.depth`. This is the entire visible sub-tree of the dir.
  2. Remove that entire run in one `drain` call.
  3. After removal, clamp `tree_selected` to `tree_view.len().saturating_sub(1)`.

- **Initial population** (on `FileTree` load in `toggle_tree` / `for_directory`):
  - Insert only the top-level entries (`depth == 0`) into `tree_view`. All directories start collapsed.

**Keybindings (when `state.focus_tree`):**
- `Up` / `Down`: move `tree_selected` within `[0, tree_view.len().saturating_sub(1)]`.
- `Right`: if `tree_view[tree_selected]` is a dir and is collapsed (next entry absent or has lesser/equal depth), run expand.
- `Left`: if `tree_view[tree_selected]` is a dir and is expanded (next entry has greater depth), run collapse. If it is a file or already collapsed dir, do nothing.
- `Enter`: if selected entry is a file, attempt to open it:
  - If no dirty buffer: call `state.open_file(path)`.
  - If dirty buffer: set `state.pending_open_path = Some(path)` and render the open-confirm modal.
- `Ctrl-T`: restore focus to editor pane in `state.pre_tree_mode`.

When `Ctrl-T` is pressed while in the editor pane:
1. Save `state.pre_tree_mode = state.mode`.
2. If `state.file_tree.is_none()` (single-file launch): scan the parent directory of the active buffer with `FileTree::from_dir`, assign to `state.file_tree`, then populate `state.tree_view` with depth-0 entries only (all dirs start collapsed). This is the same lazy-load path the existing `toggle_tree` helper uses for single-file mode — extend it rather than duplicating.
3. Set `state.focus_tree = true`.
4. Find the visible-entry index of the active buffer's path and assign it to `state.tree_selected` so the cursor lands on the current file.

### Modals — `src/frontend/tui/exit_modal.rs` (Layer 2)

**Exit modal**: update to check `state.current_buffer().map_or(false, |b| b.dirty)` and render two variants:
- Clean: "Press Ctrl-C again to quit, Esc to cancel"
- Dirty: "Press Ctrl-C again to quit without saving, Ctrl-S to save and quit, Esc to cancel"

**Open-with-unsaved modal** (new, in `exit_modal.rs` or a new `open_modal.rs`):
- Shown when `state.pending_open_path.is_some()`
- Text: "Unsaved changes. Ctrl-S to save and open, Ctrl-O to discard and open, Esc to cancel"
- `Ctrl-S`: save current buffer then open `pending_open_path`
- `Ctrl-O`: discard dirty buffer, open `pending_open_path`
- `Esc`: clear `pending_open_path`, return to editor

### LSP startup and async model — `src/frontend/tui/app.rs` (Layer 2)

**The render loop must never block on any LSP operation.** All LSP queries run in async tasks. Shared LSP results are written through `Arc<Mutex<LspSharedState>>` and read by the render loop in a brief, non-blocking lock acquisition.

`run()` uses a Tokio runtime (or spawns a dedicated thread containing one). Before entering the crossterm event loop:
1. Call `engine.start_for_context` synchronously — this only registers intent and launches the server process; it returns immediately.
2. Clone the `Arc<Mutex<LspSharedState>>` and move it into a spawned async task: the **status-polling task**.
3. Clone the same `Arc` and keep a sender half of a `tokio::sync::mpsc` channel for the **token-request task** (see below).

**Status-polling task**: runs independently of the event loop.
- Polls `engine.server_state(lang)` in a loop.
- While `status != ServerState::Running`: poll every **1 second**.
- Once `status == ServerState::Running`: poll every **3 seconds**.
- On each poll, lock `lsp_state`, update `status`, unlock.
- When status first transitions to `Running`, also trigger an immediate semantic token fetch for the active buffer path (send on the token-request channel).

**Token-request task**: receives `(PathBuf, String)` (path + current buffer content) on the channel and calls `engine.semantic_tokens(path, content)`. On completion, locks `lsp_state`, replaces `semantic_tokens`, unlocks. The task processes one request at a time; any request that arrives while one is in flight is dropped (the debounce sender uses `try_send` on a channel of capacity 1, so older pending requests are naturally superseded).

**Debounce**: every buffer change (keypress in edit mode) resets a `Instant`-based debounce timer. When 300 ms have elapsed since the last change without a new one, the event loop calls `try_send` on the token-request channel with the current buffer path and content. Because the channel has capacity 1 and `try_send` is non-blocking, a burst of keystrokes produces at most one queued request.

**Render loop reads**: at the top of each draw call, acquire the mutex, clone `lsp_state.status` and `lsp_state.semantic_tokens` into frame-local variables, release the mutex. All rendering uses those locals — the mutex is never held across a render.

### Event routing — `src/frontend/tui/app.rs` (Layer 2)

Priority order for key events:
1. Exit modal (if `show_exit_modal`) captures all keys
2. Open modal (if `pending_open_path.is_some()`) captures all keys
3. `Ctrl-T` always handled (any mode/focus)
4. If `focus_tree`: route to tree key handler
5. If `mode == Chord`: route to chord key handler (including arrow keys for `chord_cursor_col`)
6. If `mode == Edit`: route to editor key handler

In the chord key handler, after every character insertion or deletion:
- If `chord_input` is 4 chars and first char is lowercase: call `ChordEngine::try_auto_submit_short`. If `Some(query)`, enter running state and dispatch.
- If `chord_input` ends with `)` and first char is uppercase: attempt long-form auto-submit silently.
- `Enter`: call `ChordEngine::parse(&chord_input)`. If `Err`, set `chord_error = true`. If `Ok`, dispatch.
- When a chord dispatches successfully: clear `chord_input`, `chord_error = false`, `chord_running = false`.
- If chord action sets `mode_after = EditorMode::Edit`: clear chord box before entering edit mode.
- Never clear `chord_input` on a failed auto-submit attempt.

---

## Edge Case Considerations

- **Terminal resize**: All rects are recomputed from `frame.area()` every draw call, so the title bar, editor pane, status bar, and chord box reflow automatically. The editor scroll offset must be clamped to `[0, buf.line_count().saturating_sub(visible_height)]` after resize so the cursor does not scroll off screen.
- **Tree open/close layout reflow**: The horizontal split uses `Constraint::Percentage` values derived from `state.file_tree.is_some() && state.focus_tree`, so the `editor_area` rect passed to the vertical split changes width on every frame when toggled. Because title bar, editor pane, status bar, and chord box all derive their rects from `editor_area`, they all resize in one place with no extra synchronization.
- **Empty buffer**: All cursor operations must be guarded by `buf.lines.len() > 0`. `Buffer::empty` ensures at least one line, but open operations could produce unexpected state.
- **Chord box too narrow**: When `editor_area.width < 4` or `editor_area.height < 5`, skip rendering the chord box entirely rather than producing a zero-size or negative Rect.
- **Auto-submit on ambiguous short chords**: `cifn` and `cifb` both start with `cif`. The auto-submit only fires at exactly 4 chars, so there is no ambiguity — the 4-char input is always a complete short-form chord candidate.
- **Long-form chord with nested parens**: `split_chord_and_args` in `parser.rs` finds the first `(` and requires the string to end with `)`. Nested parens in values (e.g. `value: "foo()"`) will mis-parse. The auto-submit trigger (closing `)`) should only fire when `chord_input.ends_with(')')` and the depth of `(` vs `)` chars is balanced. Count open parens and only trigger when depth reaches zero.
- **File tree with no buffer open**: When launched with a directory, `buffers` is empty. The tree pane operates normally, but title bar and status bar should show placeholder text rather than panicking on `state.current_buffer().unwrap()`.
- **Single-file Ctrl-T failure**: `FileTree::from_dir` could fail (permissions, broken symlink). If it returns `Err`, set `state.status_msg` to the error string and do not set `focus_tree`; the editor remains in its current mode.
- **`tree_selected` after lazy tree load**: When the tree is loaded for the first time via Ctrl-T from single-file mode, scan the visible entry list and set `tree_selected` to the index of the active buffer's path. If the path is not found (e.g. buffer is a new unsaved file), leave `tree_selected = 0`.
- **All dirs collapsed — first navigation**: On initial load, `tree_view` contains only depth-0 entries. `tree_selected = 0` is always valid. Clamp Down navigation to `tree_view.len().saturating_sub(1)`.
- **`tree_selected` out of bounds after collapse**: the drain-based collapse removes a contiguous run from `tree_view`. Always clamp `tree_selected` to `tree_view.len().saturating_sub(1)` immediately after the drain.
- **Expand on a dir whose children are already in `tree_view`**: if the next entry already has `depth == dir.depth + 1`, the dir is already expanded — Right arrow should be a no-op, not a duplicate insert.
- **Switching from tree to chord mode**: `pre_tree_mode` could be `Mode::Chord`. Return to chord mode correctly (do not set cursor position in the editor, do show the chord box cursor).
- **LSP startup latency**: The LSP server goes through `Starting` and possibly `Installing` before reaching `Running`. The editor is fully functional during this window; lines render without highlighting. The status-polling task handles the transition transparently — the render loop just reads whatever `lsp_state.status` currently holds.
- **LSP semantic token lag**: Tokens are updated 300 ms after the last keypress via a debounced async task. During the debounce window and the round-trip to the LSP server, highlight spans may lag behind the buffer content. This is acceptable — do not attempt synchronous or incremental token updates.
- **Mutex contention**: The lock is held only long enough to clone two fields at the top of each draw call, and only long enough to replace `semantic_tokens` in the token-request task. Contention should be negligible. Do not hold the lock across any I/O or LSP call.
- **LSP unavailable for the file's language**: If `primary_language` returns `None` (e.g. a `.txt` file), `engine.start_for_context` has nothing to start. `lsp_state.status` stays `Undetected` and `lsp_state.semantic_tokens` stays empty. No highlighting, no error, no polling task started.
- **Dirty buffer + exit**: if the user triggers exit while a chord is running (`chord_running == true`), delay the exit modal until the chord completes, or show the modal and cancel the running chord.

---

## Test Considerations

- **Unit tests in `src/commands/chord_engine/`**: Add tests for `try_auto_submit_short`:
  - 4-char lowercase valid chord (`cifn`) with a cursor pos returns `Some(query)` with correct `args.cursor_pos`.
  - 4-char lowercase invalid chord (`xxxx`) returns `None` with no error/panic.
  - Input shorter than 4 chars returns `None`.
  - First char uppercase returns `None` (not a short form).
- **Unit tests in `src/data/state.rs`**: Verify that `for_file` initializes `file_tree = None` and `tree_view` empty; `for_directory` initializes `tree_view` with only depth-0 entries (all dirs collapsed by default), and `chord_cursor_col = 0`, `chord_error = false`, `chord_running = false`.
- **Unit tests for `tree_view` mutations** (in `src/frontend/tui/tree_pane.rs` or a helper module): Given a `FileTree` with dirs A, A/B, A/B/C and file A/B/C/d.rs:
  - Initial `tree_view` contains only `[A]`.
  - Expand A → `[A, A/B]` (A/B is a dir, its children absent).
  - Expand A/B → `[A, A/B, A/B/C]`.
  - Expand A/B/C → `[A, A/B, A/B/C, A/B/C/d.rs]`.
  - Collapse A/B/C → `[A, A/B, A/B/C]`, `tree_selected` clamped.
  - Collapse A → `[A]` (entire sub-tree drained in one pass).
  - Right arrow on already-expanded dir is a no-op (no duplicate children inserted).
- **Integration tests in `tests/`**: Extend or add tests verifying chord dispatch from TUI state:
  - Simulate `chord_input = "cifn"`, verify `try_auto_submit_short` produces a query and that `apply` on a real buffer produces a non-empty `ChordResult` diff.
- **Manual test checklist**:
  - Launch `ane src/lib.rs` — editor fills window, no tree visible
  - Launch `ane .` — tree at left 25%, all dirs collapsed showing only top-level entries, no buffer open initially
  - `Ctrl-T` from single-file edit mode — tree is scanned from parent dir, all dirs collapsed, pane opens at left 25%, editor shrinks to 75%, title/status bars shrink with it; current file is highlighted in tree
  - `Ctrl-T` again from tree — tree pane closes, editor expands back to 100%, mode restored to what it was before
  - Resize terminal while tree is open — editor pane and all bars reflow; chord box stays 2 cols narrower than the editor pane
  - Expand a dir in the tree with Right arrow — children appear indented, icon changes from ▸ to ▾
  - Collapse a dir with Left arrow — children disappear, `tree_selected` clamped if needed
  - `Ctrl-T` with single-file mode when parent dir is unreadable — status bar shows error, tree does not open
  - Type `cifn` in chord mode on a Rust file with function at cursor — chord auto-submits
  - Type an invalid 4-char chord — no error shown, user continues typing
  - Press Enter on a partial invalid chord — border turns red
  - While chord is running — text goes grey, border yellow
  - Open a second file from tree while buffer is dirty — open-with-unsaved modal appears
  - Ctrl-C with dirty buffer — exit modal shows save-and-quit variant
  - Cursor blinks in edit mode, stays static blue in chord mode
  - Open a `.rs` file — no syntax highlighting immediately (LSP starting); status bar updates live as LSP status changes; once LSP reaches Running, highlighting appears without any user action
  - Type rapidly in edit mode — highlighting lags by at most ~300 ms + LSP round-trip, but the editor never freezes or stutters
  - Open a `.txt` file — no highlighting ever, no error, status bar shows Undetected, no background polling tasks started
  - Verify the render loop does not stutter when the token-request task is completing (mutex acquisition is non-blocking from the render loop's perspective)

---

## Codebase Integration

- All new `EditorState` fields go in `src/data/state.rs` and must be initialized in both `for_file` and `for_directory`. `for_file` sets `file_tree = None` and `tree_view = Vec::new()`. `for_directory` builds the `FileTree` then populates `tree_view` with only the depth-0 entries from `file_tree.entries` (all dirs start collapsed).
- The lazy tree load on Ctrl-T from single-file mode lives in the existing `toggle_tree` helper in `app.rs` — extend it to populate `tree_view` with depth-0 entries after constructing the `FileTree`, and to set `tree_selected` to the index of the current buffer's path in `tree_view` (or 0 if not found at depth 0).
- `ChordEngine::try_auto_submit_short` lives in `src/commands/chord_engine/mod.rs` alongside the existing `parse`, `resolve`, `patch` methods. It may call `parse` internally and must not be exported through `src/commands/chord.rs` until needed by the CLI.
- The two-step layout (horizontal split first, then vertical split on the right column's area) should be computed once at the top of the draw closure and the resulting rects passed to each render function. This ensures a single source of truth for `editor_area` and prevents the chord box from diverging from the editor pane boundary.
- Rename `command_bar.rs` → `chord_box.rs` and update `mod.rs` and all `use super::command_bar` references in `app.rs`.
- Add `title_bar.rs` to `src/frontend/tui/` and expose it via `mod.rs`.
- Paren-depth balancing for long-form auto-submit: implement as a free function in `src/commands/chord_engine/mod.rs` (Layer 1), not in the TUI layer.
- For hardware cursor placement use `frame.set_cursor_position(ratatui::layout::Position { x, y })`. Guard all calls so only one component sets the cursor per frame — whichever component currently "owns" focus (chord box in chord mode, editor in edit mode, neither in tree mode).
- Blink timing does not need an explicit timer — the terminal blinks a hardware cursor automatically when `set_cursor_position` is called. No `Instant`-based blink state is needed.
- Syntax highlighting is LSP-only. `editor_pane::render` receives frame-local copies of `lsp_status` and `semantic_tokens` cloned from the mutex at the top of the draw call. If `lsp_status != ServerState::Running`, all lines render as plain unstyled text. If `Running`, apply `semantic_tokens` to produce styled `Span` sequences. The token-type-to-color mapping is a `match` in `editor_pane.rs` — no new module, no new crate, no regex, no language-specific logic beyond color assignment.
- `SemanticToken` and `LspSharedState` belong in `src/data/lsp/types.rs` (Layer 0). The async method that fetches semantic tokens belongs in `src/commands/lsp_engine/` (Layer 1). Neither layer imports from `frontend`.
- `Arc<Mutex<LspSharedState>>` is constructed in `run()` and cloned into the status-polling task and the token-request task before the crossterm event loop starts. The render loop holds only the `Arc` clone; it never owns the `LspEngine` directly.
- The status-polling task and token-request task are spawned with `tokio::spawn`. If the project does not yet have a Tokio runtime, introduce one in `run()` with `tokio::runtime::Runtime::new()` and use `runtime.spawn(...)`. The crossterm event loop itself remains synchronous — only the LSP background work is async.
- Debounce timer: track `last_edit: Option<Instant>` in the event loop. On each iteration, if `last_edit.is_some()` and `last_edit.elapsed() >= Duration::from_millis(300)`, call `token_tx.try_send(...)` and clear `last_edit`. On every buffer-modifying keypress, set `last_edit = Some(Instant::now())`.
- The render loop reads `lsp_state` by locking, cloning `status` and `semantic_tokens` into locals, then unlocking — before calling any render function. Never pass the `MutexGuard` into a render function.
- `tree_pane.rs` already exists — extend it rather than replacing it. The `render` function reads `state.tree_view` directly; it does no filtering or tree traversal. Expand/collapse mutation functions (called from `app.rs`) look up children in `state.file_tree.entries` by parent path and depth, then splice into `state.tree_view` with `insert`/`drain`. Keep these as free functions in `tree_pane.rs` (e.g. `pub fn expand(state: &mut EditorState, idx: usize)` and `pub fn collapse(state: &mut EditorState, idx: usize)`) so they can be unit-tested without a full terminal.
- Remove `tree_collapsed: HashSet<PathBuf>` — it is no longer needed. Expansion state is read from `tree_view` structure (presence or absence of children).
- `exit_modal.rs` already exists — add the dirty-buffer variant as a conditional branch on `state.current_buffer().map_or(false, |b| b.dirty)`. The open-with-unsaved modal can share the same centered-rect helper; add it as a second `pub fn render_open_modal(frame, state)` in the same file.
