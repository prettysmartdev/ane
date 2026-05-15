# Getting Started

`ane` is a chord-based terminal code editor built for humans and code agents. Every edit is expressed as a four-part chord -- action, positional, scope, component -- that composes into precise, language-aware operations. An embedded language server gives chords access to functions, variables, structs, and other symbols out of the box.

This guide walks you through the core concepts and gets you making edits.

---

## Core concepts

### Chords

A chord is a four-character instruction that describes an edit:

```
<action><positional><scope><component>
```

For example, `cifc` means **C**hange **I**nside **F**unction **C**ontents -- replace the body of a function. Chords can also be written in long form: `ChangeInsideFunctionContents`.

Arguments are passed in parentheses:

```
cifc(target:foo, value:"return 0;")
```

See [Chord System](01-chord-system.md) for the full reference and [Chord Examples](02-chord-examples.md) for worked before/after examples of every valid combination.

### Two frontends

ane has two ways to run:

- **TUI** -- an interactive terminal editor with a file tree, syntax highlighting, and a chord input box. Designed for humans.
- **Exec** -- a headless one-shot mode that applies a chord and outputs a unified diff. Designed for code agents and scripts.

Both frontends use the same chord engine. Some chords are frontend-specific: `Jump` is TUI-only (the CLI has no cursor).

### Language server integration

Chords that target language constructs (Function, Variable, Struct, Member) use an embedded language server to resolve symbols. ane auto-detects your project language, starts the server in the background, and installs it if needed.

Line, Buffer, and Delimiter scopes work immediately without LSP.

Supported: **Rust** (rust-analyzer), **Go** (gopls), **TypeScript/JavaScript** (vtsls), **Python** (basedpyright). All include tree-sitter syntax highlighting. **Markdown** has tree-sitter highlighting with no LSP server.

---

## Installation

```sh
curl -s https://prettysmart.dev/install/ane.sh | sh
```

The installer detects your platform and puts `ane` on your `PATH`.

<details>
<summary>Other installation options</summary>

**With cargo:**

```sh
cargo install ane-editor
```

**With mise** -- using the [GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html):

```sh
mise use -g github:prettysmartdev/ane
```

To pin to a specific version: `mise use -g github:prettysmartdev/ane@0.1.0`

**From GitHub Releases** -- download the binary for your platform from [GitHub Releases](https://github.com/prettysmartdev/ane/releases):

| Platform | Asset |
|----------|-------|
| Linux (x86_64) | `ane-linux-amd64` |
| Linux (ARM64) | `ane-linux-arm64` |
| macOS (Intel) | `ane-macos-amd64` |
| macOS (Apple Silicon) | `ane-macos-arm64` |

**From source** -- requires Rust 1.75+ and make:

```sh
git clone https://github.com/prettysmartdev/ane.git
cd ane
sudo make install
```

</details>

---

## Your first TUI session

Open a file or directory:

```sh
ane .                   # directory -- opens file tree + editor
ane path/to/file.rs     # single file -- opens directly in editor
```

ane starts in **Chord mode**. A command box appears at the bottom of the screen. Type a chord and press Enter to execute it. Short-form chords (4 lowercase characters) auto-execute when the combination is valid for the current cursor position.

Press `Ctrl-E` to switch to **Edit mode** for direct text editing. Press `Esc` or `Ctrl-E` to return to Chord mode.

Key reference:

| Key | Action |
|-----|--------|
| `Ctrl-E` | Toggle Edit / Chord mode |
| `Ctrl-T` | Toggle file tree |
| `Ctrl-S` | Save file |
| `Ctrl-C` | Exit (confirmation modal) |
| `Arrow keys` | Navigate cursor |

See [Using the TUI](03-using-the-tui.md) for the full keybinding reference and mode details.

---

## Your first CLI edit

Exec mode applies a single chord to a file and outputs a unified diff:

```sh
# Change line 5 to new text
ane exec --chord "cels(target:5, value:\"new text\")" path/to/file.rs

# Delete line 3
ane exec --chord "dels(target:3)" path/to/file.rs

# Yank (read) the entire file
ane exec --chord "yebs" path/to/file.rs

# Replace a function body
ane exec --chord "cifc(target:init, value:\"    todo!()\")" src/main.rs
```

Exec mode writes a unified diff to stdout. Yank chords output the selected text instead.

See [Exec Mode](04-exec-mode.md) for stdin piping, agent integration patterns, and more.

---

## Embedding ane as a library

If you're building a code agent or custom tooling, you can use ane as a Rust crate without the CLI/TUI dependencies:

```toml
[dependencies]
ane-editor = { version = "0.1", default-features = false }
```

This gives you the chord engine, LSP engine, buffer management, and a tool definition for LLM integration -- without pulling in `clap`, `crossterm`, or `ratatui`.

See [Embedding via Crate](05-embedding-via-crate.md) for the full API surface and usage examples.

---

## Teaching ane to your code agent

ane can generate a token-efficient skill file for any supported code agent:

```sh
ane init claude      # creates .claude/skills/ane/SKILL.md
ane init codex       # creates .codex/skills/ane/SKILL.md
```

Supported agents: claude, codex, gemini, opencode, cline, maki, charm.

The skill file teaches the agent ane's chord grammar in under 400 tokens, so it can use `ane exec` for structured edits.

---

## What's next

- **[Chord System](01-chord-system.md)** -- the full 4-part grammar reference
- **[Chord Examples](02-chord-examples.md)** -- worked before/after examples for every combination
- **[Using the TUI](03-using-the-tui.md)** -- modes, keybindings, file tree
- **[Exec Mode](04-exec-mode.md)** -- headless one-shot edits for agents and scripts
- **[Embedding via Crate](05-embedding-via-crate.md)** -- using ane as a Rust library
- **[LSP Integration](06-lsp-integration.md)** -- language server setup and status
- **[Architecture Overview](07-architecture-overview.md)** -- three-layer design

---

[Next: Chord System ->](01-chord-system.md)
