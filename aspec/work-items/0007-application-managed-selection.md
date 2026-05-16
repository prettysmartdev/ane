# Work Item: Feature

Title: Application-managed mouse selection with gutter-excluded clipboard
Issue: N/A

## Summary

Replace the terminal emulator's native text selection with application-managed mouse selection so that line-number gutters are excluded from the user's clipboard. When the terminal enters mouse capture mode, mouse events are forwarded to the application instead of being handled by the terminal's built-in selection. The editor tracks selection state (anchor + head) in logical buffer coordinates, renders highlighted selection ranges, and copies the selected buffer text to the system clipboard via the OSC 52 escape sequence on explicit `Ctrl-Y`. The selection persists across mouse-up and across non-buffer-modifying key events, so the user can review what is selected before deciding to copy, type to replace it (Edit mode only), or dismiss it. Because selection coordinates are expressed in buffer space, the gutter is never part of the copied content. The user can still fall back to native terminal selection by holding Shift during a click-drag (standard terminal convention).

---

## User Stories

### User Story 1
As a: developer

I want to: click and drag across lines in the editor pane, then press `Ctrl-Y` to copy only the file's text content (no line numbers) to my clipboard

So I can: paste code into other tools, chat windows, or files without manually stripping line-number prefixes, while keeping copy as an explicit action I can preview before committing

### User Story 2
As a: developer

I want to: see a visual highlight over the text I'm selecting as I drag the mouse, and have that highlight persist after I release the mouse button

So I can: confirm exactly which range of text will be copied and have time to review it before pressing `Ctrl-Y`

### User Story 3
As a: developer

I want to: hold Shift and click-drag to fall back to native terminal selection when I need to copy raw terminal output (including line numbers or status bar text)

So I can: retain the escape hatch for edge cases where application-managed selection is not what I need

### User Story 4
As a: developer in Edit mode

I want to: select text with the mouse and then type a character (or press Tab, Enter, or Backspace) to replace the selection with what I type, like in any GUI text editor

So I can: edit code with standard word-processor semantics instead of having to manually delete the selection before typing

### User Story 5
As a: developer

I want to: see a hint in the status bar whenever text is selected, telling me which key to press to copy

So I can: discover the copy keybinding without consulting documentation

### User Story 6
As a: developer using a terminal that does not support OSC 52

I want to: understand that I can fall back to Shift-drag native selection if my terminal does not honor OSC 52

So I can: still copy text from the editor in environments where OSC 52 is silently ignored

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
2. Compute `text_col = (x - gutter_end) as usize` — the display column within the text region, where `gutter_end = area.x + line_num_width + 1`.
3. Walk logical lines from `scroll_offset`, accumulating visual rows (using `visual_row_count`), until the accumulated row count exceeds `(y - area.y) as usize`. This identifies the logical line and which visual (wrapped) row within it the click landed on.
4. Compute the target display column: `wrap_row_start(&offsets, wrap_row_within_line) + text_col`. This uses `wrap_offsets` to account for word-wrap breaking at variable positions (space boundaries), not at fixed `text_width` intervals.
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

The `editor_render_area` `Rect` must be available in the event loop. The implementation factors the shared layout math into `compute_pane_layout()` returning a `PaneLayout` struct; both `draw()` and the event loop's `compute_editor_render_area()` consume it, avoiding duplicated horizontal+vertical constraint code.

Implement `handle_mouse`:

```rust
fn handle_mouse(state: &mut EditorState, mouse: MouseEvent, editor_area: Rect) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some((line, col)) = editor_pane::screen_to_buffer(
                mouse.column, mouse.row, editor_area, state,
            ) {
                state.cursor_line = line;
                state.cursor_col = col;
                state.selection = Some(Selection {
                    anchor_line: line,
                    anchor_col: col,
                    head_line: line,
                    head_col: col,
                });
            }
        }
        MouseEventKind::Drag(MouseButton::Left) if state.selection.is_some() => {
            if let Some((line, col)) = editor_pane::screen_to_buffer(
                mouse.column, mouse.row, editor_area, state,
            ) {
                let sel = state.selection.as_mut().unwrap();
                sel.head_line = line;
                sel.head_col = col;
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Persist the selection so the user can decide whether to copy
            // (Ctrl-Y), replace by typing in Edit mode, or dismiss (Esc / new
            // click). The one case we collapse here is a zero-width selection
            // from a single click without a drag.
            if let Some(sel) = state.selection
                && sel.anchor_line == sel.head_line
                && sel.anchor_col == sel.head_col
            {
                state.selection = None;
            }
        }
        _ => {}
    }
}
```

