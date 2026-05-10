# Agents

## Agent 1:
Name: Code Agent (external consumer)
Purpose: Any AI coding agent that uses `ane exec` to read and modify files
Model: any
Provider: any
Description:
- Code agents invoke `ane exec --chord "<chord>" <path>` to make precise file edits
- Chords use a 4-part system: Action + Positional + Scope + Component
- Short form (e.g., `cifb`) minimizes token usage; long form (e.g., `ChangeInFunctionBody`) aids readability
- Agents receive unified diff output to confirm what changed
- Chords requiring LSP will fail with a clear error if the language server is not available
Guidance:
- Prefer short form chords for token efficiency
- Use long form when clarity matters more than brevity
- Chords that target language constructs (functions, variables, structs, etc.) require an active LSP
- Line-scoped and file-scoped chords always work without LSP
- The diff output is parseable without any special tooling
- Future chords should follow the 4-part pattern: new Actions, Scopes, and Components extend the matrix
