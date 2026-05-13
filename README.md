<p align="center">
	<strong>Chord based terminal code editor for humans and agents.</strong> 
	</br>
	Language server enabled for one-shot CLI edits or interactive TUI editor.
	</br>
	</br>
	<img src="./docs/images/ane-logo.svg" width="320" alt="ane editor">
</p>

<p align="center">
	<img src="https://github.com/prettysmartdev/ane/actions/workflows/test.yml/badge.svg">
</p>

---

# ane

**A New Editor** / **Agent Native Editor**

A modern terminal editor built for humans and code agents. ane is a pure-Rust ratatui terminal app that produces a single statically-linked binary.

## Goals

1. **Chord-native editing** -- a 4-part chord system (action, positional, scope, component) for expressive, composable editing operations
2. **Agent-native interface** -- a headless `exec` mode that lets AI code agents read and modify files with minimal token usage, outputting standard unified diffs
3. **LSP-integrated** -- native language server integration for language-aware chords (starting with Rust/rust-analyzer)

## Chord System

ane chords have 4 parts: **action**, **positional**, **scope**, **component**.

| Part | Codes | Description |
|------|-------|-------------|
| Action | `c`hange, `d`elete, `r`eplace, `y`ank, `a`ppend, `p`repend, `i`nsert, `j`ump | What to do |
| Positional | `i`nside, `e`ntire, `a`fter, `b`efore, `u`ntil, `t`o, `o`utside, `n`ext, `p`revious | Where relative to scope |
| Scope | `l`ine, `b`uffer, `f`unction, `v`ariable, `s`truct, `m`ember, `d`elimiter | What language construct |
| Component | `b`eginning, `c`ontents, `e`nd, `v`alue, `p`arameters, `a`rguments, `n`ame, `s`elf | Which part of the scope |

**Examples:**
- `cifc` -- **C**hange **I**nside **F**unction **C**ontents (short form)
- `ChangeInsideFunctionContents` -- same chord, long form
- `cels(line:5, value:"new text")` -- change line 5 to "new text"
- `jnfn` -- **J**ump **N**ext **F**unction **N**ame (move cursor to the next function)

Chords that target language constructs (Function, Variable, Struct, Member) require an active LSP connection. Line, Buffer, and Delimiter scopes work without LSP.

See [docs/chord-system.md](docs/chord-system.md) for the full chord reference and [docs/chord_examples.md](docs/chord_examples.md) for worked examples of every valid scope/component combination.

### Frontend-Aware Execution

Chords behave differently depending on the frontend:

- **CLI (`ane exec`)**: accepts all arguments as parameters, returns a unified diff
- **TUI**: manipulates editor state (cursor, mode) for interactive editing

The `ApplyChordAction` trait is implemented by both CLI and TUI frontends. Jump chords are TUI-only -- the CLI rejects them before any file I/O.

## Usage

### TUI Mode (interactive editing)

```bash
# Open current directory (file tree + editor)
ane .

# Open a specific file
ane path/to/file.rs
```

**Keybindings:**

| Key | Action |
|-----|--------|
| `Ctrl-E` | Toggle between Edit mode and Chord mode |
| `Ctrl-T` | Toggle file tree pane (creates tree if opened with single file) |
| `Ctrl-S` | Save file (works in any mode) |
| `Ctrl-C` | Exit confirmation modal (press again to exit, Esc to cancel) |
| `Arrow keys` | Navigate (Edit/Chord: move cursor; Chord with input: left/right move chord cursor) |
| `Tab` | Insert tab character (Edit mode only) |
| `Enter` | Open file from tree / execute chord / newline (context-dependent) |
| `Esc` | Return to Chord mode (Edit mode) / clear chord input (Chord mode) |

**Modes:**
- **Chord mode** (default): type a chord in the command box and press Enter to execute. The cursor is displayed as a blue block.
- **Edit mode**: direct text editing with a blinking cursor. Toggle with `Ctrl-E`.

### Exec Mode (for code agents)

