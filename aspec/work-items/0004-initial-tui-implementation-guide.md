# TUI Implementation Guide (Work Item 0004)

**Status**: Implementation Phase  
**Layer**: 2 (Frontend)  
**Module**: `src/frontend/tui/`  
**Related**: `src/data/state.rs`, `src/commands/chord_engine/`, `aspec/work-items/0004-initial-tui-implementation.md`

## Overview

This document provides the complete implementation roadmap for Work Item 0004: **Implement initial TUI (interactive terminal UI) for ane**. It synthesizes the technical specification with detailed implementation guidance, module organization, test strategies, and integration points.

The TUI implementation spans three major components: the **editor pane** (file display, edit/chord mode, syntax highlighting), the **chord box** (floating interactive input), and the **file tree pane** (expandable directory navigation). All are rendered into a coordinated layout with proper keyboard routing, state management, and async LSP integration.

---

## Executive Summary

### What We're Building

A fully interactive terminal editor with:

| Component | Purpose | Module |
|-----------|---------|--------|
| **Editor Pane** | Display file content, cursor, syntax highlighting | `src/frontend/tui/editor_pane.rs` |
| **Chord Box** | Floating input for chord commands with auto-submit | `src/frontend/tui/chord_box.rs` |
| **File Tree Pane** | Expandable directory browser with selection | `src/frontend/tui/tree_pane.rs` |
| **Title Bar** | Display current file and dirty status | `src/frontend/tui/title_bar.rs` |
| **Status Bar** | Display mode, position, LSP status | `src/frontend/tui/status_bar.rs` |
| **Modals** | Exit confirmation, unsaved changes dialogs | `src/frontend/tui/exit_modal.rs` |
| **App Layout** | Coordinate all components, event routing | `src/frontend/tui/app.rs` |

### Key Design Principles

1. **Async LSP integration**: All LSP operations run in background tasks without blocking the render loop
2. **Shared state via mutex**: `LspSharedState` is written by async tasks, read by render loop
3. **Constraint-based layout**: ratatui constraints automatically reflow all components on resize
4. **Hardware cursor**: Native terminal cursor in edit mode, static blue selection in chord mode
5. **Debounced token fetching**: 300ms debounce after last keypress before requesting semantic tokens
6. **Auto-submit chords**: 4-char short-form and long-form chords submit when complete and unambiguous

### Success Criteria

- [ ] Editor renders file content with proper scrolling and cursor positioning
- [ ] Syntax highlighting via LSP semantic tokens (not static)
- [ ] Chord box auto-submits valid short/long-form chords at exactly the right moment
- [ ] File tree opens/closes on Ctrl-T, lazy-loads on first open, maintains selection
- [ ] Tree expand/collapse works correctly with proper visual state (▸ vs ▾)
- [ ] Modals prevent interaction with editor while shown
- [ ] Render loop never blocks on LSP operations
- [ ] Terminal resize causes proper layout reflow with no panics
- [ ] All manual test scenarios pass
- [ ] Integration tests verify chord dispatch from TUI state

---

## Part 1: State Management

### EditorState Extensions

Add these fields to `src/data/state.rs` in the `EditorState` struct:

```rust
pub struct EditorState {
    // ... existing fields ...
    
    // Chord mode state
    pub chord_cursor_col: usize,              // cursor position within chord_input
    pub chord_error: bool,                    // chord box shows red border
    pub chord_running: bool,                  // chord is executing (grey, yellow border)
    
    // Tree mode state
    pub pre_tree_mode: Mode,                  // mode before Ctrl-T focused tree
    pub focus_tree: bool,                     // tree pane has focus (false = editor has focus)
    pub tree_selected: usize,                 // index in tree_view
    pub tree_view: Vec<FileEntry>,            // visible entries (cached flat list)
    
    // Pending operations
    pub pending_open_path: Option<PathBuf>,   // file to open when buffer is clean
    
    // LSP shared state (written by async tasks, read by render loop)
    pub lsp_state: Arc<Mutex<LspSharedState>>,
}
```

### LspSharedState Definition

Add to `src/data/lsp/types.rs` (Layer 0):

```rust
pub struct LspSharedState {
    pub status: ServerState,
    pub semantic_tokens: Vec<SemanticToken>,
}

pub struct SemanticToken {
    pub line: usize,
    pub start_col: usize,
    pub length: usize,
    pub token_type: String,  // e.g. "keyword", "string", "comment", "type"
}
```

### Initialization

**For file mode** (`EditorState::for_file`):
```rust
chord_cursor_col: 0,
chord_error: false,
chord_running: false,
pre_tree_mode: Mode::Edit,
focus_tree: false,
tree_selected: 0,
tree_view: Vec::new(),
pending_open_path: None,
lsp_state: Arc::new(Mutex::new(LspSharedState {
    status: ServerState::Undetected,
    semantic_tokens: Vec::new(),
})),
```

**For directory mode** (`EditorState::for_directory`):
```rust
// All of the above, plus:
tree_view: {
    // Populate with depth-0 entries only (all dirs start collapsed)
    file_tree.entries
        .iter()
        .filter(|e| e.depth == 0)
        .cloned()
        .collect()
}
```

### State Reset on Chord Clear

Whenever `chord_input` is cleared, reset together:
```rust
chord_input.clear();
chord_cursor_col = 0;
chord_error = false;
chord_running = false;
```

---

## Part 2: Layout System

### Two-Level Constraint-Based Layout

All layout is computed at the top of the draw function in `src/frontend/tui/app.rs`, before calling render functions. This ensures a single source of truth for all rects.

**Level 1: Horizontal Split (full terminal)**

```rust
let layout = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([
        if state.file_tree.is_some() && state.focus_tree {
            Constraint::Percentage(25)
        } else {
            Constraint::Length(0)  // zero-width left slot
        },
        Constraint::Percentage(100),  // editor always gets remaining width
    ])
    .split(frame.size());

let [tree_area, editor_area] = [layout[0], layout[1]];
```

**Level 2: Vertical Split (editor column)**

