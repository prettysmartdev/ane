# Using the TUI

ane's TUI is an interactive terminal editor with two modes, a file tree, syntax highlighting, and a chord input box.

---

## Opening the editor

```sh
ane .                   # open a directory (file tree + editor)
ane path/to/file.rs     # open a single file
```

Opening a directory shows the file tree on the left and the editor pane on the right. Opening a single file goes straight to the editor; press `Ctrl-T` to open the tree later.

---

## Modes

### Chord mode (default)

ane starts in Chord mode. A command box at the bottom accepts chord input:

- Type a chord and press **Enter** to execute it.
- Short-form chords (4 lowercase characters) **auto-execute** when the combination is valid for the current cursor position -- no Enter required.
- Arrow keys (when the chord input is empty) move the cursor in the editor.
- Arrow keys (when chord input has text) move within the chord input string.
- Press **Esc** to clear the chord input.

### Edit mode

Press `Ctrl-E` to toggle into Edit mode for direct text editing:

- Type normally to insert text at the cursor.
- Arrow keys navigate the cursor (including across soft-wrapped lines and between line boundaries).
- Press **Esc** or `Ctrl-E` to return to Chord mode.

---

## Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl-E` | any | Toggle between Edit mode and Chord mode |
| `Ctrl-T` | any | Toggle file tree pane |
| `Ctrl-S` | any | Save file |
| `Ctrl-R` | Chord mode | Recall previous chord. Press repeatedly to cycle through chord history. Press Enter to execute. |
| `Ctrl-C` | any | Exit ane (opens confirmation modal) |
| `Arrow keys` | Edit mode | Move cursor. Left/Right wrap across line boundaries. Up/Down navigate soft-wrapped visual lines. |
| `Arrow keys` | Chord mode (empty input) | Move cursor (same as Edit mode) |
| `Arrow keys` | Chord mode (with input) | Move within the chord input string |
| `Enter` | Chord mode | Execute chord |
| `Enter` | File tree | Open selected file |
| `Esc` | Edit mode | Return to Chord mode |
| `Esc` | Chord mode | Clear chord input |

### Keys intentionally not supported

ane does not use `h`/`j`/`k`/`l` for navigation, `i` to enter insert mode, or `q` to quit. These are reserved to avoid conflicts with chord input.

---

## File tree

The file tree appears on the left when you open a directory, or when you press `Ctrl-T`.

- **Up/Down arrows** navigate the tree.
- **Left/Right arrows** expand/collapse directories.
- **Enter** opens the selected file in the editor pane.
- **Ctrl-T** toggles the tree on and off.

When the tree is focused, editor keybindings are suspended. Focus returns to the editor when you open a file or toggle the tree off.

---

## LSP status

The status bar at the bottom shows the language server state:

| Status | Meaning |
|--------|---------|
| ready | LSP running, all chords available |
| starting | LSP initializing, LSP-scoped chords wait |
| installing | Language server being auto-installed |
| not installed | No language server found for this project |
| failed | LSP encountered an error |

Non-LSP chords (Line, Buffer, Delimiter) work immediately regardless of LSP status.

---

[<- Chord Examples](02-chord-examples.md) | [Next: Exec Mode ->](04-exec-mode.md)
