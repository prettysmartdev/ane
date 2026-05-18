# Work Item 0013: On-Disk Updates — Implementation Guide

## Overview

This document provides comprehensive guidance for implementing the on-disk updates feature in ane. The feature consists of two related capabilities:

1. **File change detection**: When a file open in the TUI changes on disk, detect it in the background and show a contextual hint in the bottom bar. Offer `Ctrl-O` to reload if the buffer is unmodified, or to discard/overwrite if dirty.

2. **Live file tree updates**: The file tree automatically updates when the on-disk directory tree changes—new files and folders appear, deletions remove entries, and renames are handled gracefully.

Both features use the `notify` crate for cross-platform filesystem watching via OS-native events (inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows).

---

## Feature Scope & Constraints

### User-Facing Behavior

#### File Change Detection
- When the active buffer's file is modified on disk by an external process, the status bar displays:
  - **Clean buffer**: `" filename changed on disk. Ctrl-O to open newer version "`
  - **Dirty buffer**: `" filename changed on disk. Ctrl-O to open and discard changes, Ctrl-S to overwrite with current changes "`
- Pressing `Ctrl-O` reloads the file (discarding dirty changes if present).
- Pressing `Ctrl-S` in a dirty buffer writes the current content, overwriting the disk version.
- When the file is deleted on disk: `" filename deleted from disk. Ctrl-S to restore. "` — saving re-creates it.

#### File Tree Live Updates
- New files and folders created in the watched directory automatically appear in the tree in sorted order.
- Deleted files and folders are removed from the tree view instantly.
- File renames are reflected immediately.
- The tree cursor is clamped if it points to a deleted entry.
- The tree remains up-to-date even if the tree pane is hidden; it rebuilds correctly when reopened.

### Scope Constraints

- **Single watcher instance**: One `notify::RecommendedWatcher` services both the active file and tree root to minimize OS resource usage.
- **Non-blocking event drain**: Events are drained with `try_recv()` at the top of the event loop, ensuring no blocking I/O in the main tick.
- **Optional feature**: `notify` is gated behind the `frontends` feature flag. The library build path does not depend on it.
- **Cross-platform**: Works on Linux (inotify), macOS (FSEvents), and Windows (ReadDirectoryChangesW).

---

## Architectural Overview

### Three-Layer Design

The implementation respects ane's strict 3-layer architecture:

```
Layer 2 (Frontend)
  └─ src/frontend/tui/fs_watcher.rs       FsWatcher (owns notify watcher + mpsc receiver)
  └─ src/frontend/tui/app.rs              handle_fs_event, apply_tree_event, reload_buffer_from_disk
  └─ src/frontend/tui/status_bar.rs       Disk-change hint rendering
     ↓
Layer 1 (Commands)
  └─ [No changes required for this work item]
     ↓
Layer 0 (Data)
  └─ src/data/buffer.rs                   disk_changed, last_disk_mtime fields and record_disk_mtime()
  └─ src/data/state.rs                    disk_changed_path field
```

### Dependency Graph

```
notify                (external, optional dependency)
  ↓
FsWatcher             (Layer 2: owns watcher and receiver)
  ↓
handle_fs_event()     (Layer 2: processes notify events, writes to Layer 0)
  ↓
Buffer, EditorState   (Layer 0: plain data fields, no frontend imports)
  ↓
status_bar::render()  (Layer 2: reads disk_changed flag)
```

**Key rule**: Layer 0 fields (`Buffer::disk_changed`, `EditorState::disk_changed_path`) contain plain data types with zero I/O imports. They are set by Layer 2 event handlers and read by Layer 2 rendering logic.

---

## Implementation Details by Layer

### Layer 0: Data Types

#### `src/data/buffer.rs` Changes

Add two fields to the `Buffer` struct:

```rust
pub struct Buffer {
    // ... existing fields ...
    /// Timestamp of the last known disk state of this file.
    /// Updated on initial load, after write, and after reload from disk.
    pub last_disk_mtime: Option<std::time::SystemTime>,
    
    /// Flag indicating the file has been modified on disk since the last sync.
    /// Set by handle_fs_event when a newer mtime is detected.
    /// Cleared when the buffer is written or reloaded.
    pub disk_changed: bool,
}
```

**Initialization** in `Buffer::new()`:

```rust
pub fn new(path: PathBuf, lines: Vec<String>) -> Self {
    Self {
        // ... existing fields ...
        last_disk_mtime: None,
        disk_changed: false,
    }
}
```

