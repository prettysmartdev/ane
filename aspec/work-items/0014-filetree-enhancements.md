# Work Item: Feature + Bug

Title: filetree enhancements
Issue: issuelink

## Summary:
Three keybindings to add to the file tree in the ane TUI, plus one bug fix:

1. `Ctrl-R` — rename the item under the tree cursor (file or folder) inline within its row. If the renamed item is the active buffer's file or an ancestor folder, update the buffer's path accordingly.

2. `Ctrl-D` — delete the item under the tree cursor. Opens a confirmation dialog listing the item's name; for folders, lists direct children with a warning they will also be deleted. Enter confirms, Esc cancels.

3. `Ctrl-N` — create a new file in the directory context of the cursor. If the cursor is on a folder, create inside that folder; if on a file, create as a sibling in the same parent directory. Prompts for a filename inline in the tree.

**Bug:** `ane <file>` + `Ctrl-T` shows `tree error: No such file or directory (os error 2)`. Root cause: when `opened_path` is a bare relative filename (e.g. `notes.txt`), `path.parent()` returns `Some("")` (empty string), not `None`, so `unwrap_or(Path::new("."))` never fires. `FileTree::from_dir("")` then calls `"".canonicalize()` which fails on Linux. Fix: treat an empty parent as `.` before calling `from_dir`.


## User Stories

### User Story 1: Rename files and folders from the tree
As a: user

I want to:
Press `Ctrl-R` while the file tree is focused to rename the item under the cursor inline, editing its name directly in the tree row

So I can:
Rename files and folders without leaving ane, and have any open buffer for that file automatically track the new path so Ctrl-S saves to the renamed location


### User Story 2: Delete files and folders from the tree
As a: user