```rust
let editor_layout = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(1),    // title bar
        Constraint::Min(0),       // editor pane (grows to fill)
        Constraint::Length(1),    // status bar
    ])
    .split(editor_area);

let [title_area, editor_pane_area, status_area] = 
    [editor_layout[0], editor_layout[1], editor_layout[2]];
```

**Chord box overlay**

Computed from `editor_pane_area`:
```rust
let chord_box_area = if state.mode == Mode::Chord 
    && editor_pane_area.width >= 4 
    && editor_pane_area.height >= 5 
{
    Some(Rect {
        x: editor_pane_area.x + 1,
        y: editor_pane_area.bottom().saturating_sub(4),
        width: editor_pane_area.width.saturating_sub(2),
        height: 3,
    })
} else {
    None
};
```

### Why This Design

- **Single source of truth**: All rects derived from one constraint computation
- **Automatic reflow**: When tree opens/closes, constraints change, `editor_area` shrinks/expands, all children resize with it
- **No conditional branches in render functions**: Each render function receives a pre-computed rect, no layout logic inside
- **Terminal resize handling**: Constraints recomputed every frame from `frame.size()`, so resize is automatic

---

## Part 3: Editor Pane

### Module Structure

File: `src/frontend/tui/editor_pane.rs`

```rust
pub mod editor_pane {
    use ratatui::{
        prelude::*,
        widgets::{Block, Paragraph},
    };
    use crate::data::buffer::Buffer;
    use crate::data::lsp::types::SemanticToken;
    use crate::data::state::EditorState;
    
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        state: &EditorState,
        lsp_status: ServerState,
        semantic_tokens: &[SemanticToken],
    ) {
        // ... implementation
    }
    
    fn token_type_to_style(token_type: &str) -> Style {
        // ... implementation
    }
}
```

### Cursor Rendering

**Edit mode** (blinking terminal cursor):
```rust
if state.mode == Mode::Edit {
    let cursor_y = state.cursor_line.saturating_sub(scroll_offset);
    let cursor_x = state.cursor_col + line_num_width;
    frame.set_cursor_position(Position { x: cursor_x, y: cursor_y });
}
```

**Chord mode** (static blue highlight):
```rust
if state.mode == Mode::Chord {
    // Don't call frame.set_cursor_position()
    // Instead, render the character at cursor with blue background in span generation
}
```

### Syntax Highlighting Implementation

```rust
fn render_content(
    frame: &mut Frame,
    area: Rect,
    buffer: &Buffer,
    lsp_status: ServerState,
    semantic_tokens: &[SemanticToken],
    scroll_offset: usize,
) {
    let mut lines = Vec::new();
    
    for (line_idx, line) in buffer.lines()
        .iter()
        .skip(scroll_offset)
        .take(area.height as usize)
        .enumerate()
    {
        let real_line_idx = scroll_offset + line_idx;
        
        // Build spans for this line
        let spans = if lsp_status == ServerState::Running {
            spans_for_line_with_highlighting(
                line,
                real_line_idx,
                semantic_tokens,
            )
        } else {
            // No highlighting while LSP is starting/installing
            vec![Span::raw(line.clone())]
        };
        
        lines.push(Line::from(spans));
    }
    
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(paragraph, area);
}

fn spans_for_line_with_highlighting(
    line: &str,
    line_idx: usize,
    tokens: &[SemanticToken],
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut last_col = 0;
    
    // Find all tokens on this line
    let line_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| t.line == line_idx)
        .collect();
    
    for token in line_tokens {
        // Add unstyled text before token
        if token.start_col > last_col {
            let unstyled = &line[last_col..token.start_col];
            spans.push(Span::raw(unstyled.to_string()));
        }
        
        // Add styled token
        let token_text = &line[token.start_col..token.start_col + token.length];
        let style = token_type_to_style(&token.token_type);
        spans.push(Span::styled(token_text.to_string(), style));
        
        last_col = token.start_col + token.length;
    }
    
    // Add trailing unstyled text
    if last_col < line.len() {
        spans.push(Span::raw(line[last_col..].to_string()));
    }
    
    spans
}

fn token_type_to_style(token_type: &str) -> Style {
    match token_type {
        "keyword" => Style::default().fg(Color::Blue),
        "function" => Style::default().fg(Color::Cyan),
        "variable" => Style::default().fg(Color::White),
        "type" => Style::default().fg(Color::Cyan),
        "string" => Style::default().fg(Color::Green),
        "comment" => Style::default().fg(Color::DarkGray),
        "number" => Style::default().fg(Color::Yellow),
        "operator" => Style::default().fg(Color::White),
        _ => Style::default(),
    }
}
```

### Scroll Management

```rust
fn compute_scroll_offset(
    cursor_line: usize,
    visible_height: usize,
    total_lines: usize,
) -> usize {
    let scroll_offset = if cursor_line < 10 {
        0
    } else {
        cursor_line - 10
    };
    
    scroll_offset.min(total_lines.saturating_sub(visible_height))
}
```

### Chord Mode Visual Indicator

When in chord mode, render a static blue highlight at the cursor position:

```rust
// In spans_for_line_with_highlighting, when rendering the character at cursor
if state.mode == Mode::Chord 
    && line_idx == state.cursor_line 
    && col == state.cursor_col
{
    spans.push(Span::styled(
        char.to_string(),
        Style::default().bg(Color::Blue),
    ));
}
```

---

## Part 4: Chord Box

### Module Structure

Rename `src/frontend/tui/command_bar.rs` to `chord_box.rs` and update `mod.rs`:

```rust
pub mod chord_box {
    use ratatui::prelude::*;
    use crate::data::state::EditorState;
    
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        state: &EditorState,
    ) {
        // ... implementation
    }
}
```

### Visual State Machine

```rust
fn determine_border_style(state: &EditorState) -> Style {
    match (state.chord_error, state.chord_running) {
        (false, false) => Style::default().fg(Color::Blue),      // Normal (idle)
        (false, true) => Style::default().fg(Color::Yellow),     // Running
        (true, _) => Style::default().fg(Color::Red),            // Error
    }
}

fn determine_text_style(state: &EditorState) -> Style {
    match (state.chord_error, state.chord_running) {
        (false, false) => Style::default().fg(Color::White),
        (false, true) => Style::default().fg(Color::DarkGray),
        (true, _) => Style::default().fg(Color::White),
    }
}
```

