# Work Item: Feature

Title: Application-managed mouse selection with gutter-excluded clipboard
Issue: N/A

## Summary

Replace the terminal emulator's native text selection with application-managed mouse selection so that line-number gutters are excluded from the user's clipboard. When the terminal enters mouse capture mode, mouse events are forwarded to the application instead of being handled by the terminal's built-in selection. The editor tracks selection state (anchor + head) in logical buffer coordinates, renders highlighted selection ranges, and writes the selected buffer text to the system clipboard via the OSC 52 escape sequence. Because selection coordinates are expressed in buffer space, the gutter is never part of the copied content. The user can still fall back to native terminal selection by holding Shift during a click-drag (standard terminal convention).

---

## User Stories

### User Story 1
As a: developer

I want to: click and drag across lines in the editor pane and have only the file's text content (no line numbers) copied to my clipboard

So I can: paste code into other tools, chat windows, or files without manually stripping line-number prefixes

### User Story 2
As a: developer

I want to: see a visual highlight over the text I'm selecting as I drag the mouse

So I can: confirm exactly which range of text will be copied before releasing the mouse button

### User Story 3
As a: developer

I want to: hold Shift and click-drag to fall back to native terminal selection when I need to copy raw terminal output (including line numbers or status bar text)

So I can: retain the escape hatch for edge cases where application-managed selection is not what I need

### User Story 4
As a: developer using a terminal that does not support OSC 52

I want to: see a status bar message indicating that clipboard write failed or is unsupported

So I can: understand why the copy did not work and fall back to Shift-drag native selection instead

---

## Implementation Details

### 1. Enable mouse capture — `src/frontend/tui/app.rs` (Layer 2)

In `run()`, after `enable_raw_mode()` and `EnterAlternateScreen`, enable mouse capture:

```rust
use crossterm::event::{EnableMouseCapture, DisableMouseCapture};

io::stdout().execute(EnableMouseCapture)?;
```

In the cleanup path (after `event_loop` returns and before `disable_raw_mode`), disable it:

```rust
io::stdout().execute(DisableMouseCapture)?;
```

This causes the terminal to forward all mouse events (`Event::Mouse`) to the application instead of performing its own text selection. Shift-click/drag bypasses mouse capture in virtually all modern terminals (iTerm2, kitty, Alacritty, WezTerm, Windows Terminal, macOS Terminal.app), preserving native selection as a fallback.

### 2. Selection state — `src/data/state.rs` (Layer 0)

Add a `Selection` struct and an `Option<Selection>` field to `EditorState`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor_line: usize,
    pub anchor_col: usize,
    pub head_line: usize,
    pub head_col: usize,
}