```bash
# Short form
ane exec --chord "cifc(function:foo, value:\"return 0;\")" path/to/file.rs

# Long form
ane exec --chord "ChangeInsideFunctionContents(function:foo, value:\"return 0;\")" path/to/file.rs

# Line operations (no LSP needed)
ane exec --chord "cels(line:5, value:\"new text\")" path/to/file.rs
ane exec --chord "dels(line:3)" path/to/file.rs

# Yank (read) entire file
ane exec --chord "yebs" path/to/file.rs

# Pipe value from stdin
echo "new body" | ane exec --chord "cifc(function:foo, value:-)" path/to/file.rs
```

Exec mode outputs a unified diff to stdout showing what changed. Yank chords output the selected text.

## LSP Integration

ane natively integrates with language servers for language-aware chord operations.

- **Auto-detection**: detects the project language (e.g., `Cargo.toml` -> Rust) and starts the appropriate language server
- **Async startup**: LSP starts in the background; non-LSP chords (Line, Buffer, Delimiter) work immediately
- **Status display**: the TUI status bar shows LSP status (ready, starting, not installed, failed)
- **Chord gating**: chords marked `requires_lsp: true` wait for LSP readiness; non-LSP chords execute immediately
- **Install assistance**: if the language server isn't installed, ane reports the install command
- **Semantic highlighting**: when the LSP is running, the editor pane renders semantic token colors

Currently supported: **Rust** (rust-analyzer). More languages will be added.

## Architecture

ane uses a strict 3-layer architecture with unidirectional dependencies:

```
Layer 2: Frontend (CLI + TUI + frontend traits)
    | calls down to
Layer 1: Commands (chord engine + diff + LSP engine)
    | calls down to
Layer 0: Data (buffers, file tree, state, chord types, LSP registry)
```

- **Layer 0 (data)** -- all filesystem I/O, state, chord type definitions, LSP server registry/schemas/types
- **Layer 1 (commands)** -- chord parsing/resolution/patching, diff generation, LSP client lifecycle and requests, LSP installation
- **Layer 2 (frontend)** -- CLI argument parsing, TUI rendering/event handling, frontend action traits

Lower layers never import from higher layers. Violating this is an architectural error.

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
│   ├── chord_types.rs               # Action, Positional, Scope, Component enums
│   ├── file_tree.rs                 # Directory tree walker
│   ├── state.rs                     # Editor state (modes, cursor, LSP status)
│   └── lsp/
│       ├── types.rs                 # DocumentSymbol, SelectionRange, ServerState
│       └── registry.rs              # Server definitions, language detection
├── commands/                        # Layer 1: chord logic + LSP operations
│   ├── chord.rs                     # Chord parsing + execution entry point
│   ├── diff.rs                      # Unified diff generation
│   ├── chord_engine/
│   │   ├── mod.rs                   # ChordEngine public API
│   │   ├── parser.rs                # Short/long form chord parser
│   │   ├── resolver.rs              # Scope/component/positional resolution
│   │   ├── patcher.rs               # Diff/yank generation from resolved ranges
│   │   ├── types.rs                 # ChordQuery, TextRange, ResolvedChord
│   │   ├── errors.rs                # ChordError variants with suggestions
│   │   └── text.rs                  # Text manipulation helpers
│   └── lsp_engine/
│       ├── mod.rs                   # LspEngine public API
│       ├── engine.rs                # LSP client (start, stop, requests)
│       ├── installer.rs             # Server installation and detection
│       ├── detector.rs              # Language detection from project files
│       ├── health.rs                # Server health monitoring
│       └── transport.rs             # JSON-RPC transport layer
└── frontend/                        # Layer 2: CLI + TUI + traits
    ├── cli.rs                       # clap argument parsing
    ├── cli_frontend.rs              # CLI implementation of ApplyChordAction
    ├── traits.rs                    # ApplyChordAction trait
    └── tui/
        ├── app.rs                   # TUI event loop + keybinding handlers
        ├── chord_box.rs             # Chord input text box
        ├── editor_pane.rs           # Editor pane with semantic highlighting
        ├── exit_modal.rs            # Ctrl-C exit confirmation modal
        ├── status_bar.rs            # Mode, file, position, LSP status
        ├── title_bar.rs             # Title bar rendering
        ├── tree_pane.rs             # File tree pane
        └── tui_frontend.rs          # TUI implementation of ApplyChordAction
```

## License

MIT
