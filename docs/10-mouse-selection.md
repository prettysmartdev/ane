# Mouse Selection

ane provides application-managed mouse selection that excludes line-number gutters from clipboard content, with visual feedback and support for modern terminal emulators.

---

## Overview

When you open ane in the TUI, mouse capture is automatically enabled. This means:

- **Click and drag** to select text in the editor — selection is highlighted in real-time
- **Release the mouse** to copy the selected text to your system clipboard
- **Line numbers are never included** in the clipboard content, even though they're visible on screen
- **Shift-click-drag** to fall back to native terminal selection (includes line numbers and status bar text if needed)

This solves a common workflow friction: selecting code to paste elsewhere without manually stripping away line-number prefixes.

---

## Basic usage

### Click to position cursor

A single left-click moves the cursor to that position in the editor:

```
┌─────┬──────────────────────┐
│ 1   │ fn main() {          │  ← Click here
│ 2   │     println!("hi");  │
│ 3   │ }                    │
└─────┴──────────────────────┘
        ↑ Cursor moves to this position
```

Clicks in the line-number gutter (the left margin) are ignored — the cursor does not move.

### Click and drag to select

Click at the start of the text you want, hold the mouse button, drag to the end, then release:

```
┌─────┬──────────────────────┐
│ 1   │ fn main() {          │
│ 2   │ ┌─ println!("hi");  │← Drag endpoint
│ 3   │ │                    │
└─────┴─┴──────────────────────┘
  ↑ Click start
```

As you drag, the selected text is **highlighted in blue** so you can see exactly what will be copied.

When you release the mouse, the selected text is **automatically copied to your system clipboard**. You can then paste it into another application, chat window, or file.

**The clipboard contains only the text content** — line numbers are excluded because the selection spans buffer coordinates, not screen pixels.

### Selection highlight

Selected text appears with a **light blue background and black foreground** while the mouse button is held:

```
┌─────┬──────────────────────┐
│ 1   │ fn main() {          │
│ 2   │     ░░░░░░░░░("hi"); │← Selection highlight
│ 3   │ }                    │
└─────┴──────────────────────┘
```

The highlight disappears immediately after you release the mouse, and the clipboard copy completes.

---

## Selection across multiple lines

When you drag across multiple lines, the selection includes all text from the anchor point to the head point, with newlines between lines:

```
┌─────┬──────────────────────┐
│ 1   │ fn main() {          │
│ 2   │ ┌─ println!("a");    │
│ 3   │ │ println!("b");     │
│ 4   │ └─ println!("c");    │
│ 5   │ }                    │
└─────┴──────────────────────┘
```

Clipboard content:
```
println!("a");
println!("b");
println!("c");
```

**No leading or trailing newline** is added — the copied text matches exactly what you see selected on screen (excluding line numbers).

---

## Drag direction

Selection works identically whether you drag **left-to-right**, **right-to-left**, **top-to-bottom**, or **bottom-to-top**. The selection always normalizes to anchor (start) and head (end) order:

```
Drag right-to-left:           Drag bottom-to-top:
┌─────┬──────────────────┐   ┌─────┬──────────────────┐
│ 1   │ hello world ◄─   │   │ 1   │ aaa               │
└─────┴──────────────────┘   │ 2   │ bbb ◄──┐          │
                             │ 3   │ ccc    │ (drag end)
                             └─────┴────────┘
Same result in both cases:
Selection = "hello w"        Selection = "bbb\nccc"
```

---

## Soft-wrapped lines

ane supports soft-wrap — long lines that exceed the editor width are visually wrapped to multiple rows, but remain a single logical line in the buffer. Mouse selection works correctly across wrapped visual rows:

```
┌─────┬────────────────────────────────┐
│ 1   │ This is a very long line that  │  ← Visual row 1 of logical line 1
│     │ wraps across the editor width. │  ← Visual row 2 of logical line 1
│ 2   │ Another line here.             │  ← Visual row 1 of logical line 2
└─────┴────────────────────────────────┘
```

