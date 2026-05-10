# APIs

Convention: cli
Protocol: stdin/stdout

## Design:

Versioning:
- The chord API is versioned implicitly by the binary version
- Breaking changes to chord syntax require a major version bump

Objects:
- Chords are the primary API surface for programmatic interaction
- Every chord has 4 parts: Action, Positional, Scope, Component
- Short form: 4-character chord (e.g., `cifb` = Change In Function Body)
- Long form: PascalCase concatenation (e.g., `ChangeInFunctionBody`)

### Actions
| Short | Name | Description |
|-------|------|-------------|
| c | Change | Replace targeted content with new value (CLI: accepts text param; TUI: clears and enters Edit mode) |
| r | Replace | Find-and-replace within targeted region |
| d | Delete | Remove targeted content |
| y | Yank | Copy targeted content (no modification) |
| a | Append | Insert content after the target |
| p | Prepend | Insert content before the target |
| i | Insert | Insert content at a specific position within the target |

### Positionals
| Short | Name | Description |
|-------|------|-------------|
| i | Inside | Within the boundaries of the scope (exclusive of delimiters) |
| u | Until | From current position up to (but not including) the component |
| a | After | After the component boundary |
| b | Before | Before the component boundary |
| n | Next | The next occurrence of the scope/component |
| p | Previous | The previous occurrence of the scope/component |
| e | Entire | The full extent of the scope including delimiters |
| o | Outside | Everything outside the scope boundaries (inverse of Inside) |

### Scopes
| Short | Name | Requires LSP | Description |
|-------|------|-------------|-------------|
| l | Line | no | A single line in the buffer |
| b | Buffer | no | The entire buffer (file) |
| f | Function | yes | A function/method definition |
| v | Variable | yes | A variable or constant binding |

### Components
| Short | Name | Description |
|-------|------|-------------|
| b | Beginning | The start boundary of the scope |
| e | End | The end boundary of the scope |
| v | Value | The assigned/returned value |
| p | Parameters | The parameter list (function signatures) |
| a | Arguments | The argument list (call sites) |
| n | Name | The identifier/name |
| s | Self | The entire construct itself |

### Arguments
Arguments are passed as parenthesized key-value pairs after the chord:
- `cifp(function:getData, value:"from: int, to: int")`
- `dufe(line:49)`
- In TUI mode, arguments can be omitted when cursor context is sufficient

### LSP Gating
- Chords that target language constructs (Function, Variable, Struct, etc.) require an active LSP
- Line and File scoped chords work without LSP
- If LSP is pending, the chord waits; if LSP failed, the chord reports an error

Authentication:
- N/A — ane is a local binary. LSP integration is local-only (no network beyond language server downloads).

Conventions:
- Exec mode reads chord from --chord flag, file path as positional arg
- Output is always unified diff format on stdout
- Errors go to stderr
- Exit code 0 on success, non-zero on failure
- "no changes" is printed to stderr (not stdout) when a chord produces no diff
