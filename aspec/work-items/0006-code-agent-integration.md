# Work Item: Feature

Title: code agent integration
Issue: issuelink

## Summary

There are three parts to this work item:

1) A skill definition, consumable by code agents like Claude, Codex, and Gemini, which teaches a code agent when and how to use ane. The skill teaches the agent how to use `ane exec` with the chord system in as few tokens as possible. The skill's markdown contents are embedded in ane's source code (via `include_str!`) so they can be used by part 2.

2) A new CLI subcommand, `ane init <agent>`. When executed, it creates the skill directory in the appropriate location for the provided agent and writes the embedded skill markdown contents into the skill file. Supported agents: claude, codex, gemini, opencode, cline, maki, charm. Each agent has a different skill directory convention that must be researched and implemented.

3) A `pub mod core` module within the `ane` crate (`src/core.rs`) which re-exports a curated API surface for programmatic use — `ChordEngine`, `LspEngine`, `Buffer`, all chord types, and supporting types. Consumers use `ane::core::ChordEngine`, etc. A new `frontends` feature flag (enabled by default) gates the CLI/TUI frontends and their heavy dependencies (`clap`, `crossterm`, `ratatui`). Library consumers depend on `ane` with `default-features = false` to get only the engine, avoiding ~30 transitive crates they don't need.

---

## User Stories

### User Story 1
As a: developer using a code agent (Claude, Codex, Gemini, etc.)

I want to: run `ane init claude` in my project and have ane automatically create a skill file that teaches my agent the chord system

So I can: let my code agent make precise, structured edits via `ane exec` without manually writing prompt instructions or teaching it the chord grammar

### User Story 2
As a: developer building a custom AI agent harness in Rust

I want to: add `ane = { default-features = false }` to my `Cargo.toml` and access `ane::core::ChordEngine`, `ane::core::LspEngine`, `ane::core::Buffer`, and all chord types directly

So I can: embed ane's structured editing and LSP capabilities into my agent without shelling out to the `ane` binary, pulling in TUI dependencies, or duplicating any logic

### User Story 3
As a: code agent receiving the ane skill definition

I want to: receive a minimal-token reference for the chord grammar, `ane exec` invocation patterns, and example chords

So I can: generate correct chord strings on the first attempt without trial-and-error or verbose documentation overhead

---

## Implementation Details

### 1. Skill definition — `skills/ane-skill.md` (new file, project root)

Create a standalone markdown file that teaches a code agent the ane chord system. This file must be as token-efficient as possible while remaining unambiguous. Structure:

- **Header**: one-line description of what ane is and when to use it
- **Invocation**: `ane exec <file> --chord "<chord>"` syntax with exit code semantics (0 = success with diff on stdout, 1 = error on stderr)
- **Chord grammar**: 4-part structure — Action + Positional + Scope + Component — with the short-letter table for each:

```
Actions:    c=Change d=Delete r=Replace y=Yank a=Append p=Prepend i=Insert j=Jump
Positional: i=Inside u=Until a=After b=Before n=Next p=Previous e=Entire o=Outside t=To
Scope:      l=Line b=Buffer f=Function v=Variable s=Struct m=Member d=Delimiter
Component:  b=Beginning c=Contents e=End v=Value p=Parameters a=Arguments n=Name s=Self
```

- **Short form vs long form**: `cifn` = `ChangeInsideFunctionName`
- **Arguments**: `ChordShort(target:"name", value:"text", line:N, cursor:L:C, find:"pat", replace:"rep")` — only include what's needed
- **LSP scopes**: Function, Variable, Struct, Member require LSP; Line, Buffer, Delimiter do not
- **5-8 example chords**: cover common agent use cases (change function body, delete line, yank variable value, replace function name, append to buffer end)
- **Gotchas**: Jump is TUI-only (errors on exec), always provide `cursor` for positional context when using Inside/Until/To, pipe stdin for Replace/Change value

Target: under 400 tokens total. No prose paragraphs — use tables, lists, and terse notation.

### 2. Embed skill in source — `src/data/skill.rs` (new file, Layer 0)

Create a new module in Layer 0 that embeds the skill markdown at compile time:

