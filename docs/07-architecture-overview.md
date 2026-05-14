# Architecture Overview

ane uses a strict three-layer architecture with unidirectional dependencies. Lower layers never import from higher layers.

---

## Layers

```
Layer 2: Frontend (CLI + TUI + frontend traits)
    | calls down to
Layer 1: Commands (chord engine + diff + LSP engine)
    | calls down to
Layer 0: Data (buffers, file tree, state, chord types, LSP registry)
```

### Layer 0 -- Data (`src/data/`)

All filesystem I/O, editor state, chord type definitions, and LSP server registry/schemas/types. This layer has no knowledge of how chords are executed or how the UI works.

Key modules:
- `buffer.rs` -- file buffer (read, write, line access)
- `chord_types.rs` -- `Action`, `Positional`, `Scope`, `Component` enums and validity rules
- `state.rs` -- `EditorState` struct (cursor, mode, buffers, scroll)
- `file_tree.rs` -- directory tree structure
- `lsp/` -- LSP registry, server schemas, type definitions
- `agents.rs` -- agent configuration for `ane init`
- `skill.rs` -- embedded skill content

### Layer 1 -- Commands (`src/commands/`)

Chord parsing, resolution, and patching. LSP client lifecycle and operations. Diff generation.

Key modules:
- `chord_engine/` -- three-stage pipeline (parser, resolver, patcher)
- `chord.rs` -- `execute_chord` entry point, `FrontendCapabilities` trait
- `lsp_engine.rs` -- language server lifecycle, queries, installation
- `diff.rs` -- unified diff generation

### Layer 2 -- Frontend (`src/frontend/`)

CLI argument parsing, TUI rendering and event handling, frontend action traits.

Key modules:
- `cli.rs` -- `clap` command definitions
- `tui/` -- terminal UI (app loop, editor pane, tree pane, chord box)
- `traits.rs` -- `ChangeFrontend`, `DeleteFrontend`, etc. (action traits implemented by both frontends)
- `cli_frontend.rs` -- CLI implementations of frontend traits
- `tui/tui_frontend.rs` -- TUI implementations of frontend traits

---

## Dependency rule

The dependency direction is strictly enforced:

- **Layer 0** imports from: nothing (only std and external crates)
- **Layer 1** imports from: Layer 0
- **Layer 2** imports from: Layer 0 and Layer 1

Layer 0 never imports from `commands` or `frontend`. Layer 1 never imports from `frontend`. Violating this is a build-breaking architectural error.

---

## Feature flag

The `frontends` feature (default-enabled) gates Layer 2 and its dependencies (`clap`, `crossterm`, `ratatui`). With `default-features = false`, only Layers 0 and 1 are compiled, producing a minimal library suitable for embedding. See [Embedding via Crate](05-embedding-via-crate.md).

---

## Chord engine pipeline

The chord engine in Layer 1 processes chords through three stages:

```
1. PARSE    input string  -->  ChordQuery
2. RESOLVE  ChordQuery + buffer + LSP  -->  ResolvedChord (text ranges)
3. PATCH    ResolvedChord + buffer  -->  diff / yanked text
```

Each stage is a separate module (`parser.rs`, `resolver.rs`, `patcher.rs`) with clear input/output types and independent test coverage.

---

## Frontend traits

Each action (Change, Delete, Replace, Yank, Append, Prepend, Insert, Jump) has a corresponding trait in `src/frontend/traits.rs`. Both `CliFrontend` and `TuiFrontend` implement these traits with different behavior:

- **CLI**: accepts all arguments as parameters, returns results to stdout
- **TUI**: manipulates editor state (cursor position, mode) for interactive editing

This design means the chord engine doesn't know which frontend is running -- it produces a `ChordAction`, and the frontend decides how to apply it.

---

[<- LSP Integration](06-lsp-integration.md) | [Next: Compared to Other Editors ->](08-compared-to-other-editors.md) | [Syntax Highlighting ->](09-syntax-highlighting-and-languages.md)
