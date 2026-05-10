# CLAUDE.md

## Project Overview

ane (A New Editor / Agent Native Editor) is a Rust terminal editor built on ratatui. It produces a single static binary.

## Build & Test

```bash
cargo build              # dev build
cargo build --release    # release build (LTO + strip)
cargo test               # all tests
cargo clippy -- -D warnings
cargo fmt --check
```

No Rust toolchain is installed in this workspace. Use `Dockerfile.dev` to get a full dev environment:
```bash
docker build -f Dockerfile.dev -t ane-dev .
docker run -it -v $(pwd):/workspace ane-dev
```

## Architecture Rules (MUST follow)

Three-layer architecture with strict dependency direction:

- **Layer 0** (`src/data/`): All filesystem I/O, state, chord type definitions, LSP data (registry, schemas, types). No imports from `commands` or `frontend`.
- **Layer 1** (`src/commands/`): Chord logic, diff generation, LSP client operations, LSP installation. May import from `data`. No imports from `frontend`.
- **Layer 2** (`src/frontend/`): CLI + TUI + frontend action traits. May import from `data` and `commands`.

Lower layers NEVER depend on higher layers. Violating this is a build-breaking architectural error.

## Chord System

Chords have 4 parts: **action** (c/d/r/i/m/s/y), **positional** (i/a/r/b/f), **scope** (f/v/b/l/F/s/m/e), **component** (b/n/s/p/t/v/a).

- Short form: `cifb` = ChangeInFunctionBody
- Long form: `ChangeInFunctionBody`

Scopes with `requires_lsp: true`: Function, Variable, Block, Struct, Impl, Enum. Line and File do not require LSP.

## Frontend Traits

Each action has a trait in `src/frontend/traits.rs` (ChangeFrontend, DeleteFrontend, etc.). Both CliFrontend and TuiFrontend implement these traits with different behavior:
- CLI: accepts all arguments as parameters, returns results
- TUI: manipulates editor state (cursor, mode) for interactive editing

## TUI Keybindings

- `Ctrl-E`: toggle Edit/Chord mode
- `Ctrl-T`: toggle file tree (creates tree if opened with single file)
- `Ctrl-C`: exit confirmation modal
- `Ctrl-S`: save (Edit mode)
- Arrow keys: navigation
- Tab: insert tab (Edit mode only)
- No h/j/k/l, no i, no q

## Code Style

- Rust edition 2021, stable toolchain
- `anyhow::Result` for error handling
- Tests live in `#[cfg(test)] mod tests` blocks inside each source file
- No comments unless the "why" is non-obvious
- clap derive macros for CLI parsing
