# Work Item: Feature

Title: on-disk updates
Issue: issuelink

## Summary:
Two features that keep ane in sync with disk state:

1. When a file open in the TUI changes on disk, detect it in the background and show a contextual hint in the bottom bar. If the buffer is unmodified, offer `Ctrl-O` to reload. If the buffer is dirty, offer `Ctrl-O` to discard changes and reload, or `Ctrl-S` to overwrite disk with the current buffer.

2. The file tree live-updates when the on-disk directory tree changes. New files and folders appear automatically; deletions remove entries from the tree. Edge cases like the open file being deleted, or the tree cursor sitting on a removed/renamed directory, are handled gracefully.


## Agent Tooling Constraint

**This section applies at all times during implementation of this work item.**

Before doing any other work, the implementing agent must:

1. Load the `ane` skill to learn ane's chord grammar, available actions, scopes, components, and usage patterns. This is the primary reference for how to use `ane exec`.
2. Build ane from source and install the binary to a location on `$PATH` (e.g. `cargo install --path . --root /tmp/ane-install` and prepend `/tmp/ane-install/bin` to `$PATH`). Confirm the binary is callable with `ane --help`.

Once installed, the agent **must** use `ane exec` for every file interaction throughout the entire implementation:

- **Reading files** — use `ane exec` chords instead of `cat`, `head`, `tail`, `less`, or built-in file-read tools.
- **Exploring / discovering code** — use `ane exec` chords instead of `grep`, `rg`, `find`, `ag`, or built-in search tools.
- **Editing files** — use `ane exec` chords instead of `sed`, `awk`, `patch`, or built-in file-edit tools.

The agent's own built-in read, edit, and search tools must not be used for normal work. Shell commands unrelated to file content (e.g. `cargo build`, `cargo test`, `git status`, `ls`) are fine.

The **only** exception: if the agent suspects `ane exec` produced an incorrect result, the agent may use a built-in tool (e.g. a native file-read or grep) **solely** to verify the incorrect behaviour. Once verified (or refuted), the agent must log the finding and immediately return to using `ane exec` exclusively. Built-in tools must never be used for forward progress on the implementation itself — only for diagnosing and confirming ane misbehaviour.

If at any point `ane exec` produces an incorrect result — a read returns wrong content, an edit does not apply as expected, a search misses results, or any other misbehaviour — the agent must log the issue in `$GITROOT/ane-findings.md`. This file is a running list maintained for the duration of the work item. Each entry should include:

- A short description of what was attempted (the chord or command).
- What the expected result was.
- What actually happened (verified with a built-in tool if needed).
- Any workaround the agent used.

If the issue is minor and a workaround exists (e.g. a slightly different chord achieves the same result), work around it and keep going. However, if the required result **cannot be achieved** through `ane exec` due to missing functionality or bugs — for example, a file cannot be read, an edit cannot be applied correctly, or a necessary search is impossible — the agent must **stop implementation entirely**. Record the blocking issue in `ane-findings.md` with full detail, and do not proceed with additional implementation work.

## User Stories

### User Story 1:
As a: user

I want to:
See a hint in the bottom bar when the file I'm editing has been modified on disk by another process

So I can:
Choose whether to reload the newer version or keep my local edits, without losing work silently or missing external changes

### User Story 2:
As a: user

I want to:
Have the file tree reflect changes to the directory automatically — new files appear, deleted files disappear — without toggling the tree off and on

So I can:
Stay oriented in the project layout while tools, compilers, or other editors modify files in the background

### User Story 3:
As a: user

I want to:
Get a clear status message if the file I have open is deleted from disk

So I can:
Understand what happened and decide whether to save the buffer (which will re-create the file) or discard it


## Implementation Details:

### New dependency — `notify`

Add to `Cargo.toml`:

```toml
notify = { version = "6", optional = true }
```

Add `dep:notify` to the `frontends` feature list alongside the existing optional deps. The library build path (no `frontends`) does not link `notify`.

Use `notify::RecommendedWatcher` — inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows — for zero-overhead, OS-native event delivery. Pair with a standard `mpsc` channel:

```rust
let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
let watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())?;
```

One watcher instance serves both features. Watch the open file path with `RecursiveMode::NonRecursive` and the tree root with `RecursiveMode::Recursive`. Dynamically call `watcher.watch()` / `watcher.unwatch()` when the open file or tree root changes.

Store the watcher and receiver in a new `FsWatcher` struct in `src/frontend/tui/fs_watcher.rs`.

### Layer 0 — `src/data/buffer.rs`

