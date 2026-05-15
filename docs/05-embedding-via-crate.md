# Embedding via Crate

ane exposes its chord engine, LSP engine, and buffer management as a Rust crate. This lets you embed ane's editing capabilities in custom agent harnesses, CI tools, or other Rust programs without pulling in CLI/TUI dependencies.

---

## Adding the dependency

```toml
[dependencies]
ane-editor = { version = "0.1", default-features = false }
```

The `frontends` feature (enabled by default) gates `clap`, `crossterm`, and `ratatui`. Disabling it drops ~30 transitive crates and gives you a minimal library with just the editing core.

---

## Feature flags

| Feature | Default | What it includes |
|---------|---------|-----------------|
| `frontends` | yes | CLI (`clap`) + TUI (`crossterm`, `ratatui`) + `src/frontend/` module |

With `default-features = false`, only the core modules are compiled: `commands`, `data`, and `core`.

---

## The `core` module

`ane::core` is the primary API surface for library consumers. It re-exports everything needed to parse, resolve, and apply chords programmatically:

### Types

| Re-export | Source | Purpose |
|-----------|--------|---------|
| `parse_chord` | `commands::chord` | Parse a chord string into a `ChordQuery` |
| `execute_chord` | `commands::chord` | Full pipeline: parse + resolve + patch |
| `ChordResult` | `commands::chord` | Result of chord execution (diff, yanked text) |
| `FrontendCapabilities` | `commands::chord` | Trait for declaring frontend capabilities (e.g. `is_interactive`) |
| `ChordEngine` | `commands::chord_engine` | Direct access to the 3-stage pipeline |
| `ChordQuery` | `commands::chord_engine::types` | Parsed chord representation |
| `ChordAction` | `commands::chord_engine::types` | Resolved action with diff and metadata |
| `ChordArgs` | `commands::chord_engine::types` | Parsed arguments |
| `ResolvedChord` | `commands::chord_engine::types` | Resolved text ranges |
| `TextRange` | `commands::chord_engine::types` | Line/column range in a buffer |
| `ChordError` | `commands::chord_engine::errors` | Structured error type |
| `unified_diff` | `commands::diff` | Generate unified diff from before/after text |
| `LspEngine` | `commands::lsp_engine` | Language server lifecycle and queries |
| `LspEngineConfig` | `commands::lsp_engine` | LSP configuration |
| `Buffer` | `data::buffer` | File buffer (read, write, line access) |
| `Action`, `Component`, `Positional`, `Scope` | `data::chord_types` | Chord grammar enums |
| `SKILL_CONTENT` | `data::skill` | Embedded skill markdown (for agent integration) |

### Tool definition

`ane::core::tool_definition()` returns a `ToolDefinition` struct with `name`, `description`, and `input_schema` fields, ready to be serialized and passed to any LLM tool-use API (Claude, OpenAI, etc.):

```rust
use ane::core::tool_definition;

let def = tool_definition();
let tools = vec![serde_json::to_value(def)?];
// Pass `tools` to your LLM API
```

---

## Usage examples

### Parse and inspect a chord

```rust
use ane::core::{parse_chord, Action, Positional, Scope, Component};

let query = parse_chord("cifc")?;
assert_eq!(query.action, Action::Change);
assert_eq!(query.positional, Positional::Inside);
assert_eq!(query.scope, Scope::Function);
assert_eq!(query.component, Component::Contents);
```

### Execute a chord against a file

```rust
use ane::core::{execute_chord, FrontendCapabilities};
use ane::core::{LspEngine, LspEngineConfig};
use std::path::Path;

struct HeadlessContext;
impl FrontendCapabilities for HeadlessContext {
    fn is_interactive(&self) -> bool { false }
}

let path = Path::new("src/main.rs");
let query = ane::core::parse_chord("cifn(target:old_name, value:\"new_name\")")?;
let mut lsp = LspEngine::new(LspEngineConfig::default());
let result = execute_chord(&HeadlessContext, path, &query, &mut lsp)?;
```

### Access the embedded skill content

```rust
use ane::core::SKILL_CONTENT;

// Write to a file, send to an agent, etc.
std::fs::write(".claude/skills/ane/SKILL.md", SKILL_CONTENT)?;
```

---

## Architecture note

The `core` module is a re-export facade -- it contains no logic of its own. All types and functions live in their original modules (`commands::chord`, `data::buffer`, etc.) and are always available at those paths too. `core` simply provides a flat, convenient entry point for external consumers.

---

[<- Exec Mode](04-exec-mode.md) | [Next: LSP Integration ->](06-lsp-integration.md)
