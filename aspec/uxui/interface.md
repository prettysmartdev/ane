# User Interface

## Style

Aesthetic:
- Minimal, terminal-native design
- Clean borders, subtle color accents, no chrome or decoration beyond what aids navigation
- Cyan accent color for active elements, dark gray for inactive

Brand and colors:
- Primary accent: Cyan
- Active text: White
- Inactive text: Gray
- Line numbers: DarkGray (Yellow + Bold for current line)
- Directory entries: Blue
- Dirty buffer indicator: `[+]` in title bar
- LSP status: Green (ready), Yellow (starting/installing), Red (failed/not installed), DarkGray (unknown)
- Mode indicator: Cyan background with black text (CHORD / EDIT)

Desktop vs mobile:
- Terminal-only — no GUI, no mobile. Works in any terminal emulator that supports 256 colors

## Usage

Layout:
- Three-part vertical layout: content area + command bar (3 rows) + status bar (1 row)
- Content area: two-pane horizontal layout when file tree is visible (25% tree, 75% editor), single-pane editor otherwise
- Command bar: shows chord input prompt in Chord mode, mode indicator in Edit mode
- Status bar: mode badge, filename, cursor position, LSP status

Modes:
- **Chord mode** (default): command text box at bottom accepts chord input. Type a chord and press Enter to execute. Esc clears input.
- **Edit mode**: direct text editing with cursor. Characters typed go into the buffer.
- Toggle between modes with `Ctrl-E`

Keybindings:
- `Ctrl-E`: toggle between Edit mode and Chord mode
- `Ctrl-T`: toggle file tree pane focus. If opened with single file and no tree exists, initiates a directory scan and shows the tree.
- `Ctrl-C`: opens exit confirmation modal (press Ctrl-C again to exit, Esc to cancel)
- `Ctrl-S`: save current file (Edit mode)
- Arrow keys: navigate (both modes)
- `Tab`: insert tab character (Edit mode only; not a keybinding)
- `Enter`: execute chord (Chord mode) / open file from tree (tree focused) / newline (Edit mode)
- `Backspace`: delete character (Edit mode) / delete last chord character (Chord mode)
- `Esc`: clear chord input and status message (Chord mode)

Removed keybindings:
- No `h/j/k/l` navigation — arrow keys only
- No `i` to enter insert mode — `Ctrl-E` is the only mode toggle
- No `q` to quit — `Ctrl-C` with confirmation modal only

Empty states:
- Editor pane shows welcome message with project name and key bindings when no file is open
- "Select a file from the tree to begin editing" prompt
- "Ctrl-E to toggle Edit mode | Ctrl-T to open file tree"

Exit modal:
- Triggered by Ctrl-C
- Centered modal overlay: "Press Ctrl-C again to exit / Press Esc to cancel"
- Red border for visual urgency

Accessibility:
- Fully keyboard-driven — no mouse required
- Works with screen readers that support terminal applications

Machine use:
- The exec subcommand is the primary machine interface
- Designed for minimal token consumption by AI code agents
- Outputs standard unified diff format that agents already understand