### Rendering

```rust
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
) {
    if state.mode != Mode::Chord || area.is_empty() {
        return;
    }
    
    let border_style = determine_border_style(state);
    let text_style = determine_text_style(state);
    
    let block = Block::default()
        .title("Chord")
        .borders(Borders::ALL)
        .border_style(border_style);
    
    let inner = block.inner(area);
    
    let paragraph = Paragraph::new(state.chord_input.clone())
        .style(text_style);
    
    frame.render_widget(block, area);
    frame.render_widget(paragraph, inner);
    
    // Set cursor position for blinking chord input cursor
    let cursor_x = inner.x + state.chord_cursor_col as u16;
    let cursor_y = inner.y;
    frame.set_cursor_position(Position { x: cursor_x, y: cursor_y });
}
```

### Cursor Management in Chord Mode

```rust
pub fn handle_key(state: &mut EditorState, key: KeyEvent) {
    match key.code {
        KeyCode::Left => {
            state.chord_cursor_col = state.chord_cursor_col.saturating_sub(1);
        },
        KeyCode::Right => {
            state.chord_cursor_col = (state.chord_cursor_col + 1)
                .min(state.chord_input.len());
        },
        KeyCode::Char(c) => {
            state.chord_input.insert(state.chord_cursor_col, c);
            state.chord_cursor_col += 1;
        },
        KeyCode::Backspace => {
            if state.chord_cursor_col > 0 {
                state.chord_cursor_col -= 1;
                state.chord_input.remove(state.chord_cursor_col);
            }
        },
        _ => {}
    }
}
```

---

## Part 5: File Tree Pane

### Module Structure

File: `src/frontend/tui/tree_pane.rs`

Extend the existing `tree_pane.rs` with expand/collapse operations.

### Rendering

```rust
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
) {
    let tree = match &state.file_tree {
        Some(t) => t,
        None => return,
    };
    
    let block = Block::default()
        .title(tree.root.display().to_string())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    
    let inner = block.inner(area);
    let mut lines = Vec::new();
    
    // Render each visible entry
    for (idx, entry) in state.tree_view.iter().enumerate() {
        let icon = if entry.is_dir {
            let is_expanded = state.tree_view
                .get(idx + 1)
                .map(|next| next.depth > entry.depth)
                .unwrap_or(false);
            if is_expanded { "▾" } else { "▸" }
        } else {
            " "
        };
        
        let indent = "  ".repeat(entry.depth);
        let name = entry.path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        
        let content = format!("{}{} {}", indent, icon, name);
        
        let style = if idx == state.tree_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        
        lines.push(Line::from(Span::styled(
            content.to_string(),
            style,
        )));
    }
    
    let paragraph = Paragraph::new(lines);
    frame.render_widget(block, area);
    frame.render_widget(paragraph, inner);
}
```

### Expand Operation

```rust
pub fn expand(state: &mut EditorState, idx: usize) {
    if idx >= state.tree_view.len() {
        return;
    }
    
    let entry = &state.tree_view[idx];
    if !entry.is_dir {
        return;
    }
    
    // Check if already expanded (next entry has greater depth)
    if state.tree_view
        .get(idx + 1)
        .map(|next| next.depth > entry.depth)
        .unwrap_or(false)
    {
        return; // Already expanded
    }
    
    let dir_path = entry.path.clone();
    let dir_depth = entry.depth;
    
    // Find direct children in file_tree
    let children: Vec<FileEntry> = state.file_tree
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .filter(|e| {
            e.path.parent() == Some(&dir_path) && e.depth == dir_depth + 1
        })
        .cloned()
        .collect();
    
    // Insert children into tree_view starting at idx + 1
    for (offset, child) in children.iter().enumerate() {
        state.tree_view.insert(idx + 1 + offset, child.clone());
    }
}
```

### Collapse Operation

```rust
pub fn collapse(state: &mut EditorState, idx: usize) {
    if idx >= state.tree_view.len() {
        return;
    }
    
    let entry = &state.tree_view[idx];
    if !entry.is_dir {
        return;
    }
    
    let dir_depth = entry.depth;
    
    // Find contiguous run of entries with depth > dir_depth
    let mut end_idx = idx + 1;
    while end_idx < state.tree_view.len() 
        && state.tree_view[end_idx].depth > dir_depth
    {
        end_idx += 1;
    }
    
    // Remove all entries in the range
    if end_idx > idx + 1 {
        state.tree_view.drain((idx + 1)..end_idx);
    }
    
    // Clamp tree_selected
    state.tree_selected = state.tree_selected.min(
        state.tree_view.len().saturating_sub(1)
    );
}
```

### Keybindings

In `app.rs` event handler, when `state.focus_tree`:

```rust
KeyCode::Up => {
    state.tree_selected = state.tree_selected.saturating_sub(1);
}
KeyCode::Down => {
    state.tree_selected = (state.tree_selected + 1)
        .min(state.tree_view.len().saturating_sub(1));
}
KeyCode::Right => {
    let entry = &state.tree_view[state.tree_selected];
    if entry.is_dir {
        tree_pane::expand(state, state.tree_selected);
    }
}
KeyCode::Left => {
    let entry = &state.tree_view[state.tree_selected];
    if entry.is_dir {
        tree_pane::collapse(state, state.tree_selected);
    }
}
KeyCode::Enter => {
    let entry = state.tree_view[state.tree_selected].clone();
    if !entry.is_dir {
        if state.current_buffer().map_or(false, |b| b.dirty) {
            state.pending_open_path = Some(entry.path);
        } else {
            state.open_file(entry.path);
        }
    }
}
KeyCode::Char('T') if key.modifiers.contains(KeyModifiers::CONTROL) => {
    state.focus_tree = false;
    state.mode = state.pre_tree_mode;
}
```

---

## Part 6: Title Bar and Status Bar

### Title Bar

New file: `src/frontend/tui/title_bar.rs`

