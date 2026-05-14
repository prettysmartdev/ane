# Exec Mode

Exec mode applies a single chord to a file and exits. It is designed for code agents, scripts, and pipelines that need structured edits without an interactive UI.

---

## Basic usage

```sh
ane exec --chord "<chord>" <file>
```

The chord is applied to the file. A unified diff is written to stdout showing what changed. Yank chords output the selected text instead of a diff.

---

## Examples

```sh
# Change a specific line
ane exec --chord "cels(target:5, value:\"new text\")" path/to/file.rs

# Delete a line
ane exec --chord "dels(target:3)" path/to/file.rs

# Replace a function body
ane exec --chord "cifc(target:init, value:\"    todo!()\")" src/main.rs

# Rename a function
ane exec --chord "cifn(target:get_data, value:\"fetch_data\")" src/lib.rs

# Yank (read) the entire file
ane exec --chord "yebs" path/to/file.rs

# Long form works too
ane exec --chord "ChangeInsideFunctionContents(target:foo, value:\"return 0;\")" path/to/file.rs
```

---

## Piping values from stdin

Use `value:-` to read the replacement text from stdin. This avoids shell quoting issues with multiline content:

```sh
echo "new body" | ane exec --chord "cifc(target:foo, value:-)" path/to/file.rs

cat replacement.txt | ane exec --chord "cifc(target:init, value:-)" src/main.rs
```

---

## Output

- **Mutating chords** (Change, Delete, Replace, Append, Prepend, Insert) write a unified diff to stdout.
- **Yank chords** write the selected text to stdout.
- **Errors** are written to stderr with a nonzero exit code.

---

## Frontend-aware behavior

Not all chords are valid in exec mode. Jump chords (`j`) are rejected before any file I/O because the CLI has no cursor:

```sh
ane exec --chord "jtfc" path/to/file.rs
# Error: Jump action requires an interactive frontend; use ane in TUI mode
```

---

## Agent integration

### Teaching ane to your agent

Generate a token-efficient skill file for your code agent:

```sh
ane init claude      # .claude/skills/ane/SKILL.md
ane init codex       # .codex/skills/ane/SKILL.md
ane init gemini      # .gemini/skills/ane/SKILL.md
```

Supported agents: claude, codex, gemini, opencode, cline, maki, charm.

The skill file (under 400 tokens) teaches the agent ane's chord grammar so it can issue `ane exec` commands. Re-run to update the skill to the latest version.

### Common agent patterns

```sh
# Read a function's contents
ane exec --chord "yefc(target:handle_request)" src/server.rs

# Replace a function body with agent-generated code
echo "$GENERATED_CODE" | ane exec --chord "cifc(target:handle_request, value:-)" src/server.rs

# Add a new function after an existing one
echo "$NEW_FUNCTION" | ane exec --chord "aefe(target:existing, value:-)" src/lib.rs

# Delete a deprecated function
ane exec --chord "defs(target:old_handler)" src/server.rs
```

---

## LSP in exec mode

Exec mode auto-detects the project language and starts the language server, just like TUI mode. LSP-scoped chords (Function, Variable, Struct, Member) wait for the server to be ready before executing. Non-LSP chords (Line, Buffer, Delimiter) execute immediately.

---

[<- Using the TUI](03-using-the-tui.md) | [Next: Embedding via Crate ->](05-embedding-via-crate.md)