The selection persists across mouse-up. Copying is a separate, explicit gesture (`Ctrl-Y`), described in section 6.

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
        let from = if i == start_line {
            snap_to_char_boundary(line, start_col)
        } else {
            0
        };
        let to = if i == end_line {
            snap_to_char_boundary(line, end_col)
        } else {
            line.len()
        };
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

Uses byte-offset `start_col`/`end_col` (the same coordinate space as `cursor_col`) to slice directly into the buffer lines. Multi-line selections include newlines between lines but not a trailing newline. Defensive `snap_to_char_boundary` calls guarantee the slice never panics even if a `Selection` is ever constructed at a non-boundary byte offset.

### 6. OSC 52 clipboard write and `Ctrl-Y` keybinding — `src/frontend/tui/app.rs` (Layer 2)

The clipboard write is invoked from a dedicated `Ctrl-Y` priority handler in `handle_key`, not from mouse-up. This decouples select-from-copy so the user can preview the selection before committing.

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

In `handle_key`, registered after the existing `Ctrl-T` / `Ctrl-S` priority handlers:

```rust
// Priority 3c: Ctrl-Y yanks the active selection to the clipboard.
// Selection is cleared after copying so the highlight and status hint disappear.
if code == KeyCode::Char('y') && modifiers.contains(KeyModifiers::CONTROL) {
    if let Some(sel) = state.selection {
        let text = extract_selection_text(state, &sel);
        if !text.is_empty() {
            write_osc52(&text);
            state.status_msg = format!("copied {} chars", text.chars().count());
        }
        state.selection = None;
    }
    return false;
}
```

OSC 52 format: `ESC ] 52 ; c ; <base64-payload> BEL`. The `c` parameter targets the clipboard selection (as opposed to `p` for primary selection on X11). This sequence is supported by iTerm2, kitty, Alacritty, WezTerm, Windows Terminal, foot, and most modern terminal emulators.

The `Ctrl-Y` choice avoids platform conflicts: `Cmd-C` is intercepted by the macOS terminal emulator and never reaches a TUI; `Ctrl-C` is already bound to the exit modal. `Ctrl-Y` is unused elsewhere in ane and is the standard "yank" mnemonic from vim/Emacs traditions.

Add the `base64` crate to `Cargo.toml` (gated behind the `frontends` feature, since OSC 52 is TUI-only):