```rust
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
) {
    let filename = state.current_buffer()
        .and_then(|b| b.path.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(
            state.file_tree.as_ref()
                .and_then(|t| t.root.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("ane")
        );
    
    let dirty_indicator = if state.current_buffer()
        .map_or(false, |b| b.dirty)
    {
        "[+]"
    } else {
        "✓"
    };
    
    let title = format!("{} {}", filename, dirty_indicator);
    let paragraph = Paragraph::new(title)
        .alignment(Alignment::SpaceBetween);
    
    frame.render_widget(paragraph, area);
}
```

### Status Bar

Extend existing `src/frontend/tui/status_bar.rs` to ensure LSP status updates every frame:

```rust
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_status: ServerState,
) {
    let (line, col) = state.cursor_line_col();
    let mode_str = match state.mode {
        Mode::Edit => "EDIT",
        Mode::Chord => "CHORD",
    };
    
    let lsp_str = match lsp_status {
        ServerState::Undetected => "LSP: undetected",
        ServerState::Installing => "LSP: installing...",
        ServerState::Starting => "LSP: starting...",
        ServerState::Running => "LSP: running",
        ServerState::Failed => "LSP: failed",
    };
    
    let status = format!(
        "{} | {}:{} | {}",
        mode_str, line, col, lsp_str
    );
    
    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}
```

---

## Part 7: Modals

### Exit Modal

Update `src/frontend/tui/exit_modal.rs`:

```rust
pub fn render(
    frame: &mut Frame,
    state: &EditorState,
) {
    if !state.show_exit_modal {
        return;
    }
    
    let dirty = state.current_buffer()
        .map_or(false, |b| b.dirty);
    
    let text = if dirty {
        "Unsaved changes. Ctrl-C again to quit without saving, \
         Ctrl-S to save and quit, Esc to cancel"
    } else {
        "Press Ctrl-C again to quit, Esc to cancel"
    };
    
    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    
    let area = centered_rect(60, 20, frame.size());
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

pub fn handle_key(state: &mut EditorState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Exit
            return true;
        },
        KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::CONTROL)
            && state.current_buffer().map_or(false, |b| b.dirty) =>
        {
            // Save then exit
            state.current_buffer_mut().map(|b| b.save());
            return true;
        },
        KeyCode::Esc => {
            state.show_exit_modal = false;
        },
        _ => {}
    }
    false
}
```

### Open Modal

Add to `src/frontend/tui/exit_modal.rs` or new `open_modal.rs`:

```rust
pub fn render_open_modal(
    frame: &mut Frame,
    state: &EditorState,
) {
    if state.pending_open_path.is_none() {
        return;
    }
    
    let text = "Unsaved changes. Ctrl-S to save and open, \
                 Ctrl-O to discard and open, Esc to cancel";
    
    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    
    let area = centered_rect(60, 20, frame.size());
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

pub fn handle_key(state: &mut EditorState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.save();
            }
            if let Some(path) = state.pending_open_path.take() {
                state.open_file(path);
            }
            return true;
        },
        KeyCode::Char('O') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(path) = state.pending_open_path.take() {
                state.open_file(path);
            }
            return true;
        },
        KeyCode::Esc => {
            state.pending_open_path = None;
        },
        _ => {}
    }
    false
}
```

---

## Part 8: LSP Integration

### Async Architecture

In `src/frontend/tui/app.rs`, the `run()` function sets up LSP background tasks:

```rust
pub fn run(mut state: EditorState, engine: &LspEngine) -> Result<()> {
    // Create Tokio runtime for background LSP tasks
    let rt = tokio::runtime::Runtime::new()?;
    
    // Start LSP server synchronously (just registers intent, returns immediately)
    if let Some(lang) = state.primary_language() {
        engine.start_for_context(lang)?;
    }
    
    // Clone lsp_state Arc for background tasks
    let lsp_state_clone1 = state.lsp_state.clone();
    let lsp_state_clone2 = state.lsp_state.clone();
    
    // Channel for token requests (capacity 1 for debouncing)
    let (token_tx, mut token_rx) = tokio::sync::mpsc::channel(1);
    
    // Task 1: Status polling
    let engine_clone = engine.clone();
    rt.spawn(async move {
        let mut last_status = ServerState::Undetected;
        loop {
            if let Ok(mut guard) = lsp_state_clone1.lock() {
                guard.status = engine_clone.server_state(lang);
                
                // If just transitioned to Running, trigger token fetch
                if guard.status == ServerState::Running 
                    && last_status != ServerState::Running
                {
                    // Send token request for active buffer
                    let _ = token_tx.try_send(/* current buffer path and content */);
                }
                last_status = guard.status;
            }
            
            // Poll frequency depends on status
            let delay = if last_status == ServerState::Running {
                Duration::from_secs(3)
            } else {
                Duration::from_secs(1)
            };
            tokio::time::sleep(delay).await;
        }
    });
    
    // Task 2: Token requests
    let lsp_state_clone2_clone = lsp_state_clone2.clone();
    rt.spawn(async move {
        while let Some((path, content)) = token_rx.recv().await {
            if let Ok(tokens) = engine_clone.semantic_tokens(&path, &content) {
                if let Ok(mut guard) = lsp_state_clone2_clone.lock() {
                    guard.semantic_tokens = tokens;
                }
            }
        }
    });
    
    // Enter crossterm event loop (synchronous)
    run_event_loop(state, token_tx)?;
    
    Ok(())
}
```

### Event Loop Debouncing

```rust
fn run_event_loop(
    mut state: EditorState,
    token_tx: tokio::sync::mpsc::Sender<(PathBuf, String)>,
) -> Result<()> {
    let mut last_edit = None;
    
    loop {
        // Handle terminal events
        if let Some(event) = crossterm::event::read()? {
            match event {
                // ... event handling ...
                Event::Key(key) => {
                    // Handle key press
                    handle_key(&mut state, key);
                    
                    // If buffer was modified, set debounce timer
                    if state.mode == Mode::Edit {
                        last_edit = Some(Instant::now());
                    }
                },
                _ => {}
            }
        }
        
        // Check debounce timer
        if let Some(instant) = last_edit {
            if instant.elapsed() >= Duration::from_millis(300) {
                // Send token request
                if let Some(buffer) = state.current_buffer() {
                    let path = buffer.path.clone();
                    let content = buffer.content.clone();
                    let _ = token_tx.try_send((path, content));
                }
                last_edit = None;
            }
        }
        
        // Render frame
        draw(&mut state)?;
    }
}
```