```rust
pub const SKILL_CONTENT: &str = include_str!("../../skills/ane-skill.md");
```

Add `pub mod skill;` to `src/data/mod.rs`.

This keeps the skill content in Layer 0 (pure data, no logic) so both CLI (Layer 2) and `src/core.rs` can access it without architectural violations.

### 3. Agent directory mappings — `src/data/agents.rs` (new file, Layer 0)

Define the supported agents and their skill directory paths relative to the project root:

```rust
pub struct AgentConfig {
    pub name: &'static str,
    pub skill_dir: &'static str,
    pub skill_filename: &'static str,
}

pub const AGENTS: &[AgentConfig] = &[
    AgentConfig { name: "claude",   skill_dir: ".claude/skills/ane",              skill_filename: "SKILL.md" },
    AgentConfig { name: "codex",    skill_dir: ".codex/skills/ane",               skill_filename: "SKILL.md" },
    AgentConfig { name: "gemini",   skill_dir: ".gemini/skills/ane",              skill_filename: "SKILL.md" },
    AgentConfig { name: "opencode", skill_dir: ".opencode/skills/ane",            skill_filename: "SKILL.md" },
    AgentConfig { name: "cline",    skill_dir: ".cline/skills/ane",               skill_filename: "SKILL.md" },
    AgentConfig { name: "maki",     skill_dir: ".maki/skills/ane",                skill_filename: "SKILL.md" },
    AgentConfig { name: "charm",    skill_dir: ".charm/skills/ane",               skill_filename: "SKILL.md" },
];
```

Add `pub mod agents;` to `src/data/mod.rs`.

**IMPORTANT**: The skill directory paths above are placeholders. Each agent has its own convention for where user-defined skills are loaded from. Research must be done at implementation time to determine the correct paths. For example:
- **Claude**: `.claude/skills/<name>/SKILL.md` (confirmed — Claude Code loads skills from `.claude/skills/`)
- **Codex**: research required — check Codex CLI docs for custom tool/skill loading
- **Gemini**: research required — check Gemini Code Assist / Jules docs
- **OpenCode**: research required — check opencode GitHub repo for skill/tool conventions
- **Cline**: research required — check Cline extension docs for custom instructions/skills
- **Maki**: research required — check maki repo/docs
- **Charm**: research required — check charm repo/docs

If an agent does not support auto-loading skills from a directory, note this in the `AgentConfig` with documentation, and have `ane init <agent>` print a message explaining how to manually include the skill (e.g., "Add the following to your .clinerc instructions file: ...").

Add a lookup function:

```rust
pub fn find_agent(name: &str) -> Option<&'static AgentConfig> {
    AGENTS.iter().find(|a| a.name.eq_ignore_ascii_case(name))
}

pub fn agent_names() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.name).collect()
}
```

### 4. CLI subcommand — `src/frontend/cli.rs` (Layer 2)

Add `Init` variant to the `Command` enum:

```rust
#[derive(Subcommand, Debug)]
pub enum Command {
    Exec { ... },
    /// Initialize ane skill for a code agent (e.g. `ane init claude`)
    Init {
        /// Agent name (claude, codex, gemini, opencode, cline, maki, charm)
        #[arg()]
        agent: String,
    },
}
```

### 5. Init handler — `src/main.rs`

Add the `Init` match arm in `main()`:

```rust
Some(Command::Init { agent }) => {
    if let Err(e) = run_init(&agent) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
```

Implement `run_init`:

```rust
fn run_init(agent_name: &str) -> Result<()> {
    let config = ane::data::agents::find_agent(agent_name)
        .ok_or_else(|| anyhow::anyhow!(
            "unknown agent '{}'. Supported: {}",
            agent_name,
            ane::data::agents::agent_names().join(", ")
        ))?;

    let skill_dir = std::path::Path::new(config.skill_dir);
    std::fs::create_dir_all(skill_dir)?;

    let skill_path = skill_dir.join(config.skill_filename);
    std::fs::write(&skill_path, ane::data::skill::SKILL_CONTENT)?;

    println!("wrote ane skill to {}", skill_path.display());
    Ok(())
}
```