impl Selection {
    pub fn ordered(&self) -> (usize, usize, usize, usize) {
        if (self.anchor_line, self.anchor_col) <= (self.head_line, self.head_col) {
            (self.anchor_line, self.anchor_col, self.head_line, self.head_col)
        } else {
            (self.head_line, self.head_col, self.anchor_line, self.anchor_col)
        }
    }
}
```

Add to `EditorState`:

```rust
pub selection: Option<Selection>,
```

Initialize to `None` in both `for_file` and `for_directory` constructors. `anchor` is where the mouse was pressed down; `head` is where it currently is (or was released). The `ordered()` method returns `(start_line, start_col, end_line, end_col)` regardless of drag direction.

### 3. Mouse-to-buffer coordinate mapping — `src/frontend/tui/editor_pane.rs` (Layer 2)

Add a public function that converts a terminal (x, y) position to logical buffer (line, col) coordinates, accounting for the line-number gutter width, scroll offset, and soft-wrapping:

```rust
pub(crate) fn screen_to_buffer(
    x: u16,
    y: u16,
    area: Rect,
    state: &EditorState,
) -> Option<(usize, usize)>
```

Algorithm:
1. Reject clicks outside the editor `area` or within the gutter (`x < area.x + line_num_width + 1`).
2. Compute `text_col = (x - area.x - line_num_width - 1) as usize` — the display column within the text region.
3. Walk logical lines from `scroll_offset`, accumulating visual rows (using `visual_row_count`), until the accumulated row count exceeds `(y - area.y) as usize`. This identifies the logical line and which visual (wrapped) row within it the click landed on.
4. Compute the target display column: `visual_row_within_line * text_width + text_col`.
5. Convert to byte column using `byte_col_from_display(line, target_display_col)`.
6. Return `Some((logical_line, byte_col))`.

If the click is on a visual row past the last line of the buffer, clamp to the last line's end. If the click is past the end of a line's content, clamp to that line's length.

### 4. Mouse event handling — `src/frontend/tui/app.rs` (Layer 2)

In `event_loop`, the existing `event::read()` call already returns `Event::Mouse` variants when mouse capture is enabled. Extend the event dispatch:

```rust
Event::Mouse(mouse) => {
    handle_mouse(state, mouse, editor_render_area);
}
```

The `editor_render_area` `Rect` must be available in the event loop. Currently it's computed inside the `terminal.draw` closure. Hoist the area computation so it's available to the mouse handler (compute it from `terminal.size()` and the layout constraints, or store the last-drawn editor area in `EditorState`).

Implement `handle_mouse`:

```rust
fn handle_mouse(state: &mut EditorState, mouse: MouseEvent, editor_area: Rect) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some((line, col)) = editor_pane::screen_to_buffer(
                mouse.column, mouse.row, editor_area, state,
            ) {
                state.selection = Some(Selection {
                    anchor_line: line,
                    anchor_col: col,
                    head_line: line,
                    head_col: col,
                });
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(sel) = &mut state.selection {
                if let Some((line, col)) = editor_pane::screen_to_buffer(
                    mouse.column, mouse.row, editor_area, state,
                ) {
                    sel.head_line = line;
                    sel.head_col = col;
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(sel) = &state.selection {
                let text = extract_selection_text(state, sel);
                if !text.is_empty() {
                    write_osc52(&text);
                }
            }
            state.selection = None;
        }
        _ => {}
    }
}
```

On mouse-up, extract the selected text, write it to the clipboard via OSC 52, then clear the selection. The selection highlight disappears immediately on release.

### 5. Selection text extraction — `src/frontend/tui/app.rs` (Layer 2)

```rust
fn extract_selection_text(state: &EditorState, sel: &Selection) -> String {
    let (start_line, start_col, end_line, end_col) = sel.ordered();
    let buf = match state.current_buffer() {
        Some(b) => b,
        None => return String::new(),
    };
    let mut result = String::new();
    for i in start_line..=end_line {
        let line = match buf.lines.get(i) {
            Some(l) => l,
            None => break,
        };
        let from = if i == start_line { start_col.min(line.len()) } else { 0 };
        let to = if i == end_line { end_col.min(line.len()) } else { line.len() };
        if from <= to {
            result.push_str(&line[from..to]);
        }
        if i < end_line {
            result.push('\n');
        }
    }
    result
}
```

Uses byte-offset `start_col`/`end_col` (the same coordinate space as `cursor_col`) to slice directly into the buffer lines. Multi-line selections include newlines between lines but not a trailing newline.

### 6. OSC 52 clipboard write — `src/frontend/tui/app.rs` (Layer 2)

```rust
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

