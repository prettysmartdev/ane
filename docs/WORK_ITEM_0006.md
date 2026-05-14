# Work Item 0006: Code Agent Integration

## Overview

Work Item 0006 adds three major capabilities to ane to enable seamless integration with code agents (Claude, Codex, Gemini, and others):

1. **Skill Definition** — A token-efficient markdown guide for code agents to understand and use ane's chord system
2. **Agent Initialization** — CLI command to auto-generate agent-specific skill files in the correct directories
3. **Programmatic API** — A `core` module that exposes ane's editing engine and LSP capabilities as a library, with a `frontends` feature flag to gate CLI/TUI dependencies

This enables three key user stories:
- Code agents can be taught ane's chord grammar with `ane init <agent>`
- Developers can embed ane's engine directly via `ane = { default-features = false }`
- Custom agent harnesses can dispatch chord edits programmatically

---

## Part 1: Skill Definition

### Purpose

The skill is a minimal-token markdown reference that teaches code agents:
- When and how to use ane for structured edits
- The chord 4-part grammar (Action + Positional + Scope + Component)
- Invocation syntax: `ane exec <file> --chord "<chord>"`
- Real-world examples tailored to agent workflows

### Location and Format

**File**: `skills/ane-skill.md` (project root, outside `src/`)

**Constraints**:
- Under 400 tokens (whitespace-delimited words as proxy)
- No prose paragraphs — tables, lists, terse notation only
- Tables for grammar to minimize repetition
- Inline examples showing common patterns

### Structure

```markdown
# Header + one-line description
## Chord grammar (table of 4 parts: Action, Positional, Scope, Component)
## Arguments (signature and stdin piping pattern)
## LSP scopes (which scopes require LSP, which don't)
## Examples (5-8 concrete `ane exec` invocations)
## Notes (edge cases: Jump is TUI-only, stdin piping, etc.)
```

### Integration with Code

Embedded at compile time in `src/data/skill.rs`:

```rust
pub const SKILL_CONTENT: &str = include_str!("../../skills/ane-skill.md");
```

This allows the skill to be:
- Updated as standalone markdown without rebuilding
- Accessed via `ane::data::skill::SKILL_CONTENT` in Layer 0
- Written to disk by the `ane init` subcommand
- Exposed via the `core` public API

### Current Implementation

- **File**: `skills/ane-skill.md` — 48 lines, ~290 words, well under 400-token budget
- **Embedded**: `src/data/skill.rs` uses `include_str!` with compile-time validation tests
- **Tests**: 
  - `skill_content_is_non_empty` — basic sanity check
  - `skill_content_contains_key_markers` — verifies key phrases ("ane exec", "Action", "Scope") to catch accidental corruption

---

## Part 2: Agent Initialization

### Purpose

Provides a CLI subcommand `ane init <agent>` that:
- Creates the agent-specific skill directory (relative to CWD, usually project root)
- Writes the embedded `SKILL_CONTENT` to the appropriate skill file
- Idempotent: re-running overwrites with the latest embedded skill

### Supported Agents

7 agents with researched directory conventions:

| Agent     | Skill Directory          | Skill File |
|-----------|--------------------------|------------|
| claude    | `.claude/skills/ane`     | `SKILL.md` |
| codex     | `.codex/skills/ane`      | `SKILL.md` |
| gemini    | `.gemini/skills/ane`     | `SKILL.md` |
| opencode  | `.opencode/skills/ane`   | `SKILL.md` |
| cline     | `.cline/skills/ane`      | `SKILL.md` |
| maki      | `.maki/skills/ane`       | `SKILL.md` |
| charm     | `.charm/skills/ane`      | `SKILL.md` |

### Architecture

Spans three layers:

**Layer 0 (`src/data/`)** — Agent configuration and init logic
- `agents.rs` — `AgentConfig` struct, `AGENTS` const, `find_agent()` and `agent_names()` functions
- `init.rs` — `init_agent(agent_name, base_dir)` function that creates dirs and writes skill file

**Layer 2 (`src/frontend/cli.rs`)** — CLI argument parsing
- `Init { agent: String }` variant in `Command` enum (positional argument)

**`src/main.rs`** — Command dispatch
- Match arm calls `init_agent()` with CWD as base directory
- Prints confirmation message or error with supported agent list

### Layer Independence

- Layer 0 functions take `base_dir: &Path` as parameter (testable in isolation)
- `main.rs` calls with `std::env::current_dir()` (or hardcoded `.` in tests)
- No upward dependencies; Layer 0 never touches CLI or TUI code