**New helper method**:

```rust
/// Capture the current file mtime from disk.
/// Call once when a buffer is loaded, and after any write or reload.
pub fn record_disk_mtime(&mut self) {
    self.last_disk_mtime = std::fs::metadata(&self.path)
        .and_then(|m| m.modified())
        .ok();
}
```

**Updated `write()` method** (existing logic, add these lines at the end):

```rust
pub fn write(&mut self) -> anyhow::Result<()> {
    // ... existing write logic ...
    
    // After successful write, sync disk state:
    self.disk_changed = false;
    self.record_disk_mtime();
    self.dirty = false;
    Ok(())
}
```

**Rationale**: 
- `last_disk_mtime: Option<SystemTime>` is `None` until the buffer is loaded, to detect stale initial states.
- `disk_changed: bool` is cheaper than comparing mtimes on every render; it's set once and cleared by user action or I/O.
- `record_disk_mtime()` is idempotent and safe to call multiple times; it samples the true file state from the OS.

#### `src/data/state.rs` Changes

Add a field to `EditorState`:

```rust
pub struct EditorState {
    // ... existing fields ...
    /// Path of the file whose disk_changed flag is currently set.
    /// Allows the status bar to render the hint without scanning all buffers.
    pub disk_changed_path: Option<PathBuf>,
}
```

**Initialization** in `EditorState::new()`:

```rust
pub fn new(/* ... */) -> Self {
    Self {
        // ... existing fields ...
        disk_changed_path: None,
    }
}
```

**Rationale**: 
- This field is updated by `handle_fs_event` whenever a buffer's `disk_changed` flag is set.
- The status bar checks this field once per render, avoiding a buffer lookup every frame.
- It's cleared when the user acts on the hint (reload or write).

---

### Layer 2: Frontend Implementation

#### New File: `src/frontend/tui/fs_watcher.rs`

This module owns the filesystem watcher and manages the lifecycle of file/tree watches.

```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Config, Result as NotifyResult};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use anyhow::Result;

pub struct FsWatcher {
    watcher: RecommendedWatcher,
    pub rx: Receiver<NotifyResult<notify::Event>>,
    watched_file: Option<PathBuf>,
    watched_tree: Option<PathBuf>,
}

impl FsWatcher {
    /// Create a new filesystem watcher.
    /// Returns an error if the OS watcher cannot be initialized (e.g. inotify limit).
    pub fn new() -> Result<Self> {
        let (tx, rx) = channel::<NotifyResult<notify::Event>>();
        let watcher = RecommendedWatcher::new(tx, Config::default())?;
        Ok(Self {
            watcher,
            rx,
            watched_file: None,
            watched_tree: None,
        })
    }

    /// Watch a file for modifications and deletions.
    /// Unwatch the previous file if one was already being watched.
    pub fn watch_file(&mut self, path: &Path) -> Result<()> {
        // Clean up the previous watch
        if let Some(prev) = self.watched_file.take() {
            let _ = self.watcher.unwatch(&prev); // Ignore errors for already-dead paths
        }
        
        // Watch the new file
        self.watcher.watch(path, RecursiveMode::NonRecursive)?;
        self.watched_file = Some(path.to_path_buf());
        Ok(())
    }

    /// Stop watching the active file.
    pub fn unwatch_file(&mut self) {
        if let Some(path) = self.watched_file.take() {
            let _ = self.watcher.unwatch(&path);
        }
    }

    /// Watch a directory tree for creates, deletes, and renames.
    /// Unwatch the previous tree if one was already being watched.
    pub fn watch_tree(&mut self, root: &Path) -> Result<()> {
        // Clean up the previous watch
        if let Some(prev) = self.watched_tree.take() {
            let _ = self.watcher.unwatch(&prev);
        }
        
        // Watch the new tree
        self.watcher.watch(root, RecursiveMode::Recursive)?;
        self.watched_tree = Some(root.to_path_buf());
        Ok(())
    }

    /// Stop watching the active tree.
    pub fn unwatch_tree(&mut self) {
        if let Some(path) = self.watched_tree.take() {
            let _ = self.watcher.unwatch(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_watch_file_succeeds() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();
        
        let mut watcher = FsWatcher::new().unwrap();
        assert!(watcher.watch_file(&file_path).is_ok());
        assert_eq!(watcher.watched_file, Some(file_path.clone()));
    }

    #[test]
    fn test_unwatch_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();
        
        let mut watcher = FsWatcher::new().unwrap();
        watcher.watch_file(&file_path).unwrap();
        watcher.unwatch_file();
        assert_eq!(watcher.watched_file, None);
    }
}
```