fn write_osc52(text: &str) {
    let encoded = BASE64.encode(text.as_bytes());
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    let _ = io::stdout().write_all(seq.as_bytes());
    let _ = io::stdout().flush();
}
```

OSC 52 format: `ESC ] 52 ; c ; <base64-payload> BEL`. The `c` parameter targets the clipboard selection (as opposed to `p` for primary selection on X11). This sequence is supported by iTerm2, kitty, Alacritty, WezTerm, Windows Terminal, foot, and most modern terminal emulators.

Add the `base64` crate to `Cargo.toml`:

```toml
[dependencies]
base64 = "0.22"
```

### 7. Selection highlight rendering — `src/frontend/tui/editor_pane.rs` (Layer 2)

After the `Paragraph` is rendered and the cursor is placed, apply selection highlighting by iterating over the visible cells and inverting the style of cells that fall within the selection range:

In `render()`, after `frame.render_widget(paragraph, area)`:

```rust
if let Some(sel) = &state.selection {
    let (start_line, start_col, end_line, end_col) = sel.ordered();
    // Walk visible logical lines (scroll_offset..) and their visual rows,
    // tracking which cells fall within the selection range.
    // For each cell in the selection, apply an inverted style:
    //   cell.set_style(Style::default().bg(Color::LightBlue).fg(Color::Black));
}
```

The highlight walk mirrors the existing rendering loop: iterate logical lines from `scroll_offset`, accumulate visual rows, and for each visual row that overlaps the selection range, compute the start/end display columns that are selected, then modify cells in `frame.buffer_mut()` at the corresponding `(x, y)` positions.

The selection highlight uses `bg(Color::LightBlue).fg(Color::Black)` — a conventional inverted-selection color that is visible in both light and dark terminal themes. The gutter cells are never highlighted because `screen_to_buffer` rejects clicks in the gutter, so selection coordinates never include gutter columns.

### 8. Clear selection on buffer-modifying actions — `src/frontend/tui/app.rs` (Layer 2)

Any action that modifies the buffer (typing, delete, backspace, chord execution) should clear the active selection:

```rust
if modified {
    state.selection = None;
}
```

This is a single line after the existing `if buffer_modified { last_edit = Some(Instant::now()); }` block in `event_loop`. Mode switches (Ctrl-E) and cursor movement via keyboard also clear the selection — add `state.selection = None;` at the top of `handle_edit_mode` and `handle_chord_mode` for key events (but not for mouse events, which are handled separately).

### 9. Move cursor on click — `src/frontend/tui/app.rs` (Layer 2)

As a secondary benefit of mouse capture, a single left-click (mouse-down followed by mouse-up at the same position, or within a small threshold) should move the cursor to the clicked position:

In `handle_mouse`, on `MouseEventKind::Down`:

```rust
state.cursor_line = line;
state.cursor_col = col;
```

This gives the user click-to-position-cursor behavior. The selection is also initialized at this point, so if the user drags, it becomes a selection; if they release at the same spot, the zero-width selection produces an empty string and nothing is copied.

---

## Edge Case Considerations

- **Click in gutter area**: `screen_to_buffer` returns `None` when `x` falls within the line-number gutter. No selection is started; no cursor movement occurs. The gutter is defined as `x < area.x + line_num_width + 1`.

- **Click below last buffer line**: If the user clicks on a visual row past the last line of the buffer (empty space), clamp to the end of the last line. This prevents out-of-bounds access and gives intuitive behavior (cursor lands at EOF).

- **Click past end of a short line**: If the user clicks at display column 40 but the line is only 20 characters wide, clamp to the line's byte length. `byte_col_from_display` already handles this naturally — it stops iterating when characters run out.

- **Drag direction**: The `Selection::ordered()` method normalizes anchor/head so that `start <= end` regardless of whether the user dragged left-to-right or right-to-left, or top-to-bottom vs bottom-to-top.

- **Selection across soft-wrapped lines**: `screen_to_buffer` resolves each visual row back to the correct logical line and byte offset. A drag from visual row 2 of logical line 5 to visual row 0 of logical line 6 produces a selection spanning the correct byte ranges across two logical lines.

- **Selection with tabs**: Tabs display as 4 columns but occupy 1 byte. `screen_to_buffer` uses `byte_col_from_display` which correctly accounts for tab width when converting display coordinates to byte offsets.

- **Multi-byte UTF-8 characters**: `byte_col_from_display` iterates by character and accumulates `char::len_utf8()` bytes, so it always lands on a valid character boundary. Selection slicing uses byte offsets into the line string, which are guaranteed to be valid boundaries.

- **Tree panel visible**: When the file tree is visible and focused, mouse events in the tree area should be ignored by the editor selection handler. `screen_to_buffer` rejects coordinates outside the editor `area` Rect. If the tree is visible but not focused, the editor area is already offset by the tree's width in the layout.

- **OSC 52 unsupported terminal**: Some older terminals (notably some configurations of macOS Terminal.app) do not support OSC 52. The `write_osc52` function writes the escape sequence regardless — unsupported terminals silently ignore it. The `status_msg` can optionally be set to indicate the copy action, but detecting OSC 52 support at runtime is not reliably possible (there is no standard query/response for OSC 52 capability). Users of unsupported terminals fall back to Shift-drag for native selection.

- **Large selection / OSC 52 size limits**: Some terminals impose a maximum payload size for OSC 52 (e.g., xterm limits to 100,000 bytes of base64). For very large selections, the clipboard write may be silently truncated by the terminal. This is rare in practice and not worth adding application-side truncation logic for — if it becomes a problem, a future enhancement can split the payload or warn.

- **Mouse capture and Shift-drag fallback**: When mouse capture is enabled, virtually all modern terminals still honor Shift+click-drag as native selection. This is the standard escape hatch. Users who need to select terminal output (status bar, gutter) can hold Shift. This behavior is terminal-emulator-provided and requires no application code.

- **Selection during chord mode**: Selection works identically in both Edit and Chord modes. The mouse handler does not check the current mode — click-to-position and drag-to-select are always available.

- **Concurrent keyboard and mouse input**: If the user types while a selection is active (mouse button still held), the buffer modification clears the selection. If the user releases the mouse after the selection was cleared, `state.selection` is already `None`, so the mouse-up handler is a no-op.

- **Scroll offset changes during drag**: If the user drags to the top or bottom edge of the editor area, the scroll offset should ideally auto-scroll. This is a UX enhancement that can be deferred — initial implementation can simply clamp to visible lines. A follow-up can add auto-scroll by adjusting `scroll_offset` when `mouse.row` is at `area.y` (scroll up) or `area.bottom() - 1` (scroll down) during a drag event.

---

## Test Considerations

- **Unit: `Selection::ordered()` forward drag** — anchor (0, 5), head (2, 10) → returns (0, 5, 2, 10).

- **Unit: `Selection::ordered()` backward drag** — anchor (2, 10), head (0, 5) → returns (0, 5, 2, 10).

- **Unit: `Selection::ordered()` same-line backward** — anchor (3, 20), head (3, 5) → returns (3, 5, 3, 20).

- **Unit: `extract_selection_text` single line** — buffer `["hello world"]`, selection (0, 0) to (0, 5) → `"hello"`.

- **Unit: `extract_selection_text` multi-line** — buffer `["aaa", "bbb", "ccc"]`, selection (0, 1) to (2, 2) → `"aa\nbbb\ncc"`.

- **Unit: `extract_selection_text` clamped to line length** — buffer `["short"]`, selection (0, 0) to (0, 100) → `"short"`.

- **Unit: `extract_selection_text` empty selection** — anchor == head → `""`.

- **Unit: `screen_to_buffer` basic** — given a known `area`, `line_num_width`, and `scroll_offset`, verify a click at a specific (x, y) maps to the correct (line, col).

- **Unit: `screen_to_buffer` gutter click** — click at `x < area.x + line_num_width + 1` → returns `None`.

- **Unit: `screen_to_buffer` below buffer** — click at a y coordinate past the last line → clamps to last line's end.

- **Unit: `screen_to_buffer` with soft-wrap** — a long line wraps to 3 visual rows; a click on visual row 2 returns the correct byte offset into the logical line.

- **Unit: `screen_to_buffer` with tabs** — a line containing tabs; click at display column 8 returns the correct byte offset accounting for tab expansion.

- **Unit: `write_osc52` output** — capture stdout and verify the escape sequence matches `ESC]52;c;<base64>BEL` for a known input string.

- **Unit: selection cleared on buffer modify** — simulate a key event that modifies the buffer while `state.selection` is `Some(...)` → assert `state.selection` is `None` afterward.

- **Manual test checklist**:
  - Open a multi-line file in TUI; click and drag across several lines — selection highlight appears over text content only, not over line numbers
  - Release mouse — selected text is in system clipboard (paste into another app to verify), no line numbers present
  - Click a position in the middle of a line — cursor moves to that position
  - Click in the line-number gutter — nothing happens (no selection, no cursor move)
  - Shift-click-drag — native terminal selection activates (includes line numbers as before, confirming the fallback works)
  - Click and drag across a soft-wrapped line — selection correctly spans the visual rows and the clipboard contains the unwrapped logical line text
  - Select text containing tabs — clipboard contains actual tab characters, not spaces
  - Type while a selection is active — selection disappears and the keystroke is processed normally
  - Open the file tree (Ctrl-T) and click in the tree area — no editor selection is started

---

## Codebase Integration

- **Layer 0 change** (`src/data/state.rs`): Add `Selection` struct and `selection: Option<Selection>` field to `EditorState`. Pure data addition with no imports from higher layers.

- **Layer 2 changes**:
  - `src/frontend/tui/app.rs`: Enable/disable mouse capture in `run()`. Extend `event_loop` to handle `Event::Mouse`. Add `handle_mouse`, `extract_selection_text`, and `write_osc52` functions. Clear selection on buffer-modifying key events. Hoist or store editor render area so it's accessible in the event handler.
  - `src/frontend/tui/editor_pane.rs`: Add `screen_to_buffer` coordinate mapping function. Add selection highlight rendering pass after the paragraph widget is drawn.

- **New dependency**: `base64 = "0.22"` in `Cargo.toml` for OSC 52 payload encoding.

- Follow established conventions, best practices, testing, and architecture patterns from the project's aspec.