### Render Loop Locking

At the top of the draw function:

```rust
fn draw(state: &mut EditorState) -> Result<()> {
    // Acquire mutex, clone fields, release immediately
    let (lsp_status, semantic_tokens) = {
        let guard = state.lsp_state.lock().unwrap();
        (guard.status.clone(), guard.semantic_tokens.clone())
    };
    
    // All rendering uses locals (mutex is released)
    // ... render all components ...
    
    Ok(())
}
```

---

## Part 9: Event Routing and Key Handling

### Priority Order

In `src/frontend/tui/app.rs` `handle_event()`:

```rust
pub fn handle_event(state: &mut EditorState, event: Event) -> bool {
    if let Event::Key(key) = event {
        // Priority 1: Exit modal (if shown)
        if state.show_exit_modal {
            return exit_modal::handle_key(state, key);
        }
        
        // Priority 2: Open modal (if shown)
        if state.pending_open_path.is_some() {
            return open_modal::handle_key(state, key);
        }
        
        // Priority 3: Ctrl-T always handled
        if key.code == KeyCode::Char('T') 
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return handle_toggle_tree(state);
        }
        
        // Priority 4: Route by focus
        if state.focus_tree {
            return tree_pane::handle_key(state, key);
        }
        
        // Priority 5: Route by mode
        match state.mode {
            Mode::Chord => {
                return chord_key_handler(state, key);
            },
            Mode::Edit => {
                return editor_key_handler(state, key);
            }
        }
    }
    false
}
```

### Chord Key Handler

```rust
fn chord_key_handler(state: &mut EditorState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) => {
            chord_box::handle_key(state, key);
            
            // Try auto-submit after every character change
            try_auto_submit(state);
        },
        KeyCode::Backspace => {
            chord_box::handle_key(state, key);
            
            // Don't auto-submit on backspace, wait for completion
        },
        KeyCode::Left | KeyCode::Right => {
            // Arrow keys adjust chord_cursor_col
            chord_box::handle_key(state, key);
        },
        KeyCode::Enter => {
            // Explicit submit
            match ChordEngine::parse(&state.chord_input) {
                Ok(query) => {
                    state.chord_running = true;
                    // Dispatch chord (would be async in real implementation)
                    state.chord_error = false;
                },
                Err(_) => {
                    state.chord_error = true;
                }
            }
        },
        KeyCode::Esc => {
            state.chord_input.clear();
            state.chord_cursor_col = 0;
            state.chord_error = false;
            state.chord_running = false;
        },
        _ => {}
    }
    false
}

fn try_auto_submit(state: &mut EditorState) {
    let input = &state.chord_input;
    
    // Short-form auto-submit: exactly 4 chars, first is lowercase
    if input.len() == 4 && input.chars().next().map_or(false, |c| c.is_lowercase()) {
        if let Some(query) = ChordEngine::try_auto_submit_short(
            input,
            state.cursor_line,
            state.cursor_col,
        ) {
            state.chord_running = true;
            // Dispatch chord
            return;
        }
    }
    
    // Long-form auto-submit: ends with ), first is uppercase, balanced parens
    if input.ends_with(')') && input.chars().next().map_or(false, |c| c.is_uppercase()) {
        let paren_depth = input.chars().fold(0i32, |acc, c| {
            match c {
                '(' => acc + 1,
                ')' => acc - 1,
                _ => acc,
            }
        });
        
        if paren_depth == 0 {
            if let Ok(query) = ChordEngine::parse(input) {
                state.chord_running = true;
                // Dispatch chord
                return;
            }
        }
    }
}
```

### Editor Key Handler

```rust
fn editor_key_handler(state: &mut EditorState, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.insert_char(state.cursor_line, state.cursor_col, c);
                buf.mark_dirty();
                state.cursor_col += 1;
            }
        },
        KeyCode::Enter => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.insert_newline(state.cursor_line, state.cursor_col);
                buf.mark_dirty();
                state.cursor_line += 1;
                state.cursor_col = 0;
            }
        },
        KeyCode::Backspace => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.delete_char(state.cursor_line, state.cursor_col);
                buf.mark_dirty();
                if state.cursor_col > 0 {
                    state.cursor_col -= 1;
                }
            }
        },
        KeyCode::Up => {
            state.cursor_line = state.cursor_line.saturating_sub(1);
        },
        KeyCode::Down => {
            state.cursor_line = (state.cursor_line + 1)
                .min(state.current_buffer()
                    .map(|b| b.line_count())
                    .unwrap_or(1));
        },
        KeyCode::Left => {
            state.cursor_col = state.cursor_col.saturating_sub(1);
        },
        KeyCode::Right => {
            state.cursor_col = (state.cursor_col + 1)
                .min(state.current_buffer()
                    .map(|b| b.line_len(state.cursor_line))
                    .unwrap_or(0));
        },
        KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.save()?;
            }
        },
        KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Chord;
            state.chord_input.clear();
            state.chord_cursor_col = 0;
            state.chord_error = false;
        },
        KeyCode::Tab => {
            if let Some(buf) = state.current_buffer_mut() {
                buf.insert_char(state.cursor_line, state.cursor_col, '\t');
                buf.mark_dirty();
                state.cursor_col += 1;
            }
        },
        _ => {}
    }
    false
}
```

---

## Part 10: Chord Auto-Submit Implementation

### ChordEngine Extension

Add to `src/commands/chord_engine/mod.rs`:

```rust
impl ChordEngine {
    pub fn try_auto_submit_short(
        input: &str,
        cursor_line: usize,
        cursor_col: usize,
    ) -> Option<ChordQuery> {
        // Only attempt if exactly 4 chars and first is lowercase
        if input.len() != 4 || !input.chars().next().map_or(false, |c| c.is_lowercase()) {
            return None;
        }
        
        // Construct long form with cursor position
        let query_str = format!(
            "{}(cursor_pos:[{},{}])",
            input, cursor_line, cursor_col
        );
        
        // Parse; return None on error (don't show error to user)
        Self::parse(&query_str).ok()
    }
    
    pub fn check_paren_balance(input: &str) -> bool {
        let mut depth = 0;
        for c in input.chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                },
                _ => {}
            }
        }
        depth == 0
    }
}
```