Add two fields to `Buffer`:

```rust
pub last_disk_mtime: Option<std::time::SystemTime>,
pub disk_changed: bool,
```

Initialise both to `None` / `false` in `Buffer::new()`. After a successful `buf.write()`, reset `disk_changed = false` and refresh `last_disk_mtime` from the file's metadata. After a reload from disk, do the same.

Add a method:

```rust
pub fn record_disk_mtime(&mut self) {
    self.last_disk_mtime = std::fs::metadata(&self.path)
        .and_then(|m| m.modified())
        .ok();
}
```

Call this once when a buffer is first loaded so the initial mtime is known.

### Layer 0 — `src/data/state.rs`

Add to `EditorState`:

```rust
pub disk_changed_path: Option<PathBuf>,  // path of file whose disk_changed flag is set
```

This lets `status_bar` display the hint without re-querying the active buffer on every draw.

### Layer 2 — `src/frontend/tui/fs_watcher.rs` (new file)

```rust
pub struct FsWatcher {
    watcher: notify::RecommendedWatcher,
    pub rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    watched_file: Option<PathBuf>,
    watched_tree: Option<PathBuf>,
}

impl FsWatcher {
    pub fn new() -> Result<Self> { ... }
    pub fn watch_file(&mut self, path: &Path) -> Result<()> { ... }
    pub fn unwatch_file(&mut self) { ... }
    pub fn watch_tree(&mut self, root: &Path) -> Result<()> { ... }
    pub fn unwatch_tree(&mut self) { ... }
}
```

`FsWatcher` is created once at TUI startup and stored in the `event_loop` local scope (not in `EditorState`, to avoid mixing I/O handles into data-layer state). Pass a `&mut FsWatcher` parameter alongside the existing `engine` / `syntax_engine` arguments to `event_loop`.

### Layer 2 — `src/frontend/tui/app.rs`

**Startup**: after initialising LSP, construct `FsWatcher::new()`; if the initial path is a file, call `watcher.watch_file()` and record the buffer's initial mtime via `buf.record_disk_mtime()`. If a tree is already open, call `watcher.watch_tree()`.

**Event loop — drain fs events each tick** (at the top of the loop before `event::poll`):

```rust
while let Ok(event) = fs_watcher.rx.try_recv() {
    handle_fs_event(event, state, &mut fs_watcher);
}
```

`handle_fs_event` dispatches on `event.kind`:

- `EventKind::Modify(ModifyKind::Data(_))` or `EventKind::Modify(ModifyKind::Metadata(_))` for the watched file path → compare new mtime against `buf.last_disk_mtime`; if newer, set `buf.disk_changed = true` and `state.disk_changed_path = Some(buf.path.file_name())`.
- `EventKind::Remove(_)` for the watched file path → set `state.status_msg` to `"{filename} deleted from disk. Ctrl-S to restore."`. Clear `buf.disk_changed` (the file is gone, not changed). Unwatch the path.
- `EventKind::Create(_)` / `EventKind::Remove(_)` / `EventKind::Modify(ModifyKind::Name(_))` for paths under the tree root → call `apply_tree_event(state, &event)` (see below).

**File switch** (when `state.pending_open_path` resolves to a new buffer): unwatch the old file, watch the new one, record its mtime, clear `disk_changed` on the outgoing buffer.

**Tree open** (`Ctrl-T`): after building `FileTree::from_dir()`, call `watcher.watch_tree(root)`.

**Tree close** (`Ctrl-T` toggle off): call `watcher.unwatch_tree()`.

**`Ctrl-O` key handling** — add a branch in `handle_key` before the chord/edit dispatch (so it fires regardless of mode when `disk_changed` is set):

```rust
if state.active_buffer_disk_changed() && key == Ctrl-O {
    reload_buffer_from_disk(state)?;
    // reload_buffer_from_disk: read file, replace buf.lines, clear dirty, clear disk_changed,
    // refresh mtime, call refresh_buffer_caches
    return Ok(true);
}
```

`Ctrl-S` in the dirty + disk-changed state writes normally (`buf.write()`) which already clears `dirty` and will reset `disk_changed` after write (via the updated `write()` path in Layer 0).

**`apply_tree_event`** in `app.rs`:

```rust
fn apply_tree_event(state: &mut EditorState, event: &notify::Event) {
    let Some(tree) = &mut state.file_tree else { return };
    match event.kind {
        EventKind::Create(_) => {
            for path in &event.paths {
                insert_entry_sorted(tree, path);
                propagate_to_tree_view(state, path, TreeViewOp::Insert);
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                tree.entries.retain(|e| !e.path.starts_with(path));
                propagate_to_tree_view(state, path, TreeViewOp::Remove);
                handle_removed_path(state, path);
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            // event.paths = [from, to]
            rename_entry(tree, &event.paths[0], &event.paths[1]);
            propagate_rename_to_tree_view(state, &event.paths[0], &event.paths[1]);
        }
        _ => {}
    }
}
```

`insert_entry_sorted` inserts a new `FileEntry` into `FileTree.entries` in the same sorted order that `WalkDir::sort_by_file_name` would produce — compare parent path, then filename lexicographically.

`handle_removed_path` checks:
- If `path == buf.path` for the active buffer → set `state.status_msg` to `"{filename} deleted from disk. Ctrl-S to restore."`.
- If `state.tree_selected` points to a `tree_view` index that no longer exists → clamp to `tree_view.len().saturating_sub(1)`.

### Layer 2 — `src/frontend/tui/status_bar.rs`

Extend the hint rendering section. After the existing selection hint check, add a higher-priority disk-change hint:

```rust
if let Some(buf) = state.current_buffer() {
    if buf.disk_changed {
        let fname = buf.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let hint = if buf.dirty {
            format!(" {} changed on disk. Ctrl-O to open and discard changes, Ctrl-S to overwrite with current changes ", fname)
        } else {
            format!(" {} changed on disk. Ctrl-O to open newer version ", fname)
        };
        // render hint right-aligned, same style as the selection hint (light blue bg)
        // this block takes priority over the selection hint; skip the selection hint if disk_changed
        return;
    }
}
```

Render the disk-change hint with the same `Color::LightBlue` background and bold text as the existing `Ctrl-Y: copy` hint so it is visually consistent. Because both hints are right-anchored, only one is shown at a time; disk-change takes priority.


## Edge Case Considerations:

- **Active file deleted on disk**: `buf.disk_changed` is NOT set (the file is gone, not newer). Instead, show `"{filename} deleted from disk. Ctrl-S to restore."` in `state.status_msg`. The buffer content is preserved in memory; `Ctrl-S` writes it, re-creating the file. The fs watcher unregisters the deleted path so no further spurious events fire.
- **Active file renamed/moved externally**: notify generates a Remove event for the old path. Treat it like deletion. The buffer's `path` field still points to the old name; saving will re-create the old file, which may not be the user's intent — the status message should reflect this.
- **Spurious `mtime` change without content change**: some editors do a write-then-truncate pattern that changes mtime without changing content. The flag is still set; the user sees the hint but if they reload they get identical content. Acceptable — checking content hash on every tick is too expensive.
- **File replaced atomically (rename into place)**: generates a Remove + Create pair or a single `Modify(Name)`. Either path results in `disk_changed` being set because the new file's mtime differs.
- **Tree cursor on deleted directory**: `handle_removed_path` clamps `tree_selected` to a valid index after removing entries. If the tree becomes empty (root deleted?), set `tree_selected = 0` and `state.file_tree = None`, closing the tree pane.
- **Tree cursor on renamed directory**: the entry's path is updated in `tree_view` in place; cursor index is unchanged. The highlighted entry still looks selected correctly.
- **notify watcher fails to start** (e.g. inotify limit reached, permission denied): wrap construction in a `Result`; if it fails, disable watching silently and continue — the editor still works, just without live updates. Optionally set `state.status_msg` to a one-time warning.
- **Rapid bursts of fs events** (e.g. `cargo build` writing many files): `try_recv` draining in a loop is non-blocking, so burst events are processed within the next few ticks. Batch-applying tree insertions in one pass avoids redundant `tree_view` scans.
- **Tree not visible but watcher running**: continue accumulating and applying events to `FileTree.entries` so the tree is up-to-date when the user re-opens it. `tree_view` is rebuilt from `FileTree.entries` on `expand` calls, so correctness is maintained.
- **`disk_changed` flag persists across mode switches**: the hint remains visible in both Edit and Chord modes until the user takes action. `Ctrl-O` is handled before the mode dispatch so it fires in any mode.
- **Multiple buffers**: `disk_changed` lives on `Buffer`, not on `EditorState`. Only the active buffer's flag is shown in the hint. Background buffers accumulate `disk_changed` silently; the hint will appear when the user switches to them. Only the active buffer's file path is watched; switching buffers re-registers the watcher.
- **`notify` on Linux with inotify**: inotify watch descriptors are per-path. After `unwatch()`, re-watch the new path cleanly. Check for `ENOSPC` (`inotify` limit) and handle gracefully.
- **Binary or very large files**: `reload_buffer_from_disk` reads the file line by line (same as initial load). No special handling needed beyond what the existing buffer loader does.