### Edge Cases Handled

| Case | Behavior |
|------|----------|
| Unknown agent | Error with list of supported agents |
| Existing skill file | Overwrites without prompting (idempotent) |
| Missing parent dirs | `create_dir_all()` creates full tree |
| Permission denied | OS error with clear path context |
| Case variations | `eq_ignore_ascii_case()` handles "Claude", "CLAUDE", etc. |

### Current Implementation

**`src/data/agents.rs`** (95 lines):
- `AgentConfig` struct: name, skill_dir, skill_filename
- `AGENTS` const array: all 7 agents with researched paths
- `find_agent(name)` — case-insensitive lookup
- `agent_names()` — returns supported agent list
- Unit tests covering all edge cases (case-insensitive, unknown agents, etc.)

**`src/data/init.rs`** (24 lines):
- `init_agent(agent_name, base_dir)` — creates dirs, writes skill file
- Error handling with clear agent list in error message
- Layer 0 purity: no imports from `frontend`

**Tests** (`tests/work_item_0006.rs`):
- `run_init_creates_directory_and_file` — tempdir integration test
- `run_init_overwrites_existing_file` — idempotency test
- `run_init_unknown_agent_returns_error` — error message contains agent list

### Usage

```bash
# From project root
ane init claude

# Creates .claude/skills/ane/SKILL.md with embedded skill content
# Output: "wrote ane skill to .claude/skills/ane/SKILL.md"

# Re-run to update skill to latest version
ane init claude

# List all supported agents (in error message)
ane init vim
# Error: unknown agent 'vim'. Supported: claude, codex, gemini, opencode, cline, maki, charm
```

---

## Part 3: Programmatic API & Feature Flag

### Purpose

Exposes ane's core editing and LSP capabilities as a Rust library without bundling CLI/TUI dependencies (`clap`, `crossterm`, `ratatui`). This enables:
- Embedding ane in custom agent harnesses
- Avoiding ~30 transitive crates for library-only consumers
- Unified API surface for programmatic chord execution

### Architecture

**Feature Flag: `frontends`** (default-enabled)
- Gates `clap`, `crossterm`, `ratatui` as optional dependencies
- Main binary (`[[bin]] ane`) requires this feature
- Library consumers opt out: `ane = { default-features = false }`

**Conditional Module in `src/lib.rs`**:
```rust
pub mod commands;
pub mod core;
pub mod data;

#[cfg(feature = "frontends")]
pub mod frontend;
```

**`src/core.rs`** — Curated public re-exports
- Stateless: only `pub use` statements and `tool_definition()` function
- No new logic or types; zero-cost wrapper around existing components
- Always available (not behind feature flag)

### Module: `core`

The `core` module re-exports everything a programmatic consumer needs:

**Type Re-exports**:
```rust
pub use crate::commands::chord::{ChordResult, FrontendCapabilities, execute_chord, parse_chord};
pub use crate::commands::chord_engine::errors::ChordError;
pub use crate::commands::chord_engine::types::{ChordAction, ChordArgs, ChordQuery, ResolvedChord, TextRange};
pub use crate::commands::chord_engine::ChordEngine;
pub use crate::commands::diff::unified_diff;
pub use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
pub use crate::data::buffer::Buffer;
pub use crate::data::chord_types::{Action, Component, Positional, Scope};
pub use crate::data::skill::SKILL_CONTENT;
```

**Tool Definition** (for LLM integration):
```rust
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub fn tool_definition() -> ToolDefinition { ... }
```

### Consumer Usage Examples

#### Agent Harness Embedding ane

```toml
# Cargo.toml
[dependencies]
ane = { version = "0.1", default-features = false }
```

```rust
use ane::core::{Buffer, ChordEngine, LspEngine, LspEngineConfig, parse_chord};

// Create a buffer
let mut buffer = Buffer::new("path/to/file.rs")?;

// Parse a chord
let query = parse_chord("cifn")?;

// Execute with LSP
let lsp = LspEngine::new(config)?;
let chord_engine = ChordEngine::new(&mut buffer, &lsp)?;
let result = ane::core::execute_chord(chord_engine, &query)?;
```

#### Tool Definition for LLM API

```rust
use ane::core::tool_definition;
use serde_json::json;

let tools = vec![serde_json::to_value(tool_definition())?];
// Pass `tools` to Claude API, OpenAI API, etc.
```

### `Cargo.toml` Changes