The function writes relative to the current working directory (project root). No confirmation prompt — the file is small and idempotent (re-running overwrites with the latest embedded skill).

### 6. `frontends` feature flag — `Cargo.toml`

Add a `frontends` feature that gates all CLI/TUI dependencies. The binary requires this feature; library consumers opt out via `default-features = false`.

Update `Cargo.toml`:

```toml
[features]
default = ["frontends"]
frontends = ["dep:clap", "dep:crossterm", "dep:ratatui"]
test-support = []

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
```

Add `required-features` to the main binary so it is only built when frontends is enabled:

```toml
[[bin]]
name = "ane"
path = "src/main.rs"
required-features = ["frontends"]
```

The `mock_lsp_server` binary does not use frontends and remains unconditional.

### 7. Conditional frontend module — `src/lib.rs`

Gate the frontend module behind the feature flag:

```rust
pub mod commands;
pub mod core;
pub mod data;

#[cfg(feature = "frontends")]
pub mod frontend;
```

### 8. `pub mod core` — `src/core.rs` (new file, Layer 0/1 re-exports)

Create a curated public API module that re-exports everything a programmatic consumer needs:

```rust
pub use crate::commands::chord::{execute_chord, parse_chord, ChordResult, FrontendCapabilities};
pub use crate::commands::chord_engine::errors::ChordError;
pub use crate::commands::chord_engine::types::{
    ChordAction, ChordArgs, ChordQuery, ResolvedChord, TextRange,
};
pub use crate::commands::chord_engine::ChordEngine;
pub use crate::commands::diff::unified_diff;
pub use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
pub use crate::data::buffer::Buffer;
pub use crate::data::chord_types::{Action, Component, Positional, Scope};
pub use crate::data::skill::SKILL_CONTENT;
```

This module is always available regardless of the `frontends` feature.

**Feasibility check**: all re-exported types must be `pub` and reachable through `pub mod` chains. Currently `src/lib.rs` exposes `pub mod commands` and `pub mod data`, and the relevant submodules use `pub` visibility. Verify at implementation time that every type listed above is reachable via `crate::` paths. If any internal types need visibility changes, those are confined to adding `pub` to existing items — no architectural restructuring. If the re-export chain breaks due to private intermediate modules, stop and report the issue rather than adding hacks.

**Naming note**: `core` shadows Rust's `core` crate within `src/core.rs`. This is harmless in a `std`-based crate since no code references `core::` directly. If a future module needs `core::`, it can use `::core::` for the language crate.

Consumer usage:

```toml
# Cargo.toml — agent harness that embeds ane
[dependencies]
ane = { version = "0.1", default-features = false }
```

```rust
use ane::core::{Buffer, ChordEngine, LspEngine, LspEngineConfig, parse_chord};
```

### 9. Tool definition function — `src/core.rs`

Add a `ToolDefinition` struct and a `pub fn tool_definition()` that returns a ready-to-serialize tool schema. This gives harness developers a one-liner to add ane to their LLM's tool list.

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