If you drag from visual row 2 of line 1 to visual row 1 of line 2, the selection spans the correct text and the clipboard contains the unwrapped content:

```
wraps across the editor width.
Another line here.
```

The soft-wrap positions are resolved correctly so selection boundaries land at the intended character, not at an arbitrary column.

---

## Special characters and Unicode

### Tabs

Tabs display as 4 columns visually, but the clipboard contains actual tab characters (`\t`), not spaces:

```
┌─────┬────────────────────┐
│ 1   │ fn foo() {         │
│ 2   │ ┌─ x = 5;         │
│ 3   │ └─ y = 10;        │
└─────┴────────────────────┘
      (Click before x, drag to after 10)

Clipboard: "fn foo() {\n\t\tx = 5;\n\t\ty = 10;"
         (leading tabs preserved, not converted to spaces)
```

### Multi-byte UTF-8

ane correctly handles multi-byte Unicode characters. Selection respects character boundaries, and the clipboard receives valid UTF-8:

```
┌─────┬──────────────────────┐
│ 1   │ Café Münster 🎉     │
│ 2   │ résumé complete      │
└─────┴──────────────────────┘
      (Select "é M" and emoji)

Clipboard: "é M🎉"
           (multi-byte sequences preserved)
```

---

## Cursor movement and mode behavior

### Click behavior in Edit vs Chord mode

Mouse click-to-position-cursor works in **both Edit mode and Chord mode**:

- **Edit mode** (`Ctrl-E` active): Click moves cursor; drag starts a selection
- **Chord mode** (default): Click moves cursor; drag starts a selection

No mode change occurs on click — you remain in the same mode.

### Selection is cleared on buffer modification

If you type, execute a chord, delete, or otherwise modify the buffer while a selection is active, the selection is immediately cleared:

```
1. Click and hold, drag to select text
2. Release (selection copied to clipboard)
3. Type a keystroke or execute a chord
   → Selection highlight disappears, next edit uses the normal cursor
```

This prevents the selection range from becoming stale if the buffer length changes.

### Cursor positioning after selection

When you release the mouse after a selection, the **cursor does not move** — it remains at its previous position. The selection is purely for clipboard copying, not for cursor positioning:

```
Current cursor: line 5
Click at line 10 and drag to line 12 (select and copy)
Release mouse
→ Cursor is still at line 5
→ Clipboard contains lines 10-12 text
```

To move the cursor to a clicked position, perform a single click (press and release immediately without dragging).

---

## Fallback to native terminal selection

If ane's application-managed selection does not suit your workflow, you can fall back to your terminal emulator's native text selection by holding **Shift during a click-drag**:

```
Shift-click-drag
→ Terminal's native selection activates
→ Selection includes line numbers, status bar, etc.
→ Terminal handles the clipboard copy
```

This escape hatch preserves the standard terminal selection behavior for cases where you need it. It works in all modern terminal emulators (iTerm2, kitty, Alacritty, WezTerm, Windows Terminal, macOS Terminal.app, etc.).

---

## Clipboard integration

### OSC 52 escape sequence

ane writes the selected text to your system clipboard using the **OSC 52** escape sequence, a standard supported by modern terminal emulators:

- **iTerm2** ✓
- **kitty** ✓
- **Alacritty** ✓
- **WezTerm** ✓
- **Windows Terminal** ✓
- **foot** ✓
- **Some macOS Terminal.app versions** — may not support OSC 52; use Shift-drag fallback instead

The clipboard write happens automatically on mouse-up. If your terminal does not support OSC 52, the clipboard write is silently ignored. **Use Shift-click-drag to fall back to native terminal selection.**

### No clipboard manager required

The clipboard write uses the OSC 52 protocol, which your terminal emulator handles. You do **not** need a separate clipboard manager running in the terminal.

---

## Edge cases and limitations

### Click in the gutter (line numbers)

Clicks in the line-number gutter are ignored:

```
┌─────┬────────────────┐
│     ← Click here = no effect
│ 1   │ Code here      │
│ 2   │ More code      │
└─────┴────────────────┘
```

Click in the text area (to the right of the gutter) to position the cursor or start a selection.

### Click below the last line

If you click on empty space below the last line of the buffer, the click is clamped to the end of the last line:

```
┌─────┬────────────────┐
│ 1   │ Final line     │
│     │ (empty space)  ← Click here
└─────┴────────────────┘
        Cursor moves to end of line 1
```

### Click past the end of a short line

If you click at column 50 but the line is only 20 characters wide, the click is clamped to the end of that line:

```
┌─────┬──────────────────────┐
│ 1   │ Short    ← Cursor lands here (line end)
│ 2   │ Another line         │
└─────┴──────────────────────┘
              ↑ Click at column 50
```

### File tree visible

When the file tree is visible and you click in the tree area, the click is handled by the tree (focus or file open). It does not trigger editor selection.

When the tree is visible but not focused, the editor area is offset to the right, and clicks in the editor area work normally.

### Large selections and terminal limits

Some terminals (notably xterm) impose a maximum payload size for OSC 52 (e.g., 100,000 bytes of base64). For very large selections, the clipboard write may be silently truncated by the terminal. This is rare in practice; if it becomes a problem, use Shift-drag to select the text in smaller chunks or consider your terminal's settings.

---

## Interaction with other features

### LSP and selection

Selection is independent of LSP status and language server features. You can select text from any file, regardless of whether a language server is running or available.

### File tree and selection

When the file tree pane is open, focus can be in either the tree or the editor. Mouse selection works only in the editor pane. Clicks in the tree are routed to the tree navigator.

### Syntax highlighting and selection

The selection highlight (light blue background) is applied on top of syntax highlighting. The highlight color is chosen to be visible in both light and dark terminal themes.

---

## Keyboard shortcuts

Mouse selection does not use keyboard shortcuts — it is driven entirely by mouse events. The related keybindings are:

| Key | Action |
|-----|--------|
| `Ctrl-E` | Toggle between Edit and Chord mode (selection behavior unchanged) |
| `Ctrl-T` | Toggle file tree (selection continues to work in editor area) |
| `Ctrl-S` | Save file (selection is cleared) |

---

## Tips and best practices

### Selecting across wrapped lines

Because soft-wrapping is transparent to the selection mechanism, you can reliably select across visual rows without worrying about where the wrap boundaries fall. The logical line structure is preserved.

### Copying without intermediate steps

Click-drag-release is a single gesture — you don't need to press copy keys or interact with a context menu. The text is in your clipboard immediately after release.

### Fallback workflow

If selection highlight is not clearly visible in your terminal theme, or if you prefer native terminal selection, hold Shift during click-drag and use your terminal's native selection instead. Both workflows are equally supported.

### Mixed keyboard and mouse

You can use keyboard navigation to position the cursor, then use mouse selection for a quick copy. The two input methods work together naturally.

---

## Troubleshooting

### Selection highlight is not visible

- Check your terminal theme and color contrast settings
- Try Shift-click-drag for native terminal selection (light/dark theme independent)
- Verify mouse capture is enabled (it is by default on startup)

### Clipboard content includes line numbers

- You may have used Shift-click-drag, which activates native terminal selection
- Use regular click-drag without Shift to use ane's gutter-excluding selection

### Clipboard write doesn't work

- Your terminal may not support OSC 52
- Try a different terminal emulator, or upgrade your current one
- Fall back to Shift-click-drag for native selection, or copy/paste text through other means

### Selection disappears unexpectedly

- If you type or execute a command while holding the mouse button, the selection is cleared
- Release the mouse to finalize the clipboard copy before interacting with other parts of the editor

---

## See also

- [Using the TUI](03-using-the-tui.md) — Overview of editor modes and keybindings
- [Getting Started](00-getting-started.md) — First steps with ane