```toml
[dependencies]
base64 = { version = "0.22", optional = true }

[features]
frontends = ["dep:base64", "dep:clap", "dep:crossterm", "dep:ratatui"]
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

### 8. Selection lifecycle — `src/frontend/tui/app.rs` (Layer 2)

The selection is persistent across key events; it is only cleared in the following situations:

1. **Buffer-modifying key event in the event loop.** After `handle_key` returns, if `buffer_modified == true`, the event loop sets `state.selection = None`:

   ```rust
   if buffer_modified {
       state.selection = None;
       if let Some(buf) = state.current_buffer() {
           syntax_engine.compute(&buf.path, &buf.content());
       }
   }
   ```

2. **In Edit mode, the four buffer-mutating keys delete the selection first, then perform their normal insert.** This implements standard word-processor "type to replace selection" semantics for `Char`, `Tab`, `Enter`, and `Backspace`:

   ```rust
   KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
       modified |= delete_selection(state);
       // ...existing insert-at-cursor logic; cursor is now at the
       // deletion site so the insert lands in the right place...
   }
   KeyCode::Backspace => {
       // Backspace on an active selection deletes the selection only —
       // no extra character removal.
       if state.selection.is_some() {
           modified |= delete_selection(state);
       } else {
           // ...existing char/line-join delete logic...
       }
   }
   ```

   `delete_selection(state)` removes the selected range from the active buffer (handling both single-line `drain` and multi-line splice with line joining), places the cursor at the start of the deleted range, clears `state.selection`, and returns `true` if any content was removed.

3. **`Esc` in either mode** explicitly dismisses the selection (without copying), in addition to its existing behavior.

4. **`Ctrl-Y`** clears the selection after writing it to the clipboard.

5. **New mouse-down** overwrites `state.selection` with a fresh zero-width selection at the click position; mouse-up then collapses any zero-width selection back to `None`.

**Chord mode never modifies the selection in response to key events.** Typing characters in Chord mode appends them to `chord_input`; the selection is preserved so the user can still copy or replace it after switching back to Edit mode with `Ctrl-E`. (If a chord execution from Chord mode modifies the buffer, the existing `buffer_modified` path will clear the selection on its own.)

### 9. Status-bar hint — `src/frontend/tui/status_bar.rs` (Layer 2)

When `state.selection.is_some()`, render a hint span on the right side of the status bar reminding the user of the copy keybinding:

```rust
let hint_text: &str = if state.selection.is_some() {
    " Ctrl-Y: copy "
} else {
    ""
};
```

The hint is styled `Color::Black` on `Color::LightBlue` with `Modifier::BOLD`, matching the selection-highlight color so the two read as related. The hint width is subtracted from `available` before truncating the status message, so a long status message will be truncated before the hint is dropped.

### 10. Move cursor on click — `src/frontend/tui/app.rs` (Layer 2)

As a secondary benefit of mouse capture, a single left-click (mouse-down followed by mouse-up at the same position) moves the cursor to the clicked position:

In `handle_mouse`, on `MouseEventKind::Down`:

```rust
state.cursor_line = line;
state.cursor_col = col;
```

This gives the user click-to-position-cursor behavior. The selection is also initialized at this point as a zero-width range; if the user drags, it becomes a real selection; if they release at the same spot, mouse-up collapses the zero-width selection back to `None`.

---

## Edge Case Considerations

- **Click in gutter area**: `screen_to_buffer` returns `None` when `x` falls within the line-number gutter. No selection is started; no cursor movement occurs. The gutter is defined as `x < area.x + line_num_width + 1`.

- **Click below last buffer line**: If the user clicks on a visual row past the last line of the buffer (empty space), clamp to the end of the last line. This prevents out-of-bounds access and gives intuitive behavior (cursor lands at EOF).

- **Click past end of a short line**: If the user clicks at display column 40 but the line is only 20 characters wide, clamp to the line's byte length. `byte_col_from_display` already handles this naturally — it stops iterating when characters run out.

- **Drag direction**: The `Selection::ordered()` method normalizes anchor/head so that `start <= end` regardless of whether the user dragged left-to-right or right-to-left, or top-to-bottom vs bottom-to-top.

- **Selection across soft-wrapped lines**: `screen_to_buffer` resolves each visual row back to the correct logical line and byte offset using `wrap_offsets` (which performs word-wrap at space boundaries, falling back to character-break for long words). A drag from visual row 2 of logical line 5 to visual row 0 of logical line 6 produces a selection spanning the correct byte ranges across two logical lines. The selection highlight in `render_selection_highlight` also uses `wrap_offsets` to compute per-row display column ranges, ensuring the highlight aligns with the rendered wrap positions.

- **Selection with tabs**: Tabs display as 4 columns but occupy 1 byte. `screen_to_buffer` uses `byte_col_from_display` which correctly accounts for tab width when converting display coordinates to byte offsets.

- **Multi-byte UTF-8 characters**: `byte_col_from_display` iterates by character and accumulates `char::len_utf8()` bytes, so it always lands on a valid character boundary. Selection slicing uses byte offsets into the line string, which are guaranteed to be valid boundaries.

- **Tree panel visible**: When the file tree is visible and focused, mouse events in the tree area should be ignored by the editor selection handler. `screen_to_buffer` rejects coordinates outside the editor `area` Rect. If the tree is visible but not focused, the editor area is already offset by the tree's width in the layout.

- **OSC 52 unsupported terminal**: Some older terminals (notably some configurations of macOS Terminal.app) do not support OSC 52. The `write_osc52` function writes the escape sequence regardless — unsupported terminals silently ignore it. The `Ctrl-Y` handler always sets `status_msg = "copied N chars"` on a non-empty selection, even when the terminal silently drops the OSC 52 sequence — detecting OSC 52 support at runtime is not reliably possible (there is no standard query/response for OSC 52 capability). Users of unsupported terminals fall back to Shift-drag for native selection.

- **Large selection / OSC 52 size limits**: Some terminals impose a maximum payload size for OSC 52 (e.g., xterm limits to 100,000 bytes of base64). For very large selections, the clipboard write may be silently truncated by the terminal. This is rare in practice and not worth adding application-side truncation logic for — if it becomes a problem, a future enhancement can split the payload or warn.

- **Mouse capture and Shift-drag fallback**: When mouse capture is enabled, virtually all modern terminals still honor Shift+click-drag as native selection. This is the standard escape hatch. Users who need to select terminal output (status bar, gutter) can hold Shift. This behavior is terminal-emulator-provided and requires no application code.

- **Selection during chord mode**: Selection creation works identically in both Edit and Chord modes — the mouse handler does not check the current mode. However, "type to replace selection" semantics only apply in Edit mode: in Chord mode, character keys go to the chord input box and the selection is left untouched, so the user can switch back to Edit mode (`Ctrl-E`) with the selection still visible and then act on it.

- **Type to replace selection (Edit mode)**: In Edit mode, pressing any buffer-mutating key (`Char`, `Tab`, `Enter`, `Backspace`) while a selection is active first deletes the selection and then performs the insert at the selection's start position, matching standard GUI text-editor semantics. `Backspace` on an active selection deletes only the selection (no additional character removal). For multi-line selections, the spliced-and-joined line correctly preserves the suffix of the last selected line.

- **Concurrent keyboard and mouse input**: If the user types a buffer-modifying key in Edit mode while a selection is active (whether or not the mouse button is held), the selection is replaced by what was typed. Non-modifying keys (arrow keys, `Ctrl-S`, `Ctrl-T`, `Ctrl-E`) leave the selection intact. If the mouse is released after the selection has been cleared by a key event, the mouse-up handler sees `state.selection == None` and does nothing.

- **Scroll offset changes during drag**: If the user drags to the top or bottom edge of the editor area, the scroll offset should ideally auto-scroll. This is a UX enhancement that can be deferred — initial implementation can simply clamp to visible lines. A follow-up can add auto-scroll by adjusting `scroll_offset` when `mouse.row` is at `area.y` (scroll up) or `area.bottom() - 1` (scroll down) during a drag event.

---

## Test Considerations

- **Unit: `Selection::ordered()` forward drag** — anchor (0, 5), head (2, 10) → returns (0, 5, 2, 10). Lives in `src/data/state.rs` per the project's "tests inside source file" convention.

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

- **Unit: `write_osc52` output** — verify the OSC 52 escape sequence matches `ESC]52;c;<base64>BEL` for a known input string.

- **Unit: selection cleared after a buffer-modifying key** — mirror the event-loop path: dispatch a character key in Edit mode while a selection is active, observe `buffer_modified == true`, then clear the selection.

- **Unit: selection persists across non-modifying key** — pressing `Left` arrow in Edit mode does not clear the selection.

- **Unit: `Ctrl-Y` copies selection and clears it** — invoke `handle_key` with `Ctrl-Y` and a 5-char selection → selection becomes `None`, `status_msg == "copied 5 chars"`, handler returns `false` (no buffer modification).

- **Unit: `Ctrl-Y` with empty selection is a no-op** — no selection, no status message, no panic.

- **Unit: typing in Edit mode with selection replaces selection** — buffer `["hello world"]`, selection (0,0)-(0,5), press `X` → buffer becomes `"X world"`, cursor at (0, 1), selection cleared.

- **Unit: `Backspace` in Edit mode with selection deletes only the selection** — selection (0,5)-(0,11) over `"hello world"`, press `Backspace` → buffer becomes `"hello"`, cursor at (0, 5).

- **Unit: `Enter` in Edit mode with selection replaces with newline** — selection (0,5)-(0,6) over `"hello world"` (the space), press `Enter` → buffer becomes `["hello", "world"]`, cursor at (1, 0).

- **Unit: `Tab` in Edit mode with selection replaces with tab** — selection (0,1)-(0,4) over `"abcdef"`, press `Tab` → buffer becomes `"a\tef"`, cursor at (0, 2).

- **Unit: multi-line selection delete via type-to-replace** — buffer `["aaa", "bbb", "ccc"]`, selection (0,1)-(2,2), press `Z` → buffer collapses to `["aZc"]`, cursor at (0, 2).

- **Unit: `Ctrl-E` with selection switches mode but keeps selection** — Edit mode + selection + `Ctrl-E` → mode is now `Chord`, selection is still `Some`.

- **Unit: typing in Chord mode does not touch selection or buffer** — Chord mode + selection + keypress `q` → `chord_input == "q"`, buffer unchanged, selection still `Some`.

- **Manual test checklist**:
  - Open a multi-line file in TUI; click and drag across several lines — selection highlight appears over text content only, not over line numbers
  - Release mouse — highlight stays, status bar shows " Ctrl-Y: copy " hint
  - Press `Ctrl-Y` — highlight disappears, status shows "copied N chars", paste into another app to verify clipboard contents (no line numbers present)
  - Single-click at a position — cursor moves there, no lingering selection or hint
  - Click in the line-number gutter — nothing happens (no selection, no cursor move)
  - Shift-click-drag — native terminal selection activates (includes line numbers as before, confirming the fallback works)
  - Click and drag across a soft-wrapped line — selection correctly spans the visual rows; `Ctrl-Y` copies the unwrapped logical line text
  - Select text containing tabs and `Ctrl-Y` — clipboard contains actual tab characters, not spaces
  - In Edit mode, select some text and type a character — selection is replaced by that character (word-processor semantics); cursor lands at the insert point
  - In Edit mode, select some text and press Backspace — selection is deleted, no extra char removed
  - In Edit mode, select some text and press Enter or Tab — selection is replaced by a newline / tab
  - In Edit mode with a selection, press an arrow key — selection stays visible
  - In Chord mode, select some text and type chord letters — selection stays visible, chord input fills as normal
  - Select some text and press `Esc` — selection clears without copying
  - Open the file tree (`Ctrl-T`) and click in the tree area — no editor selection is started

---

## Codebase Integration

- **Layer 0 change** (`src/data/state.rs`): Add `Selection` struct and `selection: Option<Selection>` field to `EditorState`. Pure data addition with no imports from higher layers. `Selection::ordered()` tests live in the `#[cfg(test)] mod tests` block of the same file.