const TOOL_DESCRIPTION: &str = "\
Execute a structured chord edit on a file using ane's chord grammar.\n\
\n\
Chords are 4 characters: Action + Positional + Scope + Component.\n\
\n\
Actions:    c=Change d=Delete r=Replace y=Yank a=Append p=Prepend i=Insert\n\
Positional: i=Inside e=Entire a=After b=Before n=Next p=Previous\n\
Scope:      l=Line b=Buffer f=Function v=Variable s=Struct m=Member\n\
Component:  b=Beginning c=Contents e=End v=Value p=Parameters n=Name s=Self\n\
\n\
Args in parens: chord(target:fn_name, line:N)\n\
Use the value parameter (not inline) for replacement text.\n\
\n\
Examples:\n\
  cels(line:3) + value → change line 3\n\
  dels(line:5) → delete line 5\n\
  cifn(function:getData) + value → rename function\n\
  aale(line:10) + value → append after line 10\n\
  yefc(function:main) → yank function body\n\
  rifc(function:handler) + value → replace function contents";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "ane".to_string(),
        description: TOOL_DESCRIPTION.to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "chord": {
                    "type": "string",
                    "description": "Chord expression, e.g. \"cels(line:3)\" or \"cifn(function:getData)\""
                },
                "value": {
                    "type": "string",
                    "description": "Text for Change/Replace/Append/Insert actions. Preferred over inline value arg for multiline content."
                }
            },
            "required": ["file_path", "chord"]
        }),
    }
}
```

The `value` parameter is separate from the chord string rather than inline (e.g. `chord(value:"...")`) because:
1. LLMs produce cleaner output when multiline text is a top-level JSON string rather than escaped-within-escaped quotes
2. The harness injects `value` into `ChordArgs.value` before calling `execute_chord`, avoiding the stdin pipe mechanism that non-interactive tool calls can't use

The `ToolDefinition` struct serializes to a shape compatible with the Anthropic tool format. OpenAI consumers wrap it in `{"type": "function", "function": ...}`. The struct fields are public so consumers can modify `name` or `description` before serializing if needed.

Consumer usage:

```rust
use ane::core::tool_definition;

let tools = vec![serde_json::to_value(tool_definition())?];
// pass `tools` to your LLM API call
```

---

## Edge Case Considerations

### Skill definition edge cases

- **Token budget**: the skill must remain under 400 tokens. Every addition must be weighed against token cost. Prefer tables and short-form notation over prose. Test token count with a tokenizer before finalizing.

- **Chord grammar completeness vs brevity**: the skill should not exhaustively list every valid combination. Instead, teach the pattern and give enough examples that an LLM can generalize. Include only the most common agent use cases (change, delete, yank, replace on function/line/variable scopes).

- **Agent misinterpreting short form**: short-form chords like `cifn` could be misread as arbitrary strings. The skill must explicitly state that short form is exactly 4 lowercase characters mapping to action+positional+scope+component.

- **stdin piping for value**: agents commonly need to provide replacement text. The skill must show the `echo "text" | ane exec file --chord "cilc"` pattern since many agents work non-interactively.

### Init subcommand edge cases

- **Unknown agent name**: `ane init foobar` should list all supported agent names in the error message, not just fail silently.

- **Existing skill file**: `ane init claude` when `.claude/skills/ane/SKILL.md` already exists should overwrite without prompting. The skill is generated content and idempotent — the latest version from the binary is always correct.

- **No write permission**: if the directory cannot be created (e.g., permission denied), the error should clearly state the target path that failed, not just a generic OS error.

- **Agent skill path doesn't exist yet**: `create_dir_all` handles creating the full directory tree. No pre-existence check needed.

- **Case insensitivity**: `ane init Claude` and `ane init CLAUDE` should both work. Use case-insensitive matching in `find_agent`.

- **Running from wrong directory**: `ane init` writes relative to CWD. If the user runs it from outside their project root, the skill directory ends up in the wrong place. This is standard CLI behavior (same as `git init`) and not something to guard against — document it in the `--help` text for the `Init` subcommand.

- **Agent doesn't support auto-loading**: if research reveals that an agent (e.g., charm) doesn't have a standard skill directory, `ane init charm` should still write the file to a reasonable location and print instructions on how to manually include it.

### `frontends` feature flag edge cases

- **Visibility of re-exported types**: if any type in the `core` re-export chain is behind a `pub(crate)` or private module, the re-export will fail to compile. Check all paths at implementation time. `ChordError` in `src/commands/chord_engine/errors.rs` and `TextRange` in `types.rs` need particular attention — verify they are `pub`.

- **`test-support` feature**: the existing `test-support` feature must remain independent of `frontends`. Verify the re-exported types in `src/core.rs` don't require `test-support` to be visible.

- **Binary not built without feature**: with `required-features = ["frontends"]` on the `[[bin]]` target, `cargo build --no-default-features` will skip the binary and only build the library. This is the intended behavior — library consumers never need the binary. Document this in the crate-level README/docs.

- **Downstream feature unification**: if a downstream crate depends on `ane` with `default-features = false` but another crate in the same workspace depends on `ane` with defaults, Cargo unifies features and the downstream crate will get `frontends` too. This is standard Cargo behavior and not something to guard against, but worth noting in documentation.

- **`core` module name shadows Rust's `core` crate**: within `src/core.rs` itself, `core::` refers to the module, not the language crate. If any future code in that file needs `core::fmt::Display` etc., use `::core::`. No existing code in the crate references `core::` directly, so this is not a current concern.

- **Conditional compilation in tests**: any test that uses `frontend::` types must be gated with `#[cfg(feature = "frontends")]`. Existing tests in `src/commands/chord.rs` use `FrontendCapabilities` which lives in the commands layer (not behind the feature gate), so they are unaffected. The `HeadlessContext` test helper also lives in `commands/chord.rs` and remains available.