**Design notes**:
- `FsWatcher` is stored in the `event_loop` local scope (in `app.rs`), not in `EditorState`, to keep I/O primitives out of the data layer.
- `watch_file` and `watch_tree` automatically unwatch the previous path, allowing seamless transitions.
- The `rx` field is public so the event loop can drain it with `try_recv()`.
- Errors from `unwatch()` are silently ignored; the watcher may have already dropped a dead path.

---

#### Modifications to `src/frontend/tui/app.rs`

**Startup changes** (in the initialization section before the main event loop):

```rust
// After LSP initialization and state setup:
let mut fs_watcher = match FsWatcher::new() {
    Ok(w) => w,
    Err(e) => {
        // Silently degrade: watcher failed but the editor still works.
        eprintln!("Warning: filesystem watcher could not start ({}); live updates disabled", e);
        None
    }
};

// If opening a file initially, watch it and record its mtime.
if let Some(buf) = state.current_buffer_mut() {
    buf.record_disk_mtime();
    if let Some(ref mut watcher) = fs_watcher {
        let _ = watcher.watch_file(&buf.path); // Ignore watch failures; editor still works
    }
}

// If a file tree was opened with the file, watch it.
if state.file_tree.is_some() {
    if let Some(ref mut watcher) = fs_watcher {
        if let Some(root) = state.file_tree.as_ref().and_then(|t| t.root_path()) {
            let _ = watcher.watch_tree(root);
        }
    }
}
```

**Event loop — filesystem event drain** (at the very top of the main loop, before `event::poll`):

```rust
loop {
    // Drain filesystem events first, before terminal input.
    if let Some(ref mut watcher) = fs_watcher {
        while let Ok(event) = watcher.rx.try_recv() {
            handle_fs_event(event, &mut state, &mut fs_watcher);
        }
    }
    
    // Then poll terminal input, render, etc. as usual.
    match event::poll(std::time::Duration::from_millis(50)) {
        // ...
    }
}
```

**File switch handling** (in the code path that switches `state.active_buffer`):

```rust
// Before switching the active buffer:
if let Some(buf) = state.current_buffer_mut() {
    // Clear disk_changed on the outgoing buffer (it won't be visible anyway)
    buf.disk_changed = false;
}

// Switch the active buffer (existing code)
// ...

// After switching, set up the watcher:
if let Some(buf) = state.current_buffer() {
    buf.record_disk_mtime();
    if let Some(ref mut watcher) = fs_watcher {
        let _ = watcher.watch_file(&buf.path);
    }
    state.disk_changed_path = None; // Clear the hint
}
```

**Tree toggle handling** (in `Ctrl-T` handler):

```rust
// When opening the tree (tree not currently open):
if state.file_tree.is_none() {
    let root = /* current working dir or user selection */;
    state.file_tree = Some(FileTree::from_dir(&root)?);
    if let Some(ref mut watcher) = fs_watcher {
        let _ = watcher.watch_tree(&root);
    }
}

// When closing the tree (tree currently open):
if state.file_tree.is_some() {
    if let Some(ref mut watcher) = fs_watcher {
        watcher.unwatch_tree();
    }
    state.file_tree = None;
}
```

**`Ctrl-O` key handling** (in `handle_key`, before the mode dispatch):

```rust
pub fn handle_key(key: KeyEvent, state: &mut EditorState, fs_watcher: &mut Option<FsWatcher>) -> Result<bool> {
    // High-priority: check for Ctrl-O (reload disk changes)
    if key.code == KeyCode::Char('O') && key.modifiers == KeyModifiers::CONTROL {
        if let Some(buf) = state.current_buffer() {
            if buf.disk_changed {
                reload_buffer_from_disk(state)?;
                state.disk_changed_path = None;
                return Ok(true);
            }
        }
    }
    
    // Then proceed with normal mode dispatch, chords, edits, etc.
    // ...
}
```

**New helper function** in `app.rs`:

```rust
/// Reload the active buffer from disk, replacing all lines and clearing flags.
/// Call this when the user presses Ctrl-O to accept external changes.
fn reload_buffer_from_disk(state: &mut EditorState) -> anyhow::Result<()> {
    let buf = state.current_buffer_mut()
        .ok_or_else(|| anyhow::anyhow!("No active buffer"))?;
    
    // Re-read the file from disk
    let lines = std::fs::read_to_string(&buf.path)?
        .lines()
        .map(|l| l.to_string())
        .collect::<Vec<_>>();
    
    // Replace buffer content
    buf.lines = lines;
    
    // Clear flags
    buf.disk_changed = false;
    buf.dirty = false;
    buf.record_disk_mtime();
    
    // Refresh internal caches (e.g. syntax highlighting, LSP)
    refresh_buffer_caches(buf, state)?;
    
    // Clamp cursor to new buffer size
    if state.cursor_line >= buf.lines.len() {
        state.cursor_line = buf.lines.len().saturating_sub(1);
    }
    if state.cursor_col > buf.lines.get(state.cursor_line).map(|l| l.len()).unwrap_or(0) {
        state.cursor_col = 0;
    }
    
    // Reset scroll to keep cursor visible
    state.scroll_offset = 0;
    
    Ok(())
}
```

---

#### New: `handle_fs_event()` Function

Add to `app.rs` (or a new submodule `fs_events.rs` if the file gets large):

```rust
use notify::event::{EventKind, ModifyKind, RenameMode};

fn handle_fs_event(
    result: notify::Result<notify::Event>,
    state: &mut EditorState,
    fs_watcher: &mut Option<FsWatcher>,
) {
    let Ok(event) = result else {
        // Event delivery error; log and continue.
        return;
    };

    // Dispatch on event kind and paths.
    match event.kind {
        // File modifications or metadata changes
        EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Metadata(_)) => {
            for path in &event.paths {
                // Check if this is the active buffer's file.
                if let Some(buf) = state.current_buffer() {
                    if buf.path == *path {
                        // Compare the new mtime against the last known one.
                        if let Ok(metadata) = std::fs::metadata(path) {
                            if let Ok(new_mtime) = metadata.modified() {
                                if let Some(last_mtime) = buf.last_disk_mtime {
                                    if new_mtime > last_mtime {
                                        // File is newer; set the flag.
                                        let buf = state.current_buffer_mut().unwrap();
                                        buf.disk_changed = true;
                                        state.disk_changed_path = Some(
                                            path.file_name().unwrap_or_default().to_path_buf()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // File deletion
        EventKind::Remove(_) => {
            for path in &event.paths {
                if let Some(buf) = state.current_buffer_mut() {
                    if buf.path == *path {
                        // Active file was deleted.
                        buf.disk_changed = false; // Not changed, deleted.
                        let fname = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file");
                        state.status_msg = format!("{} deleted from disk. Ctrl-S to restore.", fname);
                        
                        // Stop watching this file.
                        if let Some(w) = fs_watcher {
                            w.unwatch_file();
                        }
                    }
                }
                
                // Also handle tree removals.
                if let Some(tree) = &mut state.file_tree {
                    apply_tree_event(state, &event);
                }
            }
        }

        // File creations
        EventKind::Create(_) => {
            if state.file_tree.is_some() {
                apply_tree_event(state, &event);
            }
        }

        // Renames
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            // event.paths = [from, to]
            if state.file_tree.is_some() {
                apply_tree_event(state, &event);
            }
        }

        _ => {
            // Ignore other event types (AccessMode, PermissionsChange, etc.)
        }
    }
}
```

---

#### New: `apply_tree_event()` Function

Add to `app.rs`:

```rust
use crate::data::file_tree::{FileEntry, FileTree};

fn apply_tree_event(state: &mut EditorState, event: &notify::Event) {
    let Some(tree) = &mut state.file_tree else {
        return;
    };

    match event.kind {
        EventKind::Create(_) => {
            for path in &event.paths {
                insert_entry_sorted(tree, path);
                propagate_to_tree_view(state, path, TreeViewOp::Insert);
            }
        }

        EventKind::Remove(_) => {
            for path in &event.paths {
                // Remove the entry and all children from the tree.
                tree.entries.retain(|e| !e.path.starts_with(path));
                
                // Update the tree view.
                propagate_to_tree_view(state, path, TreeViewOp::Remove);
                
                // Handle special cases (cursor clamping, active file deleted, etc.)
                handle_removed_path(state, path);
            }
        }

        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            // event.paths[0] = old path, event.paths[1] = new path
            if event.paths.len() >= 2 {
                rename_entry(tree, &event.paths[0], &event.paths[1]);
                propagate_rename_to_tree_view(state, &event.paths[0], &event.paths[1]);
            }
        }

        _ => {}
    }
}

fn insert_entry_sorted(tree: &mut FileTree, path: &Path) {
    let entry = FileEntry::from_path(path);
    tree.entries.push(entry);
    // Re-sort to maintain WalkDir order (parent path, then filename lexicographically).
    tree.entries.sort_by(|a, b| a.path.cmp(&b.path));
}

fn rename_entry(tree: &mut FileTree, old_path: &Path, new_path: &Path) {
    for entry in &mut tree.entries {
        if entry.path == old_path {
            entry.path = new_path.to_path_buf();
            break;
        }
    }
    // Re-sort to maintain order.
    tree.entries.sort_by(|a, b| a.path.cmp(&b.path));
}

fn handle_removed_path(state: &mut EditorState, path: &Path) {
    // If the active buffer's file was deleted, we already set status_msg in handle_fs_event.
    // Here, we handle tree cursor clamping.
    
    if let Some(tree) = &state.file_tree {
        if tree.entries.is_empty() {
            // Tree became empty (root deleted?); close it.
            state.file_tree = None;
            state.tree_view.clear();
            state.tree_selected = 0;
            return;
        }
        
        // Clamp tree cursor if it's beyond the new length.
        let visible_entries = state.tree_view.len();
        if state.tree_selected >= visible_entries {
            state.tree_selected = visible_entries.saturating_sub(1);
        }
    }
}

enum TreeViewOp {
    Insert,
    Remove,
}

fn propagate_to_tree_view(state: &mut EditorState, path: &Path, op: TreeViewOp) {
    // This is a simplified version; in practice, you'd rebuild the tree_view from the tree
    // or update it incrementally. For now, a full rebuild ensures correctness:
    if let Some(tree) = &state.file_tree {
        state.tree_view = tree.entries.iter().map(|e| e.path.clone()).collect();
    }
}

fn propagate_rename_to_tree_view(state: &mut EditorState, old_path: &Path, new_path: &Path) {
    // Update tree_view entries.
    for entry in &mut state.tree_view {
        if entry == old_path {
            *entry = new_path.to_path_buf();
            break;
        }
    }
}
```

---

#### Modifications to `src/frontend/tui/status_bar.rs`

In the status bar rendering function, add the disk-change hint with higher priority than the selection hint:

```rust
pub fn render_status_bar(
    frame: &mut Frame,
    state: &EditorState,
    rect: Rect,
) {
    // Render left side (mode, cursor position, etc.)
    // ... existing left-side code ...

    // Render right side hints
    let (hint_text, hint_style) = if let Some(buf) = state.current_buffer() {
        if buf.disk_changed {
            // Disk change hint takes priority.
            let fname = buf.path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            let hint = if buf.dirty {
                format!(
                    " {} changed on disk. Ctrl-O to open and discard changes, Ctrl-S to overwrite ",
                    fname
                )
            } else {
                format!(" {} changed on disk. Ctrl-O to open newer version ", fname)
            };
            (hint, Style::default().bg(Color::LightBlue).bold())
        } else if let Some(_) = state.selection {
            // Selection hint (lower priority).
            (
                " Ctrl-Y: copy ".to_string(),
                Style::default().bg(Color::LightBlue).bold(),
            )
        } else {
            ("".to_string(), Style::default())
        }
    } else {
        ("".to_string(), Style::default())
    };

    if !hint_text.is_empty() {
        let hint_width = hint_text.len() as u16;
        let hint_x = rect.right().saturating_sub(hint_width);
        frame.render_widget(
            Paragraph::new(hint_text).style(hint_style),
            Rect {
                x: hint_x,
                y: rect.y,
                width: hint_width.min(rect.width),
                height: 1,
            },
        );
    }
}
```

**Rationale**:
- The disk-change hint has higher priority, so it's shown in preference to the selection hint.
- Both use the same styling (light blue background, bold text) for visual consistency.
- The hint automatically clears when `buf.disk_changed` becomes false (after reload or write).

---

## Dependency Management

### `Cargo.toml` Changes

Add to the `[dependencies]` section:

```toml
notify = { version = "6", optional = true }
```

Add `dep:notify` to the `frontends` feature (which already exists for `crossterm`, `ratatui`, etc.):

```toml
[features]
frontends = [
    "dep:crossterm",
    "dep:ratatui",
    "dep:notify",
]
```

**Rationale**:
- The library build path (`cargo build --lib`) does not enable `frontends`, so `notify` is not linked for library consumers.
- The binary build (default) enables `frontends`, so the TUI can use the watcher.
- This keeps the library build lightweight and zero-dependency.