---

## Part 11: Tree Toggle and Lazy Load

### Ctrl-T Handler

Add to `src/frontend/tui/app.rs`:

```rust
pub fn handle_toggle_tree(state: &mut EditorState) -> bool {
    if state.focus_tree {
        // Already in tree mode: return to editor
        state.focus_tree = false;
        state.mode = state.pre_tree_mode;
        return true;
    }
    
    // Save current mode
    state.pre_tree_mode = state.mode;
    
    // Ensure tree is loaded
    if state.file_tree.is_none() {
        // Single-file mode: scan parent directory
        if let Some(buf) = state.current_buffer() {
            let parent = buf.path.parent();
            match parent.and_then(|p| FileTree::from_dir(p)) {
                Ok(tree) => {
                    state.file_tree = Some(tree);
                    // Populate tree_view with depth-0 entries only
                    state.tree_view = state.file_tree
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .filter(|e| e.depth == 0)
                        .cloned()
                        .collect();
                    // Find current buffer in tree_view
                    if let Some(idx) = state.tree_view
                        .iter()
                        .position(|e| e.path == buf.path)
                    {
                        state.tree_selected = idx;
                    }
                },
                Err(e) => {
                    state.status_msg = format!("Failed to load tree: {}", e);
                    return true;
                }
            }
        } else {
            // No buffer open (directory mode) - tree should already exist
            // Populate tree_view if empty
            if state.tree_view.is_empty() {
                if let Some(tree) = &state.file_tree {
                    state.tree_view = tree.entries
                        .iter()
                        .filter(|e| e.depth == 0)
                        .cloned()
                        .collect();
                }
            }
        }
    }
    
    state.focus_tree = true;
    true
}
```

---

## Part 12: Edge Cases and Error Handling

### Terminal Resize

When `frame.size()` changes:

```rust
pub fn handle_resize(state: &mut EditorState, new_size: (u16, u16)) {
    // Clamp cursor to visible area
    if let Some(buf) = state.current_buffer() {
        state.cursor_line = state.cursor_line.min(buf.line_count().saturating_sub(1));
        
        // Clamp scroll offset
        let visible_height = new_size.1 as usize;
        let scroll_offset = compute_scroll_offset(
            state.cursor_line,
            visible_height,
            buf.line_count(),
        );
        
        // If cursor is off-screen, move it
        if state.cursor_line < scroll_offset {
            state.cursor_line = scroll_offset;
        } else if state.cursor_line >= scroll_offset + visible_height {
            state.cursor_line = scroll_offset + visible_height - 1;
        }
    }
}
```

### Empty Buffer

```rust
pub fn safe_cursor_navigation(state: &mut EditorState) {
    if let Some(buf) = state.current_buffer() {
        if buf.line_count() == 0 {
            state.cursor_line = 0;
            state.cursor_col = 0;
        } else {
            state.cursor_line = state.cursor_line.min(buf.line_count() - 1);
            state.cursor_col = state.cursor_col.min(buf.line_len(state.cursor_line));
        }
    }
}
```

### Chord Box Too Narrow

```rust
let chord_box_area = if state.mode == Mode::Chord
    && editor_pane_area.width >= 4
    && editor_pane_area.height >= 5
{
    Some(Rect { /* ... */ })
} else {
    None
};
```

### Tree After Collapse

After `collapse()` operation, always clamp:

```rust
state.tree_selected = state.tree_selected.min(
    state.tree_view.len().saturating_sub(1)
);
```

### LSP Startup Latency

No special handling needed; the editor is functional during `Starting` and `Installing` states. Highlighting simply doesn't appear until `Running`.

### Missing Language Detection

```rust
if state.primary_language().is_none() {
    // No LSP for this file type
    // lsp_state.status stays Undetected
    // No highlighting
}
```

---

## Part 13: Testing Strategy

### Unit Tests

**State initialization** (`src/data/state.rs`):
```rust
#[test]
fn test_for_file_initializes_tree_empty() {
    let state = EditorState::for_file("test.rs", &buffer);
    assert!(state.file_tree.is_none());
    assert!(state.tree_view.is_empty());
    assert_eq!(state.chord_cursor_col, 0);
    assert!(!state.chord_error);
    assert!(!state.chord_running);
}

#[test]
fn test_for_directory_initializes_tree_view_depth_zero_only() {
    let state = EditorState::for_directory(".", &file_tree);
    assert!(state.file_tree.is_some());
    for entry in &state.tree_view {
        assert_eq!(entry.depth, 0);
    }
}
```

**Tree operations** (`src/frontend/tui/tree_pane.rs`):
```rust
#[test]
fn test_expand_adds_direct_children() {
    let mut state = setup_state_with_nested_tree();
    assert_eq!(state.tree_view.len(), 1); // just root
    
    tree_pane::expand(&mut state, 0);
    
    assert_eq!(state.tree_view.len(), 3); // root + 2 children
    assert_eq!(state.tree_view[1].depth, 1);
    assert_eq!(state.tree_view[2].depth, 1);
}

#[test]
fn test_collapse_removes_all_descendants() {
    let mut state = setup_expanded_state();
    assert_eq!(state.tree_view.len(), 5); // root, 2 children, 2 grandchildren
    
    tree_pane::collapse(&mut state, 0); // collapse root
    
    assert_eq!(state.tree_view.len(), 1); // just root
}

#[test]
fn test_collapse_clamps_tree_selected() {
    let mut state = setup_expanded_state();
    state.tree_selected = 4; // on a grandchild
    
    tree_pane::collapse(&mut state, 1); // collapse child
    
    assert!(state.tree_selected < state.tree_view.len());
}
```