```toml
[features]
default = ["frontends"]
frontends = ["dep:clap", "dep:crossterm", "dep:ratatui"]

[dependencies]
# Core (always included)
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
similar = "2"
walkdir = "2"

# Frontend-only (gated behind `frontends` feature)
clap = { version = "4", features = ["derive"], optional = true }
crossterm = { version = "0.28", features = ["event-stream"], optional = true }
ratatui = { version = "0.29", optional = true }

[[bin]]
name = "ane"
path = "src/main.rs"
required-features = ["frontends"]
```

### Visibility Requirements

All re-exported types must be `pub` and reachable via public module chains. The following types were checked at implementation time:

- `ChordResult`, `FrontendCapabilities`, `execute_chord`, `parse_chord` — in `src/commands/chord.rs` (public)
- `ChordError` — in `src/commands/chord_engine/errors.rs` (public)
- `ChordAction`, `ChordArgs`, `ChordQuery`, `ResolvedChord`, `TextRange` — in `src/commands/chord_engine/types.rs` (public)
- `ChordEngine` — in `src/commands/chord_engine/mod.rs` (public)
- `unified_diff` — in `src/commands/diff.rs` (public)
- `LspEngine`, `LspEngineConfig` — in `src/commands/lsp_engine/mod.rs` (public)
- `Buffer` — in `src/data/buffer.rs` (public)
- `Action`, `Component`, `Positional`, `Scope` — in `src/data/chord_types.rs` (public)
- `SKILL_CONTENT` — in `src/data/skill.rs` (public const)

### Current Implementation

**`src/core.rs`** (121 lines):
- Re-exports of core types
- `ToolDefinition` struct and `tool_definition()` function
- Tool description under 250 words (~165 words)
- Comprehensive tests covering:
  - Round-trip parse test
  - Skill content accessibility
  - Tool definition name, description, schema validation
  - JSON serialization
  - Word count validation for tool description

**`src/lib.rs`** modifications:
- `pub mod core;` (unconditional)
- `#[cfg(feature = "frontends")] pub mod frontend;` (conditional)

### Testing Strategy

| Test Type | Coverage |
|-----------|----------|
| Compile without frontends | `cargo build --no-default-features` produces library only |
| Compile with frontends | `cargo build` builds binary + all frontends (no regression) |
| Unit: core API round-trip | `parse_chord("cifn")` and `execute_chord()` work via core exports |
| Unit: tool definition | Name, description, schema validity, JSON serialization |
| Integration: test suite | `cargo test --no-default-features` passes (no frontend-gated tests fail) |
| Manual: library usage | Downstream crate using `ane = { default-features = false }` successfully imports and uses core types |

---

## Integration Points

### Codebase Changes Summary

| File | Change | Type |
|------|--------|------|
| `Cargo.toml` | Add `frontends` feature; gate clap/crossterm/ratatui; mark binary as `required-features = ["frontends"]` | Feature gate |
| `src/lib.rs` | Add `pub mod core;`; wrap `pub mod frontend` in `#[cfg(feature = "frontends")]` | Module structure |
| `src/core.rs` | New file: re-exports + tool definition | New API |
| `src/data/skill.rs` | New file: embed skill markdown via `include_str!` | Layer 0 data |
| `src/data/init.rs` | New file: `init_agent(name, base_dir)` function | Layer 0 logic |
| `src/data/agents.rs` | New file: agent config, lookup functions | Layer 0 data |
| `src/data/mod.rs` | Add `pub mod skill;`, `pub mod init;`, `pub mod agents;` | Module declaration |
| `src/frontend/cli.rs` | Add `Init { agent: String }` to `Command` enum | CLI command |
| `src/main.rs` | Add `Some(Command::Init { agent })` match arm | Command dispatch |
| `skills/ane-skill.md` | New file: agent-facing skill reference | Documentation |
| `tests/work_item_0006.rs` | New file: integration tests | Test suite |

### No Breaking Changes

- Existing chord execution logic unchanged
- Existing CLI subcommands unaffected
- Existing TUI behavior unaffected
- Feature flag is opt-in for library consumers; binary unchanged from user perspective

### Architectural Compliance

✅ **Layer 0** — `skill.rs`, `agents.rs`, `init.rs` contain only data and pure functions
✅ **Layer 1** — No new commands logic; chord execution path unchanged
✅ **Layer 2** — `cli.rs` gains `Init` variant; `main.rs` dispatches to Layer 0 init function
✅ **Dependency direction** — All new code respects three-layer hierarchy; no upward imports