### Tool definition edge cases

- **`value` injection by harness code**: the `tool_definition()` function defines the schema but does not handle dispatch. When the LLM returns a tool call with a `value` field, the harness must parse the chord via `parse_chord`, set `chord.args.value = Some(value)`, then call `execute_chord`. This is intentionally left to the consumer — ane's core is not opinionated about how tool call results are received or dispatched. The consumer usage example in section 9 and the skill markdown examples make this flow clear.

- **Tool description token budget**: the `TOOL_DESCRIPTION` constant should stay under 250 tokens. It is injected into every LLM request that includes the tool, so token cost compounds. The description teaches the grammar pattern and gives examples — it does not need to cover every combination. Test token count at implementation time.

- **Schema compatibility across providers**: the `ToolDefinition` struct serializes to Anthropic's tool format (`name`, `description`, `input_schema`). OpenAI uses a wrapper (`{"type": "function", "function": {...}}`) with `parameters` instead of `input_schema`. The struct fields are `pub` so consumers can restructure as needed. Do not add provider-specific wrapper functions — that's consumer-side glue.

- **Chord args with and without value**: some chords need `value` (Change, Replace, Append) and some don't (Delete, Yank). The schema marks `value` as optional. If the LLM provides `value` for a Delete chord, the harness sets `chord.args.value` but the resolver ignores it — no error, no side effect.

---

## Test Considerations

### Skill definition tests

- **Token count validation**: write a test or script that tokenizes `skills/ane-skill.md` (using a BPE tokenizer like `tiktoken` or by counting whitespace-delimited words as a proxy) and asserts the count is under 400 tokens.

- **Embed integrity**: unit test in `src/data/skill.rs` that asserts `SKILL_CONTENT` is non-empty and contains key markers (e.g., "ane exec", "Action", "Scope") to catch accidental file deletion or corruption.

### Init subcommand tests

- **Unit: `find_agent` returns correct config for each supported name** — iterate all 7 agent names and assert `find_agent(name).is_some()`.

- **Unit: `find_agent` is case-insensitive** — assert `find_agent("Claude")` and `find_agent("CLAUDE")` both return `Some`.

- **Unit: `find_agent` returns `None` for unknown agents** — `find_agent("vim")` returns `None`.

- **Integration: `run_init` creates directory and writes file** — in a `tempdir`, run `run_init("claude")` and assert:
  - The skill directory exists
  - The skill file exists
  - The file contents equal `SKILL_CONTENT`

- **Integration: `run_init` overwrites existing file** — write a dummy file to the skill path, run `run_init`, assert file contents are now `SKILL_CONTENT`.

- **Integration: `run_init` with unknown agent returns error** — assert error message contains "unknown agent" and the list of supported names.

- **CLI integration: `ane init claude` end-to-end** — run the binary in a tempdir and check the file is created (requires the binary to be built; may be a manual test).

### `core` module and `frontends` feature tests

- **Compile without frontends**: `cargo build --no-default-features` must succeed, producing only the library (no binary). This validates that `src/core.rs`, `src/commands/`, and `src/data/` have no hidden dependencies on frontend crates.

- **Compile with frontends**: `cargo build` (default features) must continue to build the binary and all frontend code. This is the existing build — it must not regress.