- **Layer 2 changes**:
  - `src/frontend/tui/app.rs`: Enable/disable mouse capture in `run()`. Extend `event_loop` to handle `Event::Mouse`, with a single `compute_pane_layout()` helper feeding both `draw()` and the event loop's `compute_editor_render_area()` so layout math is not duplicated. Add `handle_mouse`, `extract_selection_text`, `write_osc52`, and `delete_selection`. Wire `delete_selection` into the four buffer-mutating handlers in Edit mode (`Char`, `Tab`, `Enter`, `Backspace`) for word-processor "type to replace selection" semantics. Add a `Ctrl-Y` priority handler in `handle_key` that yanks via OSC 52. Clear selection only on (a) buffer-modifying key events, (b) `Esc`, (c) `Ctrl-Y`, (d) new mouse-down.
  - `src/frontend/tui/editor_pane.rs`: Add `screen_to_buffer` coordinate mapping function. Add `render_selection_highlight` pass after the paragraph widget is drawn, using `wrap_offsets` so the highlight aligns with rendered soft-wrap positions.
  - `src/frontend/tui/status_bar.rs`: Render a " Ctrl-Y: copy " hint span (black on light-blue, bold) whenever `state.selection.is_some()`.

- **New dependency**: `base64 = "0.22"` in `Cargo.toml`, gated behind the `frontends` feature so the no-frontends/test-support build does not pull it in.

- Follow established conventions, best practices, testing, and architecture patterns from the project's aspec.