---

## Testing Checklist

### Unit Tests (in source files)

**`src/data/skill.rs`**:
- ✅ `skill_content_is_non_empty`
- ✅ `skill_content_contains_key_markers` — checks "ane exec", "Action", "Scope"

**`src/data/agents.rs`**:
- ✅ `find_agent_returns_config_for_all_supported_names` — all 7 agents
- ✅ `find_agent_is_case_insensitive` — "Claude", "CLAUDE"
- ✅ `find_agent_returns_none_for_unknown_agent` — "vim"

**`src/core.rs`**:
- ✅ `core_parse_round_trip` — parse "cifn", validate action
- ✅ `skill_content_accessible_via_core` — non-empty
- ✅ `tool_definition_has_correct_name` — name == "ane"
- ✅ `tool_definition_has_non_empty_description`
- ✅ `tool_definition_schema_has_required_fields` — file_path, chord
- ✅ `tool_definition_serializes_to_valid_json` — serde_json roundtrip
- ✅ `tool_description_under_250_words` — word count validation

### Integration Tests (`tests/work_item_0006.rs`)

- ✅ `skill_file_under_400_tokens` — word count check
- ✅ `run_init_creates_directory_and_file` — tempdir, creates `.claude/skills/ane/SKILL.md`, content matches
- ✅ `run_init_overwrites_existing_file` — idempotency
- ✅ `run_init_unknown_agent_returns_error` — error contains "unknown agent" + agent list

### Compilation Tests

- ✅ `cargo build --release` — binary with frontends (default)
- ✅ `cargo build --no-default-features` — library only, no frontends
- ✅ `cargo test` — all tests pass with frontends
- ✅ `cargo test --no-default-features` — all non-frontend tests pass
- ✅ `cargo clippy -- -D warnings` — no warnings
- ✅ `cargo fmt --check` — formatting correct

### Manual Verification

- ✅ `ane init claude` in fresh directory creates `.claude/skills/ane/SKILL.md`
- ✅ Running again overwrites without prompt
- ✅ `ane init unsupported` lists all agent names
- ✅ Downstream crate: `ane = { default-features = false }` imports and uses core types
- ✅ Feed skill to Claude → generates correct `ane exec` commands

---

## Documentation & Discoverability

### User-Facing

- `ane init --help` — explains subcommand, lists supported agents
- `skills/ane-skill.md` — self-contained reference for code agents
- Inline examples in skill markdown — `ane exec` patterns for common use cases

### Developer-Facing

- **`src/core.rs`** — rustdoc comments (public API)
- **`src/data/skill.rs`** — brief comment explaining `include_str!` approach
- **`src/data/init.rs`** — function-level comments on edge cases
- **`Cargo.toml`** — feature documentation in `[features]` section (if added)

### This Document

- Work item requirements and rationale
- Architecture decisions and compliance
- Integration points and changes
- Testing strategy and checklist
- Usage examples for each feature

---

## Future Considerations

### Extending Agent Support

To add a new agent (e.g., "new_agent"):
1. Research the agent's skill directory convention
2. Add entry to `AGENTS` array in `src/data/agents.rs` with correct `skill_dir` and `skill_filename`
3. Add unit test case in `agents.rs`
4. No changes to core logic or feature flag needed

### Tool Definition Extensibility

The `ToolDefinition` struct's fields are `pub` so consumers can modify:
```rust
let mut tool = ane::core::tool_definition();
tool.description = "Custom description".to_string();
// Pass modified tool to LLM API
```

### Skill Content Updates

The skill is embedded at compile time. To update:
1. Edit `skills/ane-skill.md`
2. Rebuild binary
3. Run `ane init <agent>` to propagate to projects

No runtime skill loading mechanism; skill is generated content managed by ane's maintainers.

---

## Conclusion

Work Item 0006 successfully:

✅ Teaches code agents ane's chord grammar via `ane init <agent>`
✅ Provides a clean programmatic API via `ane::core::*`
✅ Gates heavy dependencies behind an opt-in feature flag
✅ Maintains architectural compliance (three-layer dependency hierarchy)
✅ Includes comprehensive tests and error handling
✅ Enables code agents to make precise structured edits without manual prompt engineering
✅ Enables developers to embed ane's engine directly in custom harnesses

All three parts—skill definition, agent initialization, and programmatic API—work together to make ane a first-class tool for both agent-driven workflows and programmatic integration.
