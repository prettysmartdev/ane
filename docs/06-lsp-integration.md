# LSP Integration

ane natively integrates with language servers to provide language-aware chord operations. This page covers how LSP works in ane, which chords require it, and how to check its status.

---

## How it works

When you open a file or run an exec command, ane:

1. **Auto-detects** the project language by scanning for project files (e.g., `Cargo.toml` for Rust).
2. **Starts the language server** in the background. Non-LSP chords work immediately while the server initializes.
3. **Auto-installs** the language server if it's not found on your system (when possible).
4. **Provides syntax highlighting** via LSP semantic tokens once the server is running.

---

## Which chords require LSP?

| Scope | LSP required | How it resolves |
|-------|:------------:|-----------------|
| Line | No | By line number or cursor position |
| Buffer | No | Entire file |
| Delimiter | No | Text-based delimiter scanning |
| Function | Yes | LSP `documentSymbol` |
| Variable | Yes | LSP `documentSymbol` + `selectionRange` |
| Struct | Yes | LSP `documentSymbol` |
| Member | Yes | LSP `documentSymbol` |

LSP-scoped chords wait for the server to reach the `Running` state before executing. If the server fails to start, these chords return an error with a diagnostic message.

---

## Supported languages

| Language | Server | Detection |
|----------|--------|-----------|
| Rust | rust-analyzer | `Cargo.toml` in project root |

More languages will be added. The LSP engine is designed to support any language server that implements the Language Server Protocol.

---

## Status display (TUI)

The status bar shows the current LSP state:

| Status | Meaning |
|--------|---------|
| ready | Server running, all chords available |
| starting | Server initializing |
| installing | Server being auto-installed |
| not installed | No server found for this language |
| failed | Server encountered an error |

---

## Chord gating

When a chord targets an LSP scope (Function, Variable, Struct, Member):

- **TUI mode**: the chord waits for LSP readiness. If the server is still starting, the status bar shows progress.
- **Exec mode**: the command blocks until the server is ready, then executes the chord.

Non-LSP chords (Line, Buffer, Delimiter) bypass this gate entirely and execute immediately.

---

[<- Embedding via Crate](05-embedding-via-crate.md) | [Next: Architecture Overview ->](07-architecture-overview.md)