**Chord auto-submit** (`src/commands/chord_engine/mod.rs`):
```rust
#[test]
fn test_try_auto_submit_short_valid_4char() {
    let query = ChordEngine::try_auto_submit_short("cifn", 5, 10).unwrap();
    assert_eq!(query.action, Action::Change);
    assert_eq!(query.args.cursor_pos, Some((5, 10)));
}

#[test]
fn test_try_auto_submit_short_invalid_returns_none() {
    assert_eq!(
        ChordEngine::try_auto_submit_short("xxxx", 5, 10),
        None
    );
}

#[test]
fn test_try_auto_submit_short_wrong_length_returns_none() {
    assert_eq!(
        ChordEngine::try_auto_submit_short("cif", 5, 10),
        None
    );
    assert_eq!(
        ChordEngine::try_auto_submit_short("cifnx", 5, 10),
        None
    );
}

#[test]
fn test_try_auto_submit_short_uppercase_returns_none() {
    assert_eq!(
        ChordEngine::try_auto_submit_short("CIFN", 5, 10),
        None
    );
}

#[test]
fn test_paren_balance_check() {
    assert!(ChordEngine::check_paren_balance("cif()"));
    assert!(ChordEngine::check_paren_balance("cif(a,b)"));
    assert!(!ChordEngine::check_paren_balance("cif(a,b"));
    assert!(!ChordEngine::check_paren_balance("cif)a,b("));
}
```

### Integration Tests

**File: `tests/work_item_0004.rs`**

```rust
#[test]
fn test_tui_chord_dispatch_changes_buffer() {
    let state = setup_editor_state_with_rust_file();
    let query = ChordEngine::try_auto_submit_short("cifn", 0, 0).unwrap();
    
    let result = ChordEngine::execute(
        &query,
        &buffers,
        &mut lsp,
        Some((0, 0)),
    ).unwrap();
    
    assert!(!result.is_empty());
    assert!(result[0].diff.is_some());
}

#[test]
fn test_tree_expand_then_collapse_is_idempotent() {
    let mut state = setup_editor_with_tree();
    let initial_len = state.tree_view.len();
    
    tree_pane::expand(&mut state, 0);
    let expanded_len = state.tree_view.len();
    assert!(expanded_len > initial_len);
    
    tree_pane::collapse(&mut state, 0);
    assert_eq!(state.tree_view.len(), initial_len);
}

#[test]
fn test_syntax_highlighting_renders_with_running_lsp() {
    let state = setup_editor_state();
    let tokens = vec![
        SemanticToken { line: 0, start_col: 0, length: 2, token_type: "keyword".to_string() },
    ];
    
    // Call render with tokens (in real test, setup terminal)
    // Verify styled spans are produced
}

#[test]
fn test_modal_blocks_key_events() {
    let mut state = setup_editor_state();
    state.show_exit_modal = true;
    
    // Simulate Ctrl-E (would normally toggle to Chord mode)
    let key = KeyEvent::new(KeyCode::Char('E'), KeyModifiers::CONTROL);
    handle_event(&mut state, Event::Key(key));
    
    // Mode should not change (modal captured the event)
    assert_eq!(state.mode, Mode::Edit);
}

#[test]
fn test_chord_mode_blue_cursor_static() {
    let mut state = setup_editor_state();
    state.mode = Mode::Chord;
    
    // Render frame and check that set_cursor_position is NOT called
    // (This is a bit tricky to test without a full terminal)
}

#[test]
fn test_tree_pane_displays_only_visible_entries() {
    let mut state = setup_editor_with_deep_tree();
    
    // Collapse some dirs so tree_view has gaps
    tree_pane::collapse(&mut state, 0);
    
    // Render should only iterate tree_view
    // Verify no panics and output matches tree_view exactly
}
```

### Manual Testing Checklist

- [ ] Launch `ane src/lib.rs` → editor fills window, no tree
- [ ] Launch `ane .` → tree on left 25%, all dirs collapsed with depth-0 entries, no buffer open
- [ ] Press Ctrl-T from single-file mode → tree loads, editor shrinks to 75%, current file highlighted
- [ ] Press Ctrl-T again → tree closes, editor expands to 100%, mode restored
- [ ] Resize terminal → all bars and panes reflow without gaps or overlaps
- [ ] Expand a dir with Right arrow → children appear indented, icon changes to ▾
- [ ] Collapse with Left arrow → children disappear, tree_selected clamped if needed
- [ ] Type `cifn` in chord mode → chord auto-submits without error
- [ ] Type partial chord then press Enter → error border appears (red)
- [ ] While chord running → yellow border, grey text
- [ ] Open file from tree with dirty buffer → modal appears with Ctrl-S/Ctrl-O options
- [ ] Press Ctrl-C with dirty buffer → exit modal shows save-and-quit variant
- [ ] Type in edit mode → cursor blinks naturally
- [ ] Switch to chord mode → blue highlight at cursor, static (no blink)
- [ ] Open `.rs` file → status bar shows "LSP: starting", then "LSP: running", highlighting appears
- [ ] Type rapidly → highlighting lags ~300ms but editor doesn't stutter
- [ ] Open `.txt` file → no highlighting, status shows "LSP: undetected", no polling tasks

---

## Part 14: Module Organization and File Structure

### Complete File Layout

```
src/frontend/tui/
├── mod.rs                    # Module exports, re-exports
├── app.rs                    # Main render loop, event routing, layout
├── editor_pane.rs           # File content, syntax highlighting, cursor
├── chord_box.rs             # Floating chord input (renamed from command_bar.rs)
├── tree_pane.rs             # File tree rendering and mutations
├── title_bar.rs             # Filename + dirty indicator
├── status_bar.rs            # Mode + position + LSP status
├── exit_modal.rs            # Exit and open-file modals
└── tests/                   # Unit tests for each module
```

### mod.rs Content

```rust
mod app;
mod editor_pane;
mod chord_box;
mod tree_pane;
mod title_bar;
mod status_bar;
mod exit_modal;

pub use app::run;
```

---

## Part 15: Codebase Integration

### Architecture Layer Rules

- **TUI code**: Layer 2 (`src/frontend/tui/`)
  - **Imports from**: Layer 0 (`data::*`, `data::lsp::*`), Layer 1 (`commands::chord_engine::*`)
  - **Never imports from**: Other Layer 2 modules (except within TUI)
  
- **State definitions**: Layer 0 (`src/data/state.rs`, `src/data/lsp/types.rs`)
  - **Never imports from**: Layer 1 or 2