- **Unit: re-exports are usable** — write a test in `src/core.rs` that constructs each major type to verify the re-exports are functional:
  ```rust
  #[test]
  fn core_parse_round_trip() {
      let query = crate::core::parse_chord("cifn").unwrap();
      assert_eq!(query.action, crate::core::Action::Change);
  }
  ```

- **Unit: `SKILL_CONTENT` is accessible via core** — assert `crate::core::SKILL_CONTENT.len() > 0`.

- **Test suite without frontends**: `cargo test --no-default-features` must pass. Any test that touches frontend types must be gated with `#[cfg(feature = "frontends")]`.

- **Unit: `tool_definition()` returns valid schema** — assert the returned `ToolDefinition` has `name == "ane"`, non-empty `description`, and `input_schema` contains `"file_path"` and `"chord"` in `required`.

- **Unit: `ToolDefinition` serializes to valid JSON** — `serde_json::to_value(tool_definition())` succeeds and the result contains expected top-level keys (`name`, `description`, `input_schema`).

- **Unit: tool description is under 250 tokens** — count whitespace-delimited words in `TOOL_DESCRIPTION` as a proxy (250 words ≈ 250 tokens for structured text). Assert it stays within budget.

### Manual test checklist

- Run `ane init claude` in a fresh directory — `.claude/skills/ane/SKILL.md` is created with correct content
- Run `ane init claude` again — file is overwritten without error
- Run `ane init foobar` — error message lists all supported agents
- Run `cargo build --no-default-features` — compiles the library without frontend deps
- Run `cargo build` — compiles the full binary with frontends (no regression)
- Run `cargo test --no-default-features` — all non-frontend tests pass
- Run `cargo test` — all tests pass (including frontend-gated tests)
- Feed the skill markdown to a code agent and verify it can generate correct `ane exec` commands from natural language instructions

---

## Codebase Integration

- **Layer 0 additions** (`src/data/`): `skill.rs` (embeds skill markdown via `include_str!`) and `agents.rs` (agent config structs and lookup functions). Both are pure data modules with no imports from Layer 1 or Layer 2. Add `pub mod skill;` and `pub mod agents;` to `src/data/mod.rs`.

- **Layer 2 additions** (`src/frontend/cli.rs`): add `Init { agent: String }` variant to the `Command` enum with clap derive attributes. The agent argument is a positional arg, not a flag — `ane init claude` not `ane init --agent claude`.

- **`src/main.rs`**: add `Init` match arm calling `run_init`. The `run_init` function uses `data::agents::find_agent` and `data::skill::SKILL_CONTENT` — both Layer 0 imports, which are valid from `main.rs`.

- **`frontends` feature flag**: `Cargo.toml` gains a `frontends` feature (default-enabled) that gates `clap`, `crossterm`, and `ratatui` as optional deps. The `[[bin]] ane` target gets `required-features = ["frontends"]` so it is only built when the feature is active. Library consumers use `ane = { default-features = false }` to get only the engine.

- **`src/core.rs`**: new module containing only `pub use` re-exports of the curated programmatic API. Added to `src/lib.rs` as `pub mod core;` (unconditional — always available). No logic, no new types, no external deps.

- **`src/lib.rs` conditional module**: `pub mod frontend` is wrapped in `#[cfg(feature = "frontends")]`. The `commands` and `data` modules remain unconditional.

- **Skill file location**: `skills/ane-skill.md` lives at the project root, outside `src/`. The `include_str!` path in `src/data/skill.rs` uses `../../skills/ane-skill.md` (relative to the source file). This keeps the skill editable as a standalone markdown file without rebuilding just to preview changes.

- **Makefile**: no new targets needed. Existing `build`, `test`, `install`, `clean` targets work unchanged. Consider adding a `build-lib` target (`cargo build --no-default-features`) for CI validation of the library-only build.

- **No changes to existing module logic**: parts 1-2 (skill + init) add new files and a new CLI subcommand without modifying any existing `src/commands/` or `src/frontend/tui/` code. Part 3 (core + feature flag) modifies `Cargo.toml` (deps become optional), `src/lib.rs` (add `pub mod core`, gate `pub mod frontend`), and may require making some types `pub` if they are currently `pub(crate)` — these are additive visibility changes, not behavioral changes.
