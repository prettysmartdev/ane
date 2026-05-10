# ane

**A New Editor** / **Agent Native Editor**

A modern terminal editor built for humans and code agents. ane is a pure-Rust ratatui terminal app that produces a single statically-linked binary.

## Goals

1. **Chord-native editing** — a 4-part chord system (action, positional, scope, component) for expressive, composable editing operations
2. **Agent-native interface** — a headless `exec` mode that lets AI code agents read and modify files with minimal token usage, outputting standard unified diffs
3. **LSP-integrated** — native language server integration for language-aware chords (starting with Rust/rust-analyzer)

## Chord System

ane chords have 4 parts: **action**, **positional**, **scope**, **component**.

| Part | Examples | Description |
|------|----------|-------------|
| Action | `c`hange, `d`elete, `r`ead, `i`nsert, `m`ove, `s`elect, `y`ank | What to do |
| Positional | `i`n, `a`t, `r`(a)round, `b`efore, `f`(a)fter | Where relative to scope |
| Scope | `f`unction, `v`ariable, `b`lock, `l`ine, `F`ile, `s`truct, `m`(i)mpl, `e`num | What language construct |
| Component | `b`ody, `n`ame, `s`ignature, `p`arameters, `t`ype, `v`alue, `a`ll | Which part of the scope |

**Examples:**
- `cifb my_func` → **C**hange **I**n **F**unction **B**ody (short form)
- `ChangeInFunctionBody my_func` → same chord, long form
- `dala 5` → **D**elete **A**t **L**ine **A**ll (delete line 5)

Chords that target language constructs (function, variable, struct, etc.) require an active LSP connection. Line and file operations work without LSP.

### Frontend-Aware Execution

Chords behave differently depending on the frontend:

- **CLI (`ane exec`)**: Change accepts replacement text as a parameter, outputs a unified diff
- **TUI**: Change deletes the target content, positions the cursor, and enters Edit mode

Each action has a frontend trait (`ChangeFrontend`, `DeleteFrontend`, etc.) that both CLI and TUI implement.

## Usage

### TUI Mode (interactive editing)

```bash
# Open current directory (file tree + editor)
ane .

# Open a specific file (editor only)
ane path/to/file.rs
```

**Keybindings:**
- `Ctrl-E` — toggle between Edit mode and Chord mode
- `Ctrl-T` — toggle file tree pane (creates it if opened with single file)
- `Ctrl-C` — opens exit confirmation modal (press Ctrl-C again to exit, Esc to cancel)
- `Ctrl-S` — save file (in Edit mode)
- Arrow keys — navigate
- `Tab` — insert tab character (Edit mode only)
- `Enter` — open file from tree (Chord mode, tree focused) / execute chord (Chord mode) / newline (Edit mode)
- `Esc` — clear chord input (Chord mode)

**Modes:**
- **Chord mode** (default): A command text box appears at the bottom. Type a chord and press Enter to execute it.
- **Edit mode**: Direct text editing with cursor. Toggle with Ctrl-E.

### Exec Mode (for code agents)

```bash
# Short form
ane exec --chord "cifb my_func new body text" path/to/file.rs

# Long form
ane exec --chord "ChangeInFunctionBody my_func new body" path/to/file.rs

# Line operations
ane exec --chord "cala 5 new text here" path/to/file.rs   # change line 5
ane exec --chord "iala 10 inserted text" path/to/file.rs   # insert at line 10
ane exec --chord "dala 3" path/to/file.rs                  # delete line 3
ane exec --chord "raFa" path/to/file.rs                    # read entire file
```

Exec mode outputs a unified diff to stdout showing what changed.

## LSP Integration

ane natively integrates with language servers for language-aware chord operations.

- **Auto-detection**: ane detects the project language (e.g., `Cargo.toml` → Rust) and starts the appropriate language server in the background
- **Async startup**: LSP check happens at launch in a background thread; non-LSP chords work immediately
- **Status display**: The TUI status bar shows LSP status (`ready`, `starting`, `not installed`, `failed`)
- **Chord gating**: Chords marked `requires_lsp: true` wait for LSP readiness; non-LSP chords execute immediately
- **Install assistance**: If the language server isn't installed, ane reports the install command

Currently supported: **Rust** (rust-analyzer). More languages will be added.

## Architecture

ane uses a strict 3-layer architecture:

```
Layer 2: Frontend (CLI + TUI + frontend traits)
    │ calls down to
Layer 1: Commands (chord engine + diff + LSP client)
    │ calls down to
Layer 0: Data (buffers, file tree, state, chord types, LSP registry)
```

- **Layer 0 (data)** — all filesystem I/O, state, chord type definitions, LSP server registry/schemas/install paths
- **Layer 1 (commands)** — chord parsing/execution, diff generation, LSP client (start/stop/status/requests), LSP installation
- **Layer 2 (frontend)** — CLI parsing, TUI rendering, frontend traits per action type (each action implements different behavior for CLI vs TUI)

## Building

```bash
cargo build              # dev build
cargo build --release    # release build (LTO + strip)
cargo test               # all tests
cargo clippy -- -D warnings
cargo fmt --check
```

### Using the dev container

```bash
docker build -f Dockerfile.dev -t ane-dev .
docker run -it -v $(pwd):/workspace ane-dev
```

## Project Structure

```
src/
├── main.rs                          # Entry point, dispatches TUI or exec
├── lib.rs                           # Crate root
├── data/                            # Layer 0: data and filesystem
│   ├── buffer.rs                    # File buffer (read/write/edit lines)
│   ├── chord_types.rs               # Action, Positional, Scope, Component enums + ChordSpec
│   ├── file_tree.rs                 # Directory tree walker
│   ├── state.rs                     # Editor state (Edit/Chord modes, LSP status)
│   └── lsp/
│       ├── types.rs                 # LspStatus, Language, SymbolLocation, LspServerInfo
│       └── registry.rs              # Server definitions, language detection
├── commands/                        # Layer 1: chord logic + LSP operations
│   ├── chord.rs                     # 4-part chord parsing (short + long form) + execution
│   ├── diff.rs                      # Unified diff generation
│   └── lsp/
│       ├── client.rs                # LSP client (start, stop, initialize, requests)
│       └── install.rs               # Server installation and detection
└── frontend/                        # Layer 2: CLI + TUI + traits
    ├── cli.rs                       # clap argument parsing
    ├── cli_frontend.rs              # CLI implementations of action traits
    ├── traits.rs                    # Frontend traits (ChangeFrontend, DeleteFrontend, etc.)
    └── tui/
        ├── app.rs                   # Main TUI event loop + keybinding handlers
        ├── command_bar.rs           # Chord input text box
        ├── editor_pane.rs           # Editor pane rendering
        ├── exit_modal.rs            # Ctrl-C exit confirmation modal
        ├── status_bar.rs            # Bottom bar (mode, file, position, LSP status)
        ├── tree_pane.rs             # File tree pane rendering
        └── tui_frontend.rs          # TUI implementations of action traits
```

## License

MIT
