# Project Foundation

Name: ane
Type: cli
Purpose: A modern terminal editor built for humans and code agents. "ane" stands for both "A New Editor" and "Agent Native Editor". It provides a ratatui-based TUI for interactive editing and a headless CLI mode (`exec`) for code agents to manipulate files programmatically with minimal token usage. ane uses a 4-part chord system (action, positional, scope, component) and natively integrates with language servers for language-aware editing.

# Technical Foundation

## Languages and Frameworks

### CLI / TUI
Language: Rust
Frameworks: ratatui, crossterm, clap
Guidance:
- Target the latest stable Rust edition (2021+)
- Produce a single statically-linked binary
- Use crossterm as the ratatui backend for cross-platform terminal support
- Use clap with derive macros for CLI argument parsing

# Best Practices
- Organize code in small, simple, modular components
- Each component should contain unit tests that validate its behaviour in terms of inputs and outputs
- The overall codebase should contain integration tests that validate the interaction between components that are used together
- Strictly enforce the 3-layer architecture: Layer 0 (data) -> Layer 1 (commands) -> Layer 2 (frontend)
- Never allow lower layers to depend on higher layers
- Keep all filesystem I/O and LSP data definitions in Layer 0
- Keep LSP operations (client, install) in Layer 1
- Keep all terminal I/O and frontend traits in Layer 2
- Prefer returning Result<T> with anyhow for error handling
- Write idiomatic Rust: use iterators, pattern matching, and the type system to prevent bugs at compile time

# Personas

### Persona 1:
Name: Developer
Purpose: A human software developer using ane as their primary terminal editor
Use-cases:
- Open files and directories for editing in the TUI
- Navigate a project's file tree
- Use chord mode to execute composable editing operations
- Edit code directly in Edit mode
- Benefit from LSP-powered language-aware chords
RBAC:
- Full access to all TUI and CLI features

### Persona 2:
Name: Code Agent
Purpose: An AI code agent (e.g. Claude Code, aider, Cursor) that needs to read and modify files efficiently
Use-cases:
- Execute chords via `ane exec` to make precise edits using 4-part chord syntax
- Use short-form chords (e.g. `cifb`) for minimal token usage
- Read file contents with minimal overhead
- Receive git-style diffs as output to confirm changes
RBAC:
- Access to all `exec` subcommand chords
- No TUI interaction (headless only)