## Test Considerations:

- **`Buffer::record_disk_mtime`** — write a temp file, call the method, assert `last_disk_mtime` is `Some`. Modify the file and call again; assert the stored time is newer.
- **`disk_changed` flag** — simulate an fs event for the active buffer's path in a unit test by directly calling `handle_fs_event` with a synthesised `Modify` event. Assert `buf.disk_changed == true`.
- **`reload_buffer_from_disk`** — write a temp file with known content, load it as a buffer, overwrite the file with different content, call `reload_buffer_from_disk`, assert `buf.lines` now reflects the new content, `buf.dirty == false`, `buf.disk_changed == false`.
- **Status bar hint — clean buffer**: set `buf.disk_changed = true`, `buf.dirty = false`; call `status_bar::render` against a mock frame; assert the hint text contains `"Ctrl-O to open newer version"` and does NOT contain `"discard changes"`.
- **Status bar hint — dirty buffer**: set both `disk_changed = true` and `dirty = true`; assert the rendered hint contains both `"Ctrl-O to open and discard changes"` and `"Ctrl-S to overwrite"`.
- **Hint priority**: set both `disk_changed = true` and `state.selection = Some(...)`. Assert the disk-change hint is shown, not the `Ctrl-Y: copy` hint.
- **`apply_tree_event` — insert**: build a `FileTree` from a temp dir, synthesise a `Create` event for a new file inside it; call `apply_tree_event`; assert the file appears in `tree.entries` in sorted order.
- **`apply_tree_event` — remove**: add an entry to `tree_view` (expanded), synthesise a `Remove` event; call `apply_tree_event`; assert the entry is gone from both `FileTree.entries` and `tree_view`.
- **`apply_tree_event` — cursor clamping**: set `tree_selected` to the index of the last entry; remove that entry; assert `tree_selected` is clamped to the new last valid index.
- **Active file deleted**: synthesise a `Remove` event for the active buffer's path; assert `state.status_msg` contains `"deleted from disk"` and `buf.disk_changed == false`.
- **`Ctrl-S` clears `disk_changed`**: set `disk_changed = true`, `dirty = true`; simulate `Ctrl-S` via `buf.write()`; assert `disk_changed == false` afterwards.
- **`FsWatcher::watch_file` / `unwatch_file` round-trip**: integration test that creates a real temp file, starts a watcher, writes to the file, and receives an event within a short timeout (e.g. 1 second).


## Codebase Integration:

- `notify` is an optional dependency gated behind the `frontends` feature (alongside `crossterm`, `ratatui`, etc.). Add `dep:notify` to the feature list in `Cargo.toml`. The library build path never links it.
- `FsWatcher` lives in `src/frontend/tui/fs_watcher.rs` (Layer 2). It owns the `notify::RecommendedWatcher` and the `mpsc::Receiver`. It is NOT stored in `EditorState` (which is Layer 0 data); pass it as a `&mut FsWatcher` parameter into `event_loop` alongside the existing `engine` and `syntax_engine` parameters.
- `buf.disk_changed` and `buf.last_disk_mtime` are Layer 0 fields on `Buffer` — plain data types (`bool`, `Option<SystemTime>`) with no frontend imports. The flag is set by `handle_fs_event` (Layer 2) and read by `status_bar::render` (Layer 2).
- `handle_fs_event` and `apply_tree_event` are free functions in `app.rs` (or a small submodule `fs_events.rs`) — they are Layer 2, not methods on `EditorState`, which keeps I/O-touching logic out of the data layer.
- `insert_entry_sorted` must produce the same order as `WalkDir::sort_by_file_name`. The simplest correct implementation: after inserting, re-sort by path. For trees ≤ a few thousand entries, this is imperceptibly fast; note the approach so a future optimisation can binary-search if needed.
- `Ctrl-O` is currently unbound. Handle it in `handle_key` before the mode dispatch, in the same style as `Ctrl-T` (tree toggle) and `Ctrl-C` (exit modal) — explicit `KeyCode::Char('o')` + `KeyModifiers::CONTROL` match arm.
- `reload_buffer_from_disk` is a new helper in `app.rs` that mirrors the existing initial file-load code path. It must call `refresh_buffer_caches` after loading and update `state.cursor_line` / `state.scroll_offset` if the new file is shorter than the cursor position.
- Run `cargo clippy -- -D warnings` and `cargo fmt --check` before marking complete.