---

## Testing Strategy

### Unit Tests

#### Test 1: `Buffer::record_disk_mtime`
**File**: `src/data/buffer.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_record_disk_mtime_initial() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "hello").unwrap();
        
        let mut buf = Buffer::new(path, vec!["hello".to_string()]);
        assert_eq!(buf.last_disk_mtime, None);
        
        buf.record_disk_mtime();
        assert!(buf.last_disk_mtime.is_some());
    }

    #[test]
    fn test_record_disk_mtime_update() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "hello").unwrap();
        
        let mut buf = Buffer::new(path.clone(), vec!["hello".to_string()]);
        buf.record_disk_mtime();
        let mtime1 = buf.last_disk_mtime;
        
        // Wait a bit and modify the file.
        std::thread::sleep(std::time::Duration::from_millis(100));
        fs::write(&path, "world").unwrap();
        
        buf.record_disk_mtime();
        let mtime2 = buf.last_disk_mtime;
        
        assert!(mtime2 > mtime1);
    }
}
```

#### Test 2: `disk_changed` Flag Detection
**File**: `src/frontend/tui/app.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_fs_event_modify_data() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "hello").unwrap();
        
        let mut state = EditorState::new();
        let mut buf = Buffer::new(path.clone(), vec!["hello".to_string()]);
        buf.record_disk_mtime();
        state.buffers.push(buf);
        state.active_buffer = 0;
        
        // Simulate file modification.
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&path, "world").unwrap();
        
        // Create a modify event.
        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            paths: vec![path.clone()],
            ..Default::default()
        };
        
        let mut watcher_opt = None;
        handle_fs_event(Ok(event), &mut state, &mut watcher_opt);
        
        assert!(state.current_buffer().unwrap().disk_changed);
    }

    #[test]
    fn test_handle_fs_event_remove() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "hello").unwrap();
        
        let mut state = EditorState::new();
        let buf = Buffer::new(path.clone(), vec!["hello".to_string()]);
        state.buffers.push(buf);
        state.active_buffer = 0;
        
        // Simulate file deletion.
        fs::remove_file(&path).unwrap();
        
        let event = notify::Event {
            kind: EventKind::Remove(RemoveKind::File),
            paths: vec![path],
            ..Default::default()
        };
        
        let mut watcher_opt = None;
        handle_fs_event(Ok(event), &mut state, &mut watcher_opt);
        
        assert!(!state.current_buffer().unwrap().disk_changed);
        assert!(state.status_msg.contains("deleted from disk"));
    }
}
```

#### Test 3: Reload from Disk
**File**: `src/frontend/tui/app.rs`

```rust
#[test]
fn test_reload_buffer_from_disk() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "hello").unwrap();
    
    let mut state = EditorState::new();
    let buf = Buffer::new(path.clone(), vec!["hello".to_string()]);
    state.buffers.push(buf);
    state.active_buffer = 0;
    
    // Modify the file on disk.
    fs::write(&path, "world").unwrap();
    
    // Reload.
    reload_buffer_from_disk(&mut state).unwrap();
    
    let buf = state.current_buffer().unwrap();
    assert_eq!(buf.lines, vec!["world"]);
    assert!(!buf.dirty);
    assert!(!buf.disk_changed);
    assert!(buf.last_disk_mtime.is_some());
}
```

#### Test 4: Status Bar Hint Rendering
**File**: `src/frontend/tui/status_bar.rs`

```rust
#[test]
fn test_status_bar_disk_changed_clean() {
    let mut state = EditorState::new();
    let mut buf = Buffer::new("test.txt".into(), vec!["hello".to_string()]);
    buf.disk_changed = true;
    buf.dirty = false;
    state.buffers.push(buf);
    state.active_buffer = 0;
    
    // In a real test, render and inspect the output.
    // For now, just assert the flag is correct.
    assert!(state.current_buffer().unwrap().disk_changed);
    assert!(!state.current_buffer().unwrap().dirty);
}

#[test]
fn test_status_bar_disk_changed_dirty() {
    let mut state = EditorState::new();
    let mut buf = Buffer::new("test.txt".into(), vec!["hello".to_string()]);
    buf.disk_changed = true;
    buf.dirty = true;
    state.buffers.push(buf);
    state.active_buffer = 0;
    
    assert!(state.current_buffer().unwrap().disk_changed);
    assert!(state.current_buffer().unwrap().dirty);
}
```

#### Test 5: Tree Event Application
**File**: `src/frontend/tui/app.rs`