- **Chord auto-submit**: Layer 1 (`src/commands/chord_engine/mod.rs`)
  - Added method `try_auto_submit_short`
  - **Never imports from**: Layer 2

### Async Runtime

Tokio runtime is created in `run()` and used exclusively for:
- Status polling task
- Token request task
- Debounce timer

Event loop remains synchronous (crossterm).

### Error Handling

Use `anyhow::Result` for all fallible operations. Errors in background tasks are logged but don't crash the editor.

---

## Part 16: Performance Considerations

1. **Render loop frequency**: Limited by terminal I/O (typically 60Hz max)
2. **Mutex contention**: Lock held only for cloning 2 fields at top of draw
3. **Token fetching**: Debounced to 300ms; only one request queued at a time
4. **Tree rendering**: O(visible rows), not O(total tree size)
5. **Syntax highlighting**: O(tokens on screen) per frame, not O(all tokens in file)

---

## Part 17: Success Checklist

- [ ] **State extensions complete**
  - [ ] `EditorState` has chord_*, focus_tree, pre_tree_mode, pending_open_path
  - [ ] `LspSharedState` with status and semantic_tokens
  - [ ] All fields initialized in `for_file` and `for_directory`

- [ ] **Layout system complete**
  - [ ] Two-level constraint system working
  - [ ] Tree open/close reflows editor correctly
  - [ ] Resize handling preserves layout integrity

- [ ] **Editor pane complete**
  - [ ] File content renders with proper scrolling
  - [ ] Cursor blinks in edit mode
  - [ ] Blue highlight in chord mode (static)
  - [ ] Syntax highlighting works when LSP ready
  - [ ] Highlighting matches token types

- [ ] **Chord box complete**
  - [ ] Visual state machine (normal/running/error)
  - [ ] Cursor at chord_cursor_col
  - [ ] Left/right arrow navigation
  - [ ] Character insertion at cursor
  - [ ] Auto-submit logic integrated

- [ ] **Tree pane complete**
  - [ ] Rendering O(visible entries)
  - [ ] Expand adds direct children only
  - [ ] Collapse removes entire subtree
  - [ ] Already-expanded dir is no-op
  - [ ] Selection highlight works

- [ ] **Title bar complete**
  - [ ] Shows filename
  - [ ] Shows [+] or ✓ for dirty status

- [ ] **Modals complete**
  - [ ] Exit modal shows save variant when dirty
  - [ ] Open-file modal appears on dirty+open
  - [ ] Both modals block underlying input

- [ ] **LSP integration complete**
  - [ ] Status polling task runs independently
  - [ ] Token request task processes queued requests
  - [ ] Debounce timer fires at 300ms
  - [ ] Render loop never blocks on mutex

- [ ] **Event routing complete**
  - [ ] Priority order followed (modals → Ctrl-T → focus → mode)
  - [ ] Tree keybindings work (up/down/left/right/enter/Ctrl-T)
  - [ ] Editor keybindings work (arrows/char/backspace/enter)
  - [ ] Chord keybindings work (char/backspace/arrows/enter/esc)

- [ ] **Auto-submit complete**
  - [ ] Short-form: 4 chars, first lowercase, auto-submit on completion
  - [ ] Long-form: ends with ), balanced parens, first uppercase
  - [ ] No error shown on failed auto-submit (silent)
  - [ ] Error shown on Enter if parse fails

- [ ] **Unit tests passing**
  - [ ] State initialization tests
  - [ ] Tree operation tests
  - [ ] Auto-submit tests
  - [ ] Paren balance tests

- [ ] **Integration tests passing**
  - [ ] Chord dispatch from TUI produces diffs
  - [ ] Tree expand/collapse idempotent
  - [ ] Modal blocks keys
  - [ ] All manual test scenarios pass

---

## Appendix A: Example Flows

### Flow 1: Single-File to Tree Navigation

```
1. Launch: ane src/lib.rs
   → EditorState::for_file(path)
   → state.file_tree = None, tree_view empty
   
2. User presses Ctrl-T
   → handle_toggle_tree()
   → FileTree::from_dir(parent)
   → tree_view = depth-0 entries from file_tree
   → tree_selected = index of current file
   → focus_tree = true
   
3. User presses Right arrow on dir
   → tree_pane::expand(state, tree_selected)
   → find children in file_tree
   → insert at tree_view[idx+1]
   
4. User presses Ctrl-T again
   → focus_tree = false
   → mode = pre_tree_mode (Edit)
   → editor pane gets full width again
```

### Flow 2: Chord Auto-Submit

```
1. User presses Ctrl-E (enter Chord mode)
   → mode = Chord
   → chord_input cleared
   → chord_cursor_col = 0
   
2. User types: c
   → chord_input = "c"
   → chord_cursor_col = 1
   → len < 4, no auto-submit
   
3. User types: i
   → chord_input = "ci"
   → chord_cursor_col = 2
   → len < 4, no auto-submit
   
4. User types: f
   → chord_input = "cif"
   → chord_cursor_col = 3
   → len < 4, no auto-submit
   
5. User types: n
   → chord_input = "cifn"
   → chord_cursor_col = 4
   → len == 4 && first lowercase
   → ChordEngine::try_auto_submit_short("cifn", cursor_line, cursor_col)
   → Returns Some(query)
   → chord_running = true
   → dispatch chord
```

### Flow 3: LSP Token Updates

```
Initial state:
→ lsp_state.status = Undetected
→ semantic_tokens = []
→ editor renders unstyled

On file open:
1. Event loop: buffer modified
   → last_edit = Some(Instant::now())

2. Event loop iteration:
   → check last_edit.elapsed() >= 300ms
   → send (path, content) on token_tx
   → last_edit = None

3. Token request task (async):
   → receives (path, content)
   → calls engine.semantic_tokens(path, content)
   → locks lsp_state
   → lsp_state.semantic_tokens = new_tokens
   → unlocks

4. Next render:
   → lock lsp_state, clone status and tokens
   → release lock
   → pass tokens to editor_pane::render
   → render_content creates styled spans
```

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-12  
**Author**: Implementation Team  
**Status**: Ready for Implementation