I want to:
Press `Ctrl-D` while the file tree is focused to delete the selected file or folder, with a confirmation dialog that shows what will be removed (including a folder's direct children) before I commit

So I can:
Clean up files from the project without switching to a shell, while having a safety prompt that prevents accidental deletions


### User Story 3: Create new files from the tree
As a: user

I want to:
Press `Ctrl-N` while the file tree is focused to create a new file, entering its name inline, with the creation directory inferred from where the cursor sits (inside a folder, or alongside a file)

So I can:
Add new files to the right place in the project tree without navigating away from ane or memorising directory paths


## Implementation Details:

### Bug fix — `toggle_tree` empty-parent path

In `src/frontend/tui/app.rs`, `toggle_tree`, the `else` branch that computes `dir`:

```rust
// before
let dir = if state.opened_path.is_dir() {
    state.opened_path.clone()
} else {
    state
        .opened_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
};

// after
let dir = if state.opened_path.is_dir() {
    state.opened_path.clone()
} else {
    match state.opened_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
};
```

`Path::new("file.txt").parent()` returns `Some(Path::new(""))`. The fix treats an empty parent string the same as `None` and falls back to `"."`, which canonicalizes correctly to the current working directory.

---

### New state fields — `src/data/state.rs`

Add to `EditorState`:

```rust
pub tree_rename_state: Option<TreeRenameState>,
pub tree_delete_confirm: Option<TreeDeleteState>,
pub tree_new_file_state: Option<TreeNewFileState>,
```

Add three small structs (also in `state.rs`, or a new `src/data/tree_op_state.rs`):

```rust
pub struct TreeRenameState {
    pub index: usize,       // tree_view index being renamed
    pub input: String,      // current edit buffer (pre-filled with existing name)
    pub cursor: usize,      // byte offset of text cursor within input
}

pub struct TreeDeleteState {
    pub index: usize,
    pub children_preview: Vec<String>, // names of direct children (folders only)
}

pub struct TreeNewFileState {
    pub parent_dir: PathBuf,  // resolved directory where the file will be created
    pub input: String,
    pub cursor: usize,
}
```

---

### Keybinding dispatch — `src/frontend/tui/app.rs`

In `handle_tree_keys`, add three arms before the `_ => {}` catch-all:

```rust
KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
    begin_tree_rename(state);
}
KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
    begin_tree_delete(state);
}
KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
    begin_tree_new_file(state);
}
```

Before dispatching `handle_tree_keys`, add priority guards in the existing key-routing block (similar to how `list_dialog` and `show_exit_modal` are guarded today):

```rust
if state.tree_rename_state.is_some() {
    handle_tree_rename_input(state, code, modifiers, syntax_engine);
    return false;
}
if state.tree_delete_confirm.is_some() {
    handle_tree_delete_input(state, code);
    return false;
}
if state.tree_new_file_state.is_some() {
    handle_tree_new_file_input(state, code, modifiers);
    return false;
}
```

---

### Ctrl-R — rename

**`begin_tree_rename(state)`**:
- Get the `FileEntry` at `state.tree_view[state.tree_selected]`.
- Set `state.tree_rename_state = Some(TreeRenameState { index: state.tree_selected, input: entry.name().to_string(), cursor: entry.name().len() })`.

**`handle_tree_rename_input(state, code, modifiers, syntax_engine)`**:
- `Char(c)` not CONTROL → insert `c` at `cursor`, advance `cursor`.
- `Backspace` → delete char before `cursor`, retreat `cursor`.
- `Left` / `Right` → move `cursor`.
- `Enter` → call `commit_tree_rename(state, syntax_engine)`.
- `Esc` → clear `tree_rename_state`.

**`commit_tree_rename(state, syntax_engine)`**:
- Get the old path from `tree_view[rename_state.index]`.
- Build new path: `old_path.parent().unwrap() / new_name`.
- Validate `new_name` is non-empty, contains no path separators, and the destination does not already exist.
- Call `std::fs::rename(old_path, new_path)`. On error, set `state.status_msg` to the error string and clear `tree_rename_state`.
- On success:
  - Update every `FileEntry` in `state.file_tree.entries` and `state.tree_view` whose path starts with `old_path` (use `rename_subtree`, which already exists in `app.rs` from WI-13).
  - For each open `Buffer` in `state.buffers` whose path starts with `old_path`, rewrite its path to the corresponding new path.
  - If the active buffer's path changed, call `refresh_buffer_caches(state, syntax_engine)`.
  - Clear `tree_rename_state`.
  - Set `state.status_msg` to `"renamed → {new_name}"`.

**Rendering** — in `tree_pane::render`, if `state.tree_rename_state` is `Some(r)` and `r.index` matches the row being drawn, replace the normal name span with the inline edit buffer. Render the input string with a cursor block at `r.cursor`. Use the same selected-row highlight background so the row is still visually active.

---

### Ctrl-D — delete

**`begin_tree_delete(state)`**:
- Get the `FileEntry` at `state.tree_view[state.tree_selected]`.
- Disallow deleting the tree root: if `entry.depth == 0` and it is a directory matching `file_tree.root`, set `state.status_msg = "cannot delete tree root"` and return.
- If it's a folder, collect `state.file_tree.entries` where `depth == entry.depth + 1` and path starts with `entry.path` — these are direct children. Map to name strings.
- Set `state.tree_delete_confirm = Some(TreeDeleteState { index: state.tree_selected, children_preview })`.

**`handle_tree_delete_input(state, code)`**:
- `Enter` → call `commit_tree_delete(state)`.
- `Esc` → clear `tree_delete_confirm`.

**`commit_tree_delete(state)`**:
- Get the path from `tree_view[delete_state.index]`.
- If it's a directory: `std::fs::remove_dir_all(path)`.
- If it's a file: `std::fs::remove_file(path)`.
- On error, set `state.status_msg` to error string and clear `tree_delete_confirm`.
- On success:
  - Remove all entries from `file_tree.entries` and `tree_view` whose path starts with the deleted path.
  - Clamp `tree_selected` to `tree_view.len().saturating_sub(1)`.
  - If any open buffer's path starts with the deleted path, set `state.status_msg` to `"{name} deleted — Ctrl-S to restore"`. Do not close the buffer; the user may Ctrl-S to re-create the file.
  - Clear `tree_delete_confirm`.

**Rendering** — add `render_delete_confirm` in `exit_modal.rs` (alongside existing modal renderers). Use `Clear` + a bordered popup with title `" Delete? "` and a red border. Show `"Delete {name}?"`. If `children_preview` is non-empty, add a blank line and list up to 5 children (e.g. `"  • child.rs"`) followed by `"  (and N more)"` if truncated. Footer: `"  Enter to confirm"`, `"  Esc to cancel"`. Use the existing `centered_rect` helper.

---

### Ctrl-N — new file

**`begin_tree_new_file(state)`**:
- Get the `FileEntry` at `state.tree_view[state.tree_selected]`.
- Resolve `parent_dir`:
  - If `entry.is_dir` → `entry.path.clone()`.
  - Else → `entry.path.parent().unwrap().to_path_buf()`.
- If `tree_view.is_empty()`, fall back to `file_tree.root.clone()`.
- Set `state.tree_new_file_state = Some(TreeNewFileState { parent_dir, input: String::new(), cursor: 0 })`.

**`handle_tree_new_file_input(state, code, modifiers)`**:
- `Char(c)` not CONTROL → insert at `cursor`, advance.
- `Backspace` → delete char before cursor.
- `Left` / `Right` → move cursor.
- `Enter` → call `commit_tree_new_file(state)`.
- `Esc` → clear `tree_new_file_state`.

**`commit_tree_new_file(state)`**:
- Validate `input` is non-empty and contains no path separators; on failure set `status_msg` and return (keep state active so the user can correct).
- Build `new_path = parent_dir / input`.
- If `new_path.exists()`, set `state.status_msg = "file already exists"` and return (leave `tree_new_file_state` active).
- Call `std::fs::File::create(&new_path)`. On error, set `status_msg` and clear.
- On success:
  - Insert a `FileEntry` for `new_path` into `file_tree.entries` in sorted order (reuse `insert_entry_sorted` from WI-13).
  - If the parent directory is expanded in `tree_view`, insert the entry at the correct sorted position and set `tree_selected` to point to it.
  - Open the new file via `state.open_file(&new_path)` then `refresh_buffer_caches`.
  - Exit tree focus: `state.focus_tree = false; state.mode = Mode::Edit; state.status_msg = "-- EDIT --"`.
  - Clear `tree_new_file_state`.

**Rendering** — in `tree_pane::render`, if `tree_new_file_state` is `Some(n)`, render a synthetic inline row after the selected directory's last visible child. Show `"  ○ {input}{cursor_block}"` indented at `parent_entry.depth + 1`, using the same selected-row background.


## Edge Case Considerations:

- **Rename to existing name**: before calling `std::fs::rename`, check `new_path.exists()` and surface `"rename failed: destination exists"` rather than silently clobbering on platforms that allow it.
- **Rename tree root**: `tree_view` entry at depth 0 whose path matches `file_tree.root`. After rename, update `file_tree.root` to the new path so the pane title (derived from `tree.root.file_name()`) reflects the change.
- **Rename open buffer's parent folder**: `rename_subtree` rewrites all paths starting with the old prefix, so buffers inside the renamed folder are updated automatically.
- **Delete active buffer's file**: preserve the buffer in memory; do not close it. Show a status hint so the user knows the file is gone. Ctrl-S will re-create it.
- **Delete folder containing a deeply nested open buffer**: the path-prefix check in `commit_tree_delete` catches all depths; no special case is needed.
- **Ctrl-N on empty tree_view**: fall back to `file_tree.root` as `parent_dir`.
- **Ctrl-N input containing path separators**: reject `/` and `\`; show `"filename cannot contain path separators"`. To create nested files, the user must first expand or create the parent folder.
- **Ctrl-D on tree root**: guarded in `begin_tree_delete`; show `"cannot delete tree root"` and do not open the dialog.
- **Empty `tree_view` after delete**: set `tree_selected = 0`; the pane renders a blank bordered box, preserving existing behavior.
- **Rename/delete with fs watcher active (WI-13)**: `apply_tree_event` in WI-13 is idempotent for entries already removed (`retain` on missing entries is a no-op), so double-processing from both the synchronous commit and the watcher event is safe.
- **Ctrl-T pressed while rename/delete/new-file state is active**: the priority guards return before `handle_tree_keys` is reached, so `Ctrl-T` is effectively absorbed (no tree toggle mid-edit). This avoids dangling op state when the tree hides.
- **Very long filenames in inline edit**: clip rendered display to the row width; the full `input` string is kept in state, display scrolls from the right if it overflows.
- **Esc during rename with unchanged name**: discard state cleanly; no `fs::rename` is issued.


## Test Considerations:

- **Bug fix — empty parent**: unit test `toggle_tree` with `opened_path = PathBuf::from("notes.txt")` (relative bare filename). Assert it does not error and produces a valid `file_tree` rooted at `.` (current working directory).
- **`begin_tree_rename`**: populate `tree_view` with one entry; call `begin_tree_rename`; assert `tree_rename_state` is `Some` with `input` equal to the entry's filename and `cursor` at the end.
- **`commit_tree_rename` success**: create a real file in `TempDir`; point `tree_view[0]` at it; call with a new name; assert old path is gone, new path exists, and `tree_view[0].path` is updated.
- **`commit_tree_rename` updates open buffer path**: add a buffer matching the renamed file's path; after commit, assert `buf.path` reflects the new name.
- **`commit_tree_rename` rejects existing destination**: create two files; rename first to second's name; assert `status_msg` contains `"exists"` and no rename occurred.
- **`begin_tree_delete` children preview**: build a `FileTree` with a folder and two direct children; cursor on the folder; call `begin_tree_delete`; assert `children_preview` contains both child names.
- **`commit_tree_delete` file**: create a temp file; call `commit_tree_delete`; assert file no longer exists and `tree_view` no longer contains the entry.
- **`commit_tree_delete` folder recursive**: create a temp dir with nested contents; call `commit_tree_delete`; assert the entire tree is removed from disk and from `file_tree.entries`.
- **`commit_tree_delete` clamps cursor**: set `tree_selected` to the last index; delete that entry; assert `tree_selected` clamps to the new last valid index.
- **`begin_tree_delete` disallows root**: set cursor to a depth-0 dir matching `file_tree.root`; call `begin_tree_delete`; assert `tree_delete_confirm` remains `None` and `status_msg` contains `"cannot delete tree root"`.
- **`begin_tree_new_file` on file entry**: cursor on a file; assert `parent_dir` equals the file's parent directory.
- **`begin_tree_new_file` on folder entry**: cursor on a folder; assert `parent_dir` equals the folder's own path.
- **`commit_tree_new_file` success**: call with a valid name in a temp dir; assert file exists on disk, appears in `tree_view`, and `active_buffer` is opened to it with mode `Edit`.
- **`commit_tree_new_file` existing file**: call with a name that already exists; assert `status_msg` contains `"already exists"` and `tree_new_file_state` remains `Some`.
- **`commit_tree_new_file` rejects path separators**: set `input = "sub/file.rs"`; assert no file is created and `status_msg` contains `"cannot contain path separators"`.
- **Delete confirm modal truncation**: build `TreeDeleteState` with 8 children; render via `render_delete_confirm`; assert at most 5 names appear and `"(and 3 more)"` is shown.


## Codebase Integration:

- **Layer discipline**: all new state structs (`TreeRenameState`, `TreeDeleteState`, `TreeNewFileState`) belong in `src/data/state.rs` (Layer 0 — plain data, no frontend imports). Handler functions (`begin_tree_rename`, `commit_tree_rename`, etc.) and the modal renderer call belong in `src/frontend/tui/app.rs` (Layer 2). The delete confirmation modal render function belongs in `src/frontend/tui/exit_modal.rs` alongside existing modals. Inline rename/new-file rendering belongs in `src/frontend/tui/tree_pane.rs`.
- **`rename_subtree`** already exists in `app.rs` (added in WI-13). Reuse it in `commit_tree_rename` — do not duplicate the path-prefix rewrite logic.
- **`insert_entry_sorted`** from WI-13 must be reused in `commit_tree_new_file`. If WI-13 has not yet landed, add the helper in this work item and note it for deduplication on merge.
- **`state.open_file`** is an existing method on `EditorState` (`src/data/state.rs`). Use it in `commit_tree_new_file` — it handles adding to `state.buffers` and updating `active_buffer`.
- **Keybinding conflicts**: `Ctrl-R` is bound in Chord mode to cycle chord history but is unbound in tree-focus mode. `Ctrl-D` and `Ctrl-N` are unbound in all modes. Verify by inspecting `handle_tree_keys` and `handle_chord_mode` match arms before adding.
- **Priority routing**: the three op-state guards must appear in the key-routing block _before_ the call to `handle_tree_keys`, mirroring how `list_dialog` and `show_exit_modal` are guarded today (see `app.rs` around line 742).
- **`centered_rect`** in `exit_modal.rs` is currently private. Make it `pub(super)` so `render_delete_confirm` can call it without moving it — no need to expose it beyond the `tui` module.
- Run `cargo clippy -- -D warnings` and `cargo fmt --check` before marking complete.