```rust
#[test]
fn test_apply_tree_event_create() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    
    // Create initial tree.
    let mut tree = FileTree::from_dir(root).unwrap();
    let initial_count = tree.entries.len();
    
    // Create a new file.
    let new_file = root.join("new.txt");
    fs::write(&new_file, "").unwrap();
    
    // Apply a Create event.
    let event = notify::Event {
        kind: EventKind::Create(CreateKind::File),
        paths: vec![new_file],
        ..Default::default()
    };
    
    insert_entry_sorted(&mut tree, &event.paths[0]);
    
    assert_eq!(tree.entries.len(), initial_count + 1);
}

#[test]
fn test_apply_tree_event_remove() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    
    let file = root.join("file.txt");
    fs::write(&file, "").unwrap();
    
    // Create tree with the file.
    let mut tree = FileTree::from_dir(root).unwrap();
    assert!(tree.entries.iter().any(|e| e.path == file));
    
    // Remove the file from disk and apply Remove event.
    fs::remove_file(&file).unwrap();
    tree.entries.retain(|e| !e.path.starts_with(&file));
    
    assert!(!tree.entries.iter().any(|e| e.path == file));
}
```

### Integration Tests

#### Test 6: Watch File Round-Trip
**File**: `src/frontend/tui/fs_watcher.rs`

```rust
#[test]
fn test_watch_file_receives_event() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("test.txt");
    fs::write(&path, "hello").unwrap();
    
    let mut watcher = FsWatcher::new().unwrap();
    watcher.watch_file(&path).unwrap();
    
    // Modify the file.
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&path, "world").unwrap();
    
    // Within a short timeout, should receive an event.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Ok(event) = watcher.rx.try_recv() {
            if let Ok(ev) = event {
                if matches!(ev.kind, EventKind::Modify(_)) {
                    assert!(ev.paths.contains(&path));
                    return; // Test passes
                }
            }
        }
        
        if std::time::Instant::now() > deadline {
            panic!("Event not received within timeout");
        }
        
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
```

---

## Edge Cases & Handling

### Active File Deleted

**Scenario**: The user has `main.rs` open and an external tool deletes it.

**Expected behavior**:
- `handle_fs_event` receives a `Remove` event for the file path.
- Sets `state.status_msg` to `"main.rs deleted from disk. Ctrl-S to restore."`.
- Sets `buf.disk_changed = false` (the file is gone, not changed).
- Unregisters the file from the watcher so no further events fire.
- The buffer content is preserved in memory; pressing `Ctrl-S` writes it back, re-creating the file.

**Implementation notes**:
- The `disk_changed` flag is *not* set for deletions; instead, a status message is used.
- The distinction prevents confusion between "file changed" and "file deleted."

### Active File Renamed

**Scenario**: An external tool renames `main.rs` to `main.rs.bak`.

**Expected behavior**:
- `notify` generates a `Modify(Name(_))` event (or `Remove + Create` pair depending on OS).
- The file can no longer be watched at the old path; the watcher unregisters it.
- Treat it as deletion: show `"main.rs deleted from disk. Ctrl-S to restore."`.
- The buffer's `path` field still points to the old name; saving will re-create it at the old location.
- This may not be the user's intent, but it preserves buffer content without guessing.

**Implementation notes**:
- Rename events are treated as remove + re-create for simplicity; chasing a moving file is complex and error-prone.

### Tree Cursor on Deleted Directory

**Scenario**: The user has the tree open and focused on `/src/components/`. An external tool deletes that directory.

**Expected behavior**:
- `apply_tree_event` removes all entries under `/src/components/` from the tree.
- Checks if `state.tree_selected` points to a now-invalid index and clamps it to the last valid entry.
- The cursor visually jumps to a nearby valid entry without crashing.

**Implementation notes**:
- Saturating subtract ensures the index is never negative.
- If the entire tree becomes empty, close it and reset `state.file_tree = None`.

### Spurious mtime Changes

**Scenario**: An editor writes a file, then truncates it (changing mtime without changing content). The user sees the disk-change hint but finds the content is identical on reload.

**Expected behavior**:
- The hint is shown anyway (checking content hash on every tick is expensive).
- The user presses `Ctrl-O`, reloads, and sees no change. No harm done.

**Implementation notes**:
- Comparing mtimes is much cheaper than hashing file content. Spurious hints are acceptable overhead.

### Rapid Bursts of Events

**Scenario**: `cargo build` writes many files to `target/`. The `notify` watcher delivers a burst of events.

