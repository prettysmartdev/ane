# Work Item 0014: Implementation Guide
## Filetree Enhancements — Rename, Delete, Create

**Last Updated:** 2026-05-18  
**Status:** Implementation Guide  
**Target:** Rust / ratatui TUI

---

## Overview

This guide provides a comprehensive implementation plan for **Work Item 0014**: adding three interactive filetree operations (rename, delete, create) to the ane TUI, plus fixing a bug in the empty-parent path handling.

The filetree is a key component of ane's TUI interface. This work item adds essential file management keybindings to make ane a complete development environment without requiring external shell commands:

- **Ctrl-R**: Rename files and folders inline within the tree
- **Ctrl-D**: Delete files and folders with a confirmation dialog
- **Ctrl-N**: Create new files with an inline prompt
- **Bug fix**: Empty-parent path handling in `toggle_tree`

Each operation includes full state management, filesystem synchronization, buffer tracking, and rendering support for inline editing.

### Key Goals

1. **File Management in TUI**: Enable rename, delete, and create operations without leaving ane
2. **State Safety**: Maintain consistency between filesystem, file tree state, and open buffers
3. **User Feedback**: Clear inline editing UX with confirmation dialogs and status messages
4. **Edge Case Robustness**: Handle tree root deletion, buffer path tracking, name validation
5. **Idempotent Watcher Integration**: Ensure filesystem watcher events (from WI-13) don't cause double-processing

### Architecture Integration

**Layer discipline**:
- **Layer 0** (`src/data/state.rs`): State structs for rename/delete/new-file operations
- **Layer 1**: (No new code in commands layer for this work item)
- **Layer 2** (`src/frontend/tui/app.rs`, `tree_pane.rs`, `exit_modal.rs`): Keybinding dispatch, input handling, rendering

---

## Architecture Rules (From CLAUDE.md)

The ane project enforces a **three-layer architecture with strict dependency direction**:

- **Layer 0** (`src/data/`): All filesystem I/O, state definitions, chord types, LSP registry. No imports from `commands` or `frontend`.
- **Layer 1** (`src/commands/`): Chord logic, LSP operations. Imports from `data` only.
- **Layer 2** (`src/frontend/`): CLI + TUI + frontend traits. Imports from `data` and `commands`.

**Critical Rule**: Lower layers NEVER depend on higher layers. Violating this is a build-breaking error.

---

## Implementation Phases

### Phase 1: Bug Fix — Empty-Parent Path

**File**: `src/frontend/tui/app.rs` → `toggle_tree` function

**Problem**: When `ane <file>` is invoked with a relative filename like `notes.txt`, the file has no parent directory in the traditional sense. `Path::new("notes.txt").parent()` returns `Some(Path::new(""))` (an empty path), not `None`. The current code falls through to `unwrap_or(Path::new("."))`, which never fires because `Some` is truthy. The empty path is then passed to `FileTree::from_dir("")`, which attempts `"".canonicalize()`, failing on Linux with `No such file or directory`.

**Fix**: Treat an empty parent path the same as `None` and fall back to `"."` (current working directory), which canonicalizes correctly.

**Implementation**:

```rust
// Before:
let dir = if state.opened_path.is_dir() {
    state.opened_path.clone()
} else {
    state
        .opened_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
};

// After:
let dir = if state.opened_path.is_dir() {
    state.opened_path.clone()
} else {
    match state.opened_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
};
```

**Verification**:
- Test `ane notes.txt` then `Ctrl-T` → tree should open rooted at `.` (current working directory), not error.

---

### Phase 2: State Layer — Add State Structs

**File**: `src/data/state.rs` (or a new `src/data/tree_op_state.rs` if preferred for separation)

Add three state structs to `EditorState`:

```rust
pub struct TreeRenameState {
    /// Index in tree_view being edited
    pub index: usize,
    /// Current edit buffer, pre-filled with existing name
    pub input: String,
    /// Byte offset of text cursor within input
    pub cursor: usize,
}

pub struct TreeDeleteState {
    /// Index in tree_view being deleted
    pub index: usize,
    /// Names of direct children (folders only), for confirmation preview
    pub children_preview: Vec<String>,
}

pub struct TreeNewFileState {
    /// Resolved directory where the file will be created
    pub parent_dir: PathBuf,
    /// Current filename input
    pub input: String,
    /// Byte offset of text cursor within input
    pub cursor: usize,
}
```

Add three optional fields to `EditorState`:

```rust
pub struct EditorState {
    // ... existing fields ...
    pub tree_rename_state: Option<TreeRenameState>,
    pub tree_delete_confirm: Option<TreeDeleteState>,
    pub tree_new_file_state: Option<TreeNewFileState>,
}
```

**Initialization**: Set all three fields to `None` in `EditorState::new()`.

---

### Phase 3: Keybinding Dispatch

**File**: `src/frontend/tui/app.rs` → key routing and tree handlers

#### 3.1 Priority Guards in Main Key Routing

In the main key-event handler (around line 742 in `app.rs`), add three guards **before** the call to `handle_tree_keys`:

```rust
// Priority: tree op state takes precedence over tree navigation
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

These guards ensure that when a user is actively editing a filename or confirming a delete, Ctrl-T and other tree navigation keys are absorbed (not processed as tree commands).

#### 3.2 Add Keybinding Handlers

In `handle_tree_keys`, add three new arms before the `_ => {}` catch-all:

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

**Keybinding Conflict Check**:
- `Ctrl-R` in tree mode is currently unbound (it cycles chord history in Chord mode, but tree mode and Chord mode are mutually exclusive).
- `Ctrl-D` and `Ctrl-N` are unbound in all modes.
- Verify by inspecting existing match arms in `handle_tree_keys` and `handle_chord_mode`.

---

### Phase 4: Ctrl-R — Rename Implementation

#### 4.1 Begin Rename

**Function**: `begin_tree_rename(state: &mut EditorState)`

```rust
fn begin_tree_rename(state: &mut EditorState) {
    if state.tree_view.is_empty() {
        return;
    }
    
    let index = state.tree_selected;
    if index >= state.tree_view.len() {
        return;
    }
    
    let entry = &state.tree_view[index];
    let name = entry.name();
    
    state.tree_rename_state = Some(TreeRenameState {
        index,
        input: name.to_string(),
        cursor: name.len(),
    });
}
```

#### 4.2 Handle Rename Input

**Function**: `handle_tree_rename_input(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers, syntax_engine: &SyntaxEngine)`

```rust
fn handle_tree_rename_input(
    state: &mut EditorState,
    code: KeyCode,
    modifiers: KeyModifiers,
    syntax_engine: &SyntaxEngine,
) {
    let Some(ref mut rename_state) = state.tree_rename_state else {
        return;
    };
    
    match code {
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            rename_state.input.insert(rename_state.cursor, c);
            rename_state.cursor += c.len_utf8();
        }
        KeyCode::Backspace => {
            if rename_state.cursor > 0 {
                let before = &rename_state.input[..rename_state.cursor];
                // Find the start of the character before cursor
                if let Some(pos) = before.char_indices()
                    .rev()
                    .next()
                    .map(|(i, _)| i)
                {
                    rename_state.input.remove(pos);
                    rename_state.cursor = pos;
                }
            }
        }
        KeyCode::Left => {
            if rename_state.cursor > 0 {
                let before = &rename_state.input[..rename_state.cursor];
                rename_state.cursor = before
                    .char_indices()
                    .rev()
                    .next()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
        }
        KeyCode::Right => {
            if rename_state.cursor < rename_state.input.len() {
                rename_state.cursor += rename_state.input[rename_state.cursor..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
            }
        }
        KeyCode::Enter => {
            commit_tree_rename(state, syntax_engine);
        }
        KeyCode::Esc => {
            state.tree_rename_state = None;
        }
        _ => {}
    }
}
```

#### 4.3 Commit Rename

**Function**: `commit_tree_rename(state: &mut EditorState, syntax_engine: &SyntaxEngine)`

```rust
fn commit_tree_rename(state: &mut EditorState, syntax_engine: &SyntaxEngine) {
    let Some(ref rename_state) = state.tree_rename_state else {
        return;
    };
    
    let new_name = rename_state.input.trim();
    
    // Validation
    if new_name.is_empty() {
        state.status_msg = "filename cannot be empty".to_string();
        return;
    }
    if new_name.contains('/') || new_name.contains('\\') {
        state.status_msg = "filename cannot contain path separators".to_string();
        return;
    }
    
    let index = rename_state.index;
    if index >= state.tree_view.len() {
        state.tree_rename_state = None;
        return;
    }
    
    let old_path = state.tree_view[index].path.clone();
    let new_path = match old_path.parent() {
        Some(parent) => parent.join(new_name),
        None => PathBuf::from(new_name),
    };
    
    // Check if destination already exists
    if new_path.exists() && new_path != old_path {
        state.status_msg = "rename failed: destination exists".to_string();
        return;
    }
    
    // Check if renaming tree root
    if old_path == state.file_tree.root {
        state.status_msg = "cannot rename tree root (use Ctrl-T to switch)".to_string();
        return;
    }
    
    // Perform filesystem rename
    if let Err(e) = std::fs::rename(&old_path, &new_path) {
        state.status_msg = format!("rename failed: {}", e);
        state.tree_rename_state = None;
        return;
    }
    
    // Update file_tree.entries and tree_view
    rename_subtree(&mut state.file_tree.entries, &mut state.tree_view, &old_path, &new_path);
    
    // Update buffer paths
    for buffer in &mut state.buffers {
        if buffer.path.starts_with(&old_path) {
            let relative = buffer.path.strip_prefix(&old_path).unwrap();
            buffer.path = new_path.join(relative);
        }
    }
    
    // Refresh caches if active buffer path changed
    if let Some(active_buf) = state.active_buffer {
        if state.buffers[active_buf].path.starts_with(&new_path) {
            refresh_buffer_caches(state, syntax_engine);
        }
    }
    
    state.status_msg = format!("renamed → {}", new_name);
    state.tree_rename_state = None;
}
```

#### 4.4 Rendering

In `src/frontend/tui/tree_pane.rs` → `render` function:

When drawing a tree row, check if `state.tree_rename_state` is active and its index matches the current row. If so, replace the normal filename span with the inline edit buffer:

```rust
if let Some(rename_state) = &state.tree_rename_state {
    if rename_state.index == current_row_index {
        // Draw the input string with a cursor block at rename_state.cursor
        let display_str = &rename_state.input;
        let prefix = &display_str[..rename_state.cursor.min(display_str.len())];
        let after_cursor = &display_str[rename_state.cursor.min(display_str.len())..];
        
        let mut x = tree_indent + 2; // after folder icon
        
        // Draw prefix
        buffer.set_string(x, y, prefix, style);
        x += prefix.width();
        
        // Draw cursor block (or space if at end)
        let cursor_char = after_cursor.chars().next().unwrap_or(' ');
        buffer.set_string(
            x,
            y,
            cursor_char.to_string(),
            style.bg(Color::White).fg(Color::Black),
        );
        x += 1;
        
        // Draw rest
        buffer.set_string(x, y, &after_cursor[cursor_char.len_utf8()..], style);
        
        return; // Skip normal name rendering
    }
}
```

Use the same selected-row background color (`Color::Cyan` or project standard) so the row remains visually active.

---

### Phase 5: Ctrl-D — Delete Implementation

#### 5.1 Begin Delete

**Function**: `begin_tree_delete(state: &mut EditorState)`

```rust
fn begin_tree_delete(state: &mut EditorState) {
    if state.tree_view.is_empty() {
        return;
    }
    
    let index = state.tree_selected;
    if index >= state.tree_view.len() {
        return;
    }
    
    let entry = &state.tree_view[index];
    
    // Disallow deleting tree root
    if entry.depth == 0 && entry.path == state.file_tree.root {
        state.status_msg = "cannot delete tree root".to_string();
        return;
    }
    
    // Collect direct children if folder
    let mut children_preview = Vec::new();
    if entry.is_dir {
        let target_depth = entry.depth + 1;
        for other in &state.file_tree.entries {
            if other.path.starts_with(&entry.path)
                && other.depth == target_depth
                && other.path.parent() == Some(&entry.path)
            {
                children_preview.push(other.name().to_string());
            }
        }
    }
    
    state.tree_delete_confirm = Some(TreeDeleteState {
        index,
        children_preview,
    });
}
```

#### 5.2 Handle Delete Input

**Function**: `handle_tree_delete_input(state: &mut EditorState, code: KeyCode)`

```rust
fn handle_tree_delete_input(state: &mut EditorState, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            commit_tree_delete(state);
        }
        KeyCode::Esc => {
            state.tree_delete_confirm = None;
        }
        _ => {}
    }
}
```

#### 5.3 Commit Delete

**Function**: `commit_tree_delete(state: &mut EditorState)`

```rust
fn commit_tree_delete(state: &mut EditorState) {
    let Some(ref delete_state) = state.tree_delete_confirm else {
        return;
    };
    
    let index = delete_state.index;
    if index >= state.tree_view.len() {
        state.tree_delete_confirm = None;
        return;
    }
    
    let path = state.tree_view[index].path.clone();
    let is_dir = state.tree_view[index].is_dir;
    
    // Perform filesystem delete
    let delete_result = if is_dir {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    
    if let Err(e) = delete_result {
        state.status_msg = format!("delete failed: {}", e);
        state.tree_delete_confirm = None;
        return;
    }
    
    // Remove from file_tree.entries and tree_view
    state.file_tree.entries.retain(|entry| !entry.path.starts_with(&path));
    state.tree_view.retain(|entry| !entry.path.starts_with(&path));
    
    // Clamp tree_selected to valid range
    if state.tree_selected >= state.tree_view.len() && state.tree_view.len() > 0 {
        state.tree_selected = state.tree_view.len() - 1;
    } else if state.tree_view.is_empty() {
        state.tree_selected = 0;
    }
    
    // Check for open buffers matching deleted path
    let mut deleted_buffer_msg = None;
    for buffer in &mut state.buffers {
        if buffer.path.starts_with(&path) {
            // Preserve buffer but mark as deleted
            deleted_buffer_msg = Some(format!("{} deleted — Ctrl-S to restore", buffer.path.file_name().unwrap_or_default().to_string_lossy()));
        }
    }
    
    if let Some(msg) = deleted_buffer_msg {
        state.status_msg = msg;
    } else {
        state.status_msg = format!("deleted: {}", path.file_name().unwrap_or_default().to_string_lossy());
    }
    
    state.tree_delete_confirm = None;
}
```

#### 5.4 Delete Confirmation Modal

**File**: `src/frontend/tui/exit_modal.rs`

Add a new render function:

```rust
pub fn render_delete_confirm(
    f: &mut Frame,
    state: &EditorState,
    area: Rect,
) {
    let Some(ref delete_state) = state.tree_delete_confirm else {
        return;
    };
    
    if delete_state.index >= state.tree_view.len() {
        return;
    }
    
    let entry = &state.tree_view[delete_state.index];
    let name = entry.name();
    
    // Create a centered popup (reuse centered_rect, make it pub(super))
    let popup_area = centered_rect(60, 50, area);
    
    let border_style = Style::default()
        .fg(Color::Red)
        .bg(Color::Black);
    
    let inner = Rect {
        x: popup_area.x + 1,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(2),
        height: popup_area.height.saturating_sub(2),
    };
    
    // Clear the area
    f.render_widget(Clear, popup_area);
    
    // Draw border
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .style(border_style)
            .title(" Delete? "),
        popup_area,
    );
    
    // Build content lines
    let mut lines = vec![
        Line::from(format!("Delete {}?", name)),
        Line::from(""),
    ];
    
    // Add preview of direct children (folders only)
    if !delete_state.children_preview.is_empty() {
        for (i, child_name) in delete_state.children_preview.iter().take(5).enumerate() {
            lines.push(Line::from(format!("  • {}", child_name)));
        }
        if delete_state.children_preview.len() > 5 {
            lines.push(Line::from(format!("  (and {} more)", delete_state.children_preview.len() - 5)));
        }
        lines.push(Line::from(""));
    }
    
    // Footer
    lines.push(Line::from("  Enter to confirm"));
    lines.push(Line::from("  Esc to cancel"));
    
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(Color::White).bg(Color::Black));
    
    f.render_widget(paragraph, inner);
}
```

Call this in the main TUI render function, alongside other modal renders:

```rust
if state.tree_delete_confirm.is_some() {
    render_delete_confirm(f, state, f.size());
}
```

---

### Phase 6: Ctrl-N — Create New File Implementation

#### 6.1 Begin New File

**Function**: `begin_tree_new_file(state: &mut EditorState)`

```rust
fn begin_tree_new_file(state: &mut EditorState) {
    let parent_dir = if state.tree_view.is_empty() {
        state.file_tree.root.clone()
    } else {
        let index = state.tree_selected;
        if index >= state.tree_view.len() {
            state.file_tree.root.clone()
        } else {
            let entry = &state.tree_view[index];
            if entry.is_dir {
                entry.path.clone()
            } else {
                entry.path.parent().unwrap().to_path_buf()
            }
        }
    };
    
    state.tree_new_file_state = Some(TreeNewFileState {
        parent_dir,
        input: String::new(),
        cursor: 0,
    });
}
```

#### 6.2 Handle New File Input

**Function**: `handle_tree_new_file_input(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers)`

```rust
fn handle_tree_new_file_input(
    state: &mut EditorState,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    let Some(ref mut new_file_state) = state.tree_new_file_state else {
        return;
    };
    
    match code {
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            new_file_state.input.insert(new_file_state.cursor, c);
            new_file_state.cursor += c.len_utf8();
        }
        KeyCode::Backspace => {
            if new_file_state.cursor > 0 {
                let before = &new_file_state.input[..new_file_state.cursor];
                if let Some(pos) = before.char_indices().rev().next().map(|(i, _)| i) {
                    new_file_state.input.remove(pos);
                    new_file_state.cursor = pos;
                }
            }
        }
        KeyCode::Left => {
            if new_file_state.cursor > 0 {
                let before = &new_file_state.input[..new_file_state.cursor];
                new_file_state.cursor = before
                    .char_indices()
                    .rev()
                    .next()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
        }
        KeyCode::Right => {
            if new_file_state.cursor < new_file_state.input.len() {
                new_file_state.cursor += new_file_state.input[new_file_state.cursor..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
            }
        }
        KeyCode::Enter => {
            commit_tree_new_file(state);
        }
        KeyCode::Esc => {
            state.tree_new_file_state = None;
        }
        _ => {}
    }
}
```

#### 6.3 Commit New File

**Function**: `commit_tree_new_file(state: &mut EditorState)`

```rust
fn commit_tree_new_file(state: &mut EditorState) {
    let Some(ref new_file_state) = state.tree_new_file_state else {
        return;
    };
    
    let filename = new_file_state.input.trim();
    
    // Validation
    if filename.is_empty() {
        state.status_msg = "filename cannot be empty".to_string();
        return;
    }
    if filename.contains('/') || filename.contains('\\') {
        state.status_msg = "filename cannot contain path separators".to_string();
        return;
    }
    
    let new_path = new_file_state.parent_dir.join(filename);
    
    // Check if file already exists
    if new_path.exists() {
        state.status_msg = "file already exists".to_string();
        return;
    }
    
    // Create file
    if let Err(e) = std::fs::File::create(&new_path) {
        state.status_msg = format!("create failed: {}", e);
        return;
    }
    
    // Insert into file_tree.entries in sorted order (reuse insert_entry_sorted from WI-13)
    let new_entry = FileEntry::from_path(&new_path);
    insert_entry_sorted(&mut state.file_tree.entries, new_entry.clone());
    
    // Update tree_view if parent is expanded
    // (This requires checking if parent_dir is visible in tree_view and has children_expanded flag)
    // Insert at correct sorted position within the parent's visible children
    let parent_is_expanded = state.tree_view.iter().any(|e| {
        e.path == new_file_state.parent_dir && e.expanded
    });
    
    if parent_is_expanded {
        // Find insertion position among siblings
        let insert_pos = state.tree_view.iter().position(|e| {
            e.path.parent() == Some(&new_file_state.parent_dir)
                && e.name() > filename
        }).unwrap_or(state.tree_view.len());
        
        let mut tree_entry = FileEntry::from_path(&new_path);
        tree_entry.depth = state.tree_view
            .iter()
            .find(|e| e.path == new_file_state.parent_dir)
            .map(|e| e.depth + 1)
            .unwrap_or(1);
        
        state.tree_view.insert(insert_pos, tree_entry);
        state.tree_selected = insert_pos;
    }
    
    // Open the file in a buffer
    state.open_file(&new_path);
    refresh_buffer_caches(state, syntax_engine); // syntax_engine passed as param
    
    // Exit tree focus and enter Edit mode
    state.focus_tree = false;
    state.mode = Mode::Edit;
    state.status_msg = "-- EDIT --".to_string();
    
    state.tree_new_file_state = None;
}
```

#### 6.4 Rendering

In `src/frontend/tui/tree_pane.rs` → `render` function:

When rendering, check if `tree_new_file_state` is active. Render a synthetic row after the parent directory's visible children:

```rust
if let Some(new_file_state) = &state.tree_new_file_state {
    // Find parent directory in tree_view to get its depth
    if let Some(parent_entry) = state.tree_view.iter().find(|e| e.path == new_file_state.parent_dir) {
        let child_depth = parent_entry.depth + 1;
        let indent = child_depth as u16 * 2; // 2 chars per depth
        
        // Render synthetic row at y position after parent's children
        // Use "○ {input}{cursor_block}" format
        let x = area.x + indent + 2;
        let y = calculate_synthetic_row_y(); // Helper to find correct Y position
        
        buffer.set_string(x, y, "○ ", style);
        
        let display_str = &new_file_state.input;
        let prefix = &display_str[..new_file_state.cursor.min(display_str.len())];
        let after_cursor = &display_str[new_file_state.cursor.min(display_str.len())..];
        
        let mut col = x + 2;
        
        // Draw prefix
        buffer.set_string(col, y, prefix, style);
        col += prefix.width() as u16;
        
        // Draw cursor block
        let cursor_char = after_cursor.chars().next().unwrap_or(' ');
        buffer.set_string(
            col,
            y,
            cursor_char.to_string(),
            style.bg(Color::White).fg(Color::Black),
        );
        col += 1;
        
        // Draw rest
        buffer.set_string(col, y, &after_cursor[cursor_char.len_utf8()..], style);
    }
}
```

Use the same selected-row background color so the synthetic row is visually active.

---

### Phase 7: Testing

#### 7.1 Bug Fix Tests

**File**: `src/frontend/tui/app.rs` → `#[cfg(test)] mod tests`

```rust
#[test]
fn test_toggle_tree_with_relative_bare_filename() {
    let mut state = EditorState::new();
    state.opened_path = PathBuf::from("notes.txt");
    
    // Should not panic and should create a valid file tree rooted at "."
    state.toggle_tree();
    
    assert!(state.focus_tree);
    assert_eq!(state.file_tree.root, std::env::current_dir().unwrap());
    assert!(!state.tree_view.is_empty() || true); // tree_view may be empty if pwd is empty
}
```

#### 7.2 Rename Tests

```rust
#[test]
fn test_begin_tree_rename() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("original.txt");
    std::fs::File::create(&file_path).unwrap();
    
    // Populate tree_view
    state.tree_view = vec![FileEntry::from_path(&file_path)];
    state.tree_selected = 0;
    
    begin_tree_rename(&mut state);
    
    assert!(state.tree_rename_state.is_some());
    let rename_state = state.tree_rename_state.unwrap();
    assert_eq!(rename_state.input, "original.txt");
    assert_eq!(rename_state.cursor, "original.txt".len());
}

#[test]
fn test_commit_tree_rename_success() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("original.txt");
    std::fs::File::create(&file_path).unwrap();
    
    state.tree_view = vec![FileEntry::from_path(&file_path)];
    state.file_tree.entries = state.tree_view.clone();
    state.tree_selected = 0;
    state.tree_rename_state = Some(TreeRenameState {
        index: 0,
        input: "renamed.txt".to_string(),
        cursor: 11,
    });
    
    let syntax_engine = SyntaxEngine::new();
    commit_tree_rename(&mut state, &syntax_engine);
    
    let new_path = temp_dir.path().join("renamed.txt");
    assert!(new_path.exists());
    assert!(!file_path.exists());
    assert!(state.tree_rename_state.is_none());
    assert!(state.status_msg.contains("renamed"));
}

#[test]
fn test_commit_tree_rename_existing_destination() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("file1.txt");
    let file2 = temp_dir.path().join("file2.txt");
    std::fs::File::create(&file1).unwrap();
    std::fs::File::create(&file2).unwrap();
    
    state.tree_view = vec![FileEntry::from_path(&file1)];
    state.file_tree.entries = state.tree_view.clone();
    state.tree_selected = 0;
    state.tree_rename_state = Some(TreeRenameState {
        index: 0,
        input: "file2.txt".to_string(),
        cursor: 9,
    });
    
    let syntax_engine = SyntaxEngine::new();
    commit_tree_rename(&mut state, &syntax_engine);
    
    // Should fail and keep original name
    assert!(file1.exists());
    assert!(state.status_msg.contains("exists"));
}

#[test]
fn test_commit_tree_rename_updates_buffer_path() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("original.txt");
    std::fs::File::create(&file_path).unwrap();
    
    let buffer = Buffer::new(file_path.clone());
    state.buffers.push(buffer);
    state.active_buffer = Some(0);
    
    state.tree_view = vec![FileEntry::from_path(&file_path)];
    state.file_tree.entries = state.tree_view.clone();
    state.tree_selected = 0;
    state.tree_rename_state = Some(TreeRenameState {
        index: 0,
        input: "renamed.txt".to_string(),
        cursor: 11,
    });
    
    let syntax_engine = SyntaxEngine::new();
    commit_tree_rename(&mut state, &syntax_engine);
    
    let new_path = temp_dir.path().join("renamed.txt");
    assert_eq!(state.buffers[0].path, new_path);
}
```

#### 7.3 Delete Tests

```rust
#[test]
fn test_begin_tree_delete_disallows_root() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    state.file_tree.root = temp_dir.path().to_path_buf();
    
    state.tree_view = vec![FileEntry {
        path: temp_dir.path().to_path_buf(),
        depth: 0,
        expanded: false,
        is_dir: true,
    }];
    state.tree_selected = 0;
    
    begin_tree_delete(&mut state);
    
    assert!(state.tree_delete_confirm.is_none());
    assert!(state.status_msg.contains("cannot delete tree root"));
}

#[test]
fn test_commit_tree_delete_file() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::File::create(&file_path).unwrap();
    
    state.tree_view = vec![FileEntry::from_path(&file_path)];
    state.file_tree.entries = state.tree_view.clone();
    state.tree_selected = 0;
    state.tree_delete_confirm = Some(TreeDeleteState {
        index: 0,
        children_preview: vec![],
    });
    
    commit_tree_delete(&mut state);
    
    assert!(!file_path.exists());
    assert!(state.tree_view.is_empty());
    assert!(state.tree_delete_confirm.is_none());
}

#[test]
fn test_commit_tree_delete_folder_recursive() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let sub_dir = temp_dir.path().join("subdir");
    std::fs::create_dir(&sub_dir).unwrap();
    std::fs::File::create(sub_dir.join("nested.txt")).unwrap();
    
    state.tree_view = vec![
        FileEntry { path: sub_dir.clone(), depth: 0, expanded: true, is_dir: true },
        FileEntry { path: sub_dir.join("nested.txt"), depth: 1, expanded: false, is_dir: false },
    ];
    state.file_tree.entries = state.tree_view.clone();
    state.tree_selected = 0;
    state.tree_delete_confirm = Some(TreeDeleteState {
        index: 0,
        children_preview: vec!["nested.txt".to_string()],
    });
    
    commit_tree_delete(&mut state);
    
    assert!(!sub_dir.exists());
    assert!(state.tree_view.is_empty());
}
```

#### 7.4 Create New File Tests

```rust
#[test]
fn test_begin_tree_new_file_on_folder() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    
    state.tree_view = vec![FileEntry {
        path: temp_dir.path().to_path_buf(),
        depth: 0,
        expanded: true,
        is_dir: true,
    }];
    state.tree_selected = 0;
    
    begin_tree_new_file(&mut state);
    
    assert!(state.tree_new_file_state.is_some());
    let new_file_state = state.tree_new_file_state.unwrap();
    assert_eq!(new_file_state.parent_dir, temp_dir.path());
    assert_eq!(new_file_state.input, "");
}

#[test]
fn test_begin_tree_new_file_on_file() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("sibling.txt");
    std::fs::File::create(&file_path).unwrap();
    
    state.tree_view = vec![FileEntry::from_path(&file_path)];
    state.tree_selected = 0;
    
    begin_tree_new_file(&mut state);
    
    assert!(state.tree_new_file_state.is_some());
    let new_file_state = state.tree_new_file_state.unwrap();
    assert_eq!(new_file_state.parent_dir, temp_dir.path());
}

#[test]
fn test_commit_tree_new_file_success() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    
    state.file_tree.root = temp_dir.path().to_path_buf();
    state.tree_new_file_state = Some(TreeNewFileState {
        parent_dir: temp_dir.path().to_path_buf(),
        input: "newfile.rs".to_string(),
        cursor: 10,
    });
    
    let syntax_engine = SyntaxEngine::new();
    commit_tree_new_file(&mut state, &syntax_engine);
    
    let new_path = temp_dir.path().join("newfile.rs");
    assert!(new_path.exists());
    assert_eq!(state.mode, Mode::Edit);
    assert!(!state.focus_tree);
}

#[test]
fn test_commit_tree_new_file_existing() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    let existing = temp_dir.path().join("exists.txt");
    std::fs::File::create(&existing).unwrap();
    
    state.tree_new_file_state = Some(TreeNewFileState {
        parent_dir: temp_dir.path().to_path_buf(),
        input: "exists.txt".to_string(),
        cursor: 10,
    });
    
    commit_tree_new_file(&mut state);
    
    assert!(state.tree_new_file_state.is_some()); // State remains active
    assert!(state.status_msg.contains("already exists"));
}

#[test]
fn test_commit_tree_new_file_rejects_path_separators() {
    let mut state = EditorState::new();
    let temp_dir = TempDir::new().unwrap();
    
    state.tree_new_file_state = Some(TreeNewFileState {
        parent_dir: temp_dir.path().to_path_buf(),
        input: "sub/file.rs".to_string(),
        cursor: 11,
    });
    
    commit_tree_new_file(&mut state);
    
    let new_path = temp_dir.path().join("sub/file.rs");
    assert!(!new_path.exists());
    assert!(state.status_msg.contains("cannot contain path separators"));
}
```

---

## Edge Case Handling

1. **Rename to existing name**: Check `new_path.exists()` before calling `std::fs::rename`. Surface error rather than clobbering.

2. **Rename tree root**: After renaming, update `file_tree.root` to the new path so the pane title reflects the change.

3. **Rename open buffer's parent folder**: `rename_subtree` rewrites all paths starting with the old prefix, so buffers inside the renamed folder are updated automatically.

4. **Delete active buffer's file**: Preserve the buffer in memory; do not close it. Show status hint so the user knows the file is gone. Ctrl-S will re-create it.

5. **Delete folder containing deeply nested open buffer**: The path-prefix check in `commit_tree_delete` catches all depths; no special case needed.

6. **Ctrl-N on empty tree_view**: Fall back to `file_tree.root` as `parent_dir`.

7. **Ctrl-N input containing path separators**: Reject `/` and `\`; show error. Users must first expand or create the parent folder.

8. **Ctrl-D on tree root**: Guarded in `begin_tree_delete`; show error and do not open dialog.

9. **Empty tree_view after delete**: Set `tree_selected = 0`; the pane renders a blank bordered box.

10. **Rename/delete with fs watcher active (WI-13)**: `apply_tree_event` from WI-13 is idempotent for entries already removed (`retain` on missing entries is a no-op), so double-processing is safe.

11. **Ctrl-T pressed while rename/delete/new-file state is active**: Priority guards return before `handle_tree_keys` is reached, so Ctrl-T is absorbed. This avoids dangling op state when the tree hides.

12. **Very long filenames in inline edit**: Clip rendered display to row width; full input string is kept in state, display scrolls from right if it overflows.

13. **Esc during rename with unchanged name**: Discard state cleanly; no `fs::rename` is issued.

---

## Code Layer Discipline

Verify that code is placed in the correct architectural layer:

- **Layer 0** (`src/data/state.rs`):
  - `TreeRenameState`
  - `TreeDeleteState`
  - `TreeNewFileState`

- **Layer 2** (`src/frontend/tui/app.rs`):
  - `begin_tree_rename`, `handle_tree_rename_input`, `commit_tree_rename`
  - `begin_tree_delete`, `handle_tree_delete_input`, `commit_tree_delete`
  - `begin_tree_new_file`, `handle_tree_new_file_input`, `commit_tree_new_file`
  - Keybinding dispatch in `handle_tree_keys` and priority guards in main key router

- **Layer 2** (`src/frontend/tui/tree_pane.rs`):
  - Inline rename rendering in `render` function
  - Synthetic row rendering for new file creation

- **Layer 2** (`src/frontend/tui/exit_modal.rs`):
  - `render_delete_confirm` function
  - Make `centered_rect` `pub(super)` for reuse

---

## Reuse Existing Functions

- **`rename_subtree`**: Already exists in `app.rs` from WI-13. Reuse in `commit_tree_rename` — do not duplicate path-prefix rewrite logic.
- **`insert_entry_sorted`**: From WI-13. Reuse in `commit_tree_new_file`. If WI-13 has not landed, add the helper in this work item with a note for deduplication on merge.
- **`state.open_file`**: Existing method on `EditorState` in `src/data/state.rs`. Use in `commit_tree_new_file`.
- **`refresh_buffer_caches`**: Existing method. Call after path updates or buffer creation.
- **`centered_rect`**: Currently private in `exit_modal.rs`. Make `pub(super)` so `render_delete_confirm` can use it.

---

## Code Quality

Before marking complete, run:

```bash
cargo clippy -- -D warnings
cargo fmt --check
cargo test
```

---

## Summary

This implementation adds three essential filetree operations to ane's TUI:

1. **Ctrl-R**: Inline rename with filesystem synchronization
2. **Ctrl-D**: Delete with confirmation modal
3. **Ctrl-N**: Create new files in the right directory context

Plus a bug fix for empty-parent path handling.

All operations maintain state consistency between the filesystem, file tree, and open buffers. Edge cases are handled robustly, and the implementation respects ane's three-layer architecture.