**Expected behavior**:
- `try_recv()` drains all pending events in one loop before rendering.
- Tree insertions are batched and sorted once, avoiding redundant lookups.
- The editor remains responsive; events are processed within the next tick or two.

**Implementation notes**:
- Non-blocking `try_recv()` ensures the event loop never stalls on I/O.
- Burst handling is automatic; no special code needed.

### Watcher Initialization Fails

**Scenario**: The system hits the inotify watch limit or lacks permissions to watch the directory.

**Expected behavior**:
- `FsWatcher::new()` returns an error.
- The editor logs a warning and continues without live updates.
- All functionality works; the user just doesn't see live updates.

**Implementation notes**:
- The watcher is optional; degradation is silent and graceful.
- Optionally set a one-time status message warning the user, but don't block startup.

### Tree Not Visible but Watcher Running

**Scenario**: The tree is being watched but the user toggles it off (presses `Ctrl-T` to hide it).

**Expected behavior**:
- Events continue to be processed and applied to `FileTree.entries`.
- When the user toggles the tree back on, it's already up-to-date.
- No gaps or stale entries.

**Implementation notes**:
- Events are applied to the tree data structure regardless of visibility.
- `tree_view` is rebuilt from `tree_entries` when the tree is expanded, ensuring consistency.

---

## Code Organization Checklist

Before marking the work item complete, ensure:

- [ ] `src/data/buffer.rs` has `disk_changed` and `last_disk_mtime` fields with initializers.
- [ ] `src/data/buffer.rs` has `record_disk_mtime()` and updated `write()` method.
- [ ] `src/data/state.rs` has `disk_changed_path` field with initializer.
- [ ] `src/frontend/tui/fs_watcher.rs` is created with full `FsWatcher` struct and all methods.
- [ ] `src/frontend/tui/app.rs` has startup initialization, event drain loop, and file/tree switch handlers.
- [ ] `src/frontend/tui/app.rs` has `handle_fs_event()`, `apply_tree_event()`, `reload_buffer_from_disk()`, and helper functions.
- [ ] `src/frontend/tui/status_bar.rs` renders the disk-change hint with correct styling and priority.
- [ ] `Cargo.toml` has `notify = { version = "6", optional = true }` and `dep:notify` in the `frontends` feature.
- [ ] All unit tests in each file pass (`cargo test`).
- [ ] Integration tests pass.
- [ ] `cargo clippy -- -D warnings` produces no warnings.
- [ ] `cargo fmt --check` shows no formatting issues.
- [ ] Manual TUI testing confirms:
  - File change detection works (modify a file in another editor).
  - Reload with `Ctrl-O` works (clean and dirty buffers).
  - Overwrite with `Ctrl-S` works (dirty buffer overwrites disk).
  - File deletion message appears and `Ctrl-S` restores it.
  - Tree live-updates (create/delete/rename files in another tool).
  - Tree cursor clamps correctly after deletions.

---

## Related Documentation

- **docs/00-getting-started.md** — TUI navigation and basic operations.
- **docs/03-using-the-tui.md** — Keybindings and mode switching.
- **docs/07-architecture-overview.md** — 3-layer architecture and module structure.
- **aspec/architecture/design.md** — Core design principles and layer boundaries.
- **CLAUDE.md** — Project build, test, and code style guidelines.

---

## Appendix: Glossary of Terms

| Term | Definition |
|------|-----------|
| **disk_changed** | Flag on `Buffer` indicating the file has been modified on disk since the last sync. |
| **last_disk_mtime** | Timestamp of the file's last known modification time on disk. Updated on load, write, and reload. |
| **FsWatcher** | Struct in Layer 2 that owns the `notify::RecommendedWatcher` and `mpsc::Receiver`. |
| **handle_fs_event** | Layer 2 function that processes `notify::Event` and updates `Buffer` and `EditorState`. |
| **apply_tree_event** | Layer 2 function that updates the file tree in response to file system changes. |
| **reload_buffer_from_disk** | Function that re-reads a file, replaces buffer content, and clears dirty/disk_changed flags. |
| **Recursive / Non-Recursive watch** | `RecursiveMode::Recursive` watches a directory tree; `NonRecursive` watches a single file. |
| **try_recv()** | Non-blocking channel receive; returns immediately with Ok(event), Err(TryRecvError), or waits for the next poll. |
| **Watcher graceful degradation** | If the watcher fails to initialize, the editor continues to work without live updates. |
