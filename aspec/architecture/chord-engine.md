# ChordEngine

Layer: 1 (commands)
Module: `src/commands/chord_engine/`
Dependencies: Layer 0 (`data::chord_types`, `data::buffer`), Layer 1 (`commands::lsp_engine`)

## Purpose

The ChordEngine is ane's composable editing pipeline. It takes named buffers and a chord string as input and produces a `ChordAction` describing the exact changes to apply. It is self-contained: it does not read files from disk, modify buffers, or interact with the frontend. Its sole job is to transform a chord + buffer contents into a precise, structured patch.

The engine is designed so that **no individual chord combination has its own function**. Instead, three composable stages — Parser, Resolver, Patcher — each handle their dimension of the problem independently, and the stages compose to cover the full matrix of chord combinations.

## Chord Grammar

### Vocabulary

#### Actions
| Short | Long | Description |
|-------|------|-------------|
| c | Change | Replace targeted content with new value (or freeform edit for interactive frontends) |
| r | Replace | Find-and-replace within new value (concrete new value required) |
| d | Delete | Remove targeted content |
| y | Yank | Copy targeted content (no modification) (use system clipboard) |
| a | Append | Insert content after the target |
| p | Prepend | Insert content before the target |
| i | Insert | Insert content at a specific position within the target |
| j | Jump | Jump cursor to (causes error for non-interactive frontends, not in the chord engine) |

#### Positionals
| Short | Long | Description |
|-------|------|-------------|
| i | Inside | Within the boundaries of the scope (exclusive of delimiters) |
| u | Until | From current position up to (but not including) the component |
| a | After | After the component boundary |
| b | Before | Before the component boundary |
| n | Next | The next occurrence of the scope/component |
| p | Previous | The previous occurrence of the scope/component |
| e | Entire | The full extent of the scope including delimiters |
| o | Outside | Everything outside the scope boundaries (inverse of Inside) |

#### Scopes
| Short | Long | Requires LSP | Description |
|-------|------|-------------|-------------|
| l | Line | no | A single line in the buffer |
| b | Buffer | no | The entire buffer (file) |
| f | Function | yes | A function/method definition |
| v | Variable | yes | A variable or constant binding |
| s | Struct | yes | A struct, class, or similar named aggregate type definition |
| m | Member | yes | A member of an aggregate: a struct field, enum variant, or similar named child of a type definition |

#### Components
| Short | Long | Description |
|-------|------|-------------|
| b | Beginning | The start boundary of the scope |
| e | End | The end boundary of the scope |
| v | Value | The inner or assigned value (function body, variable or constant assigned value) |
| p | Parameters | The parameter list (function signatures) |
| a | Arguments | The argument list (call sites) |
| n | Name | The identifier/name of function, variable, constant, etc. |
| s | Self | The entire construct itself (the scope as a whole) |

### Syntax Forms

**Short form**: 4 characters, one from each dimension: `{action}{positional}{scope}{component}`
- Example: `dufe` → DeleteUntilFunctionEnd
- Example: `cifp` → ChangeInsideFunctionParameters
- Example: `aale` → AppendAfterLineEnd
- Example: `jnfv` -> JumpNextFunctionValue

**Long form**: PascalCase concatenation of full names.
- Example: `DeleteUntilFunctionEnd`
- Example: `ChangeInsideFunctionParameters`
- Example: `AppendAfterLineEnd`

**Arguments**: Passed as parenthesized key-value pairs after the chord name.
- `cifp(target:getData, value:"from: int, to: int")`
- `dufe(target:49)`
- `dols` (no arguments — operates on cursor context)
- `jnfv` (no arguments - operates on cursor itself)

**Cursor context**: In TUI mode, arguments may be omitted when the cursor provides context. The resolver receives a `resolve_cursor_pos[line, col]` directive and determines the target from buffer analysis and LSP data.

### Examples

| Chord | Meaning |
|-------|---------|
| `dufe` | Delete from cursor until end of current function |
| `yefv` | Yank the entire function's value (body) |
| `aale` | Append content after the end of the current line |
| `cblb` | Change from before the beginning of the line (prepend-replace from buffer start to line start) |
| `dols` | Delete everything outside the current line's self (delete all other lines) |
| `cifp(target:getData, value:"(x: i32, y: i32)")` | Change inside function parameters for getData |
| `ribs` | Replace inside the buffer self (find-and-replace in entire file) |
| `cisv` | Change inside struct value (edit the body of a struct definition) |
| `dimn` | Delete inside member name (delete a field or variant's identifier) |
| `yemv` | Yank entire member value (copy a struct field's type or enum variant's data) |

## Pipeline Architecture

```
 Input                    Stage 1              Stage 2              Stage 3              Output
┌──────────┐         ┌──────────┐         ┌──────────┐         ┌──────────┐         ┌──────────────┐
│ chord    │         │          │         │          │         │          │         │              │
│ string   │────────▶│  Parser  │────────▶│ Resolver │────────▶│ Patcher  │────────▶│ ChordAction  │
│ +        │         │          │         │          │         │          │         │ (per buffer) │
│ buffers  │         └──────────┘         └──────────┘         └──────────┘         └──────────────┘
└──────────┘         ChordQuery           ResolvedChord         HashMap<String,
                                                                 ChordAction>
```

### Stage 1: Parser

**Input**: Raw chord string (short form or long form, with optional arguments).
**Output**: `ChordQuery`

The parser is purely syntactic. It does not look at buffer contents or LSP data. Its responsibilities:

1. Detect whether input is short form or long form.
2. Tokenize and validate each of the 4 chord dimensions.
3. Parse parenthesized arguments into a structured map.
4. Validate that the combination is semantically coherent (e.g., `YankInsideLineParameters` is nonsensical — lines don't have parameters).
5. Produce descriptive error messages on failure, including suggestions for similar valid chords.

```rust
pub struct ChordQuery {
    pub action: Action,
    pub positional: Positional,
    pub scope: Scope,
    pub component: Component,
    pub args: ChordArgs,
    pub requires_lsp: bool,
}

pub struct ChordArgs {
    pub target_name: Option<String>,    // e.g., function name, variable name
    pub target_line: Option<usize>,     // explicit line number
    pub cursor_pos: Option<(usize, usize)>,  // (line, col) from TUI cursor
    pub value: Option<String>,          // replacement/insertion text
}
```

**Validation rules** (enforced at parse time):
- Components must be valid for the given scope (e.g., `Parameters` is valid for `Function`, not for `Line`).
- Actions that produce no output require compatible positionals (e.g., `Yank` + `Until` is valid; `Delete` + `Entire` + `Buffer` + `Self` is valid but destructive — flag with warning).
- Short form must be exactly 4 characters (before the argument parentheses).

**Error messages** follow a consistent format:
```
chord error: invalid component 'Parameters' for scope 'Line'
  chord: cipb
  help: Line scope supports components: Beginning, End, Self
  did you mean: cifp (ChangeInsideFunctionParameters)?
```

### Stage 2: Resolver

**Input**: `ChordQuery` + named buffers (`HashMap<String, Buffer>`) + LSP access (via `LspEngine`).
**Output**: `ResolvedChord`

The resolver's job is to determine the exact byte/line/col ranges in each buffer that the chord targets. It answers every spatial question the patcher will need.

**Resolution process**:

1. **Scope Resolution**: Determine which entity the chord targets.
   - For `Line` scope: use `target_line` from args, or `cursor_pos` line.
   - For `Buffer` scope: the entire buffer content.
   - For `Function`/`Variable` scope: query `LspEngine::document_symbols()` and `LspEngine::symbol_range()` to find the construct by name or by cursor position.
   - For `Struct` scope: query `LspEngine::document_symbols()` to find the struct/class/type definition by name or cursor position. The scope range covers the entire type definition including its body.
   - For `Member` scope: query `LspEngine::document_symbols()`, find the containing struct or enum, then locate the specific field or variant by name or cursor position within its children. The scope range covers the individual member declaration.

2. **Component Resolution**: Within the resolved scope, find the specific component.
   - `Beginning`: the first character position of the scope.
   - `End`: the last character position of the scope.
   - `Name`: the identifier span (from LSP symbol data).
   - `Parameters`: the parameter list span (opening paren through closing paren, exclusive).
   - `Arguments`: the argument list span at a call site.
   - `Value`: the RHS of an assignment, the body of a function, the type annotation of a struct field, or the associated data of an enum variant.
   - `Self`: the entire scope span.

3. **Positional Resolution**: Apply the positional modifier to determine the final target range.
   - `Inside`: content between delimiters of the component (e.g., inside parens, inside braces).
   - `Until`: from cursor/start position up to the component boundary.
   - `After`: content following the component's end boundary.
   - `Before`: content preceding the component's start boundary.
   - `Entire`: the full span of the component including any delimiters.
   - `Next`/`Previous`: finds the next/previous occurrence of the scope starting from cursor position or named entity.
   - `Outside`: inverse of Inside — everything outside the scope/component.

4. **Multi-buffer iteration**: If multiple buffers are provided, the resolver iterates each one independently. For LSP-dependent scopes, it queries symbols per buffer using the buffer's key as the file path passed to the LSP. **The keys in the `HashMap<String, Buffer>` must be absolute file paths identical to those the LSPEngine uses** — the ChordEngine has no knowledge of the filesystem and cannot resolve or canonicalize paths itself. The caller is responsible for ensuring path agreement between the buffer map and the LSP.

```rust
pub struct ResolvedChord {
    pub query: ChordQuery,
    pub resolutions: HashMap<String, BufferResolution>,
}

pub struct BufferResolution {
    pub target_range: TextRange,        // exact range to operate on
    pub scope_range: TextRange,         // full range of the containing scope
    pub replacement: Option<String>,    // resolved replacement text (if action needs it)
    pub cursor_destination: Option<CursorPosition>,  // where cursor should land after
    pub mode_after: Option<EditorMode>, // what mode to enter after (Edit, Chord)
}

pub struct TextRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

pub struct CursorPosition {
    pub line: usize,
    pub col: usize,
}

pub enum EditorMode {
    Edit,
    Chord,
}
```

**Cursor context resolution**: When `cursor_pos` is set but `target_name` is not, the resolver:
1. Queries LSP for symbols at the cursor position.
2. Walks up the symbol tree to find the innermost scope matching the chord's scope type.
3. Resolves as if that symbol's name had been provided explicitly.

**Error cases**:
- Symbol not found at cursor position → descriptive error with nearby symbols listed.
- LSP not ready → return error indicating LSP state.
- Ambiguous target (e.g., multiple overloads) → error listing candidates.

### Stage 3: Patcher

**Input**: `ResolvedChord`
**Output**: `HashMap<String, ChordAction>` (buffer name → action)

The patcher generates the concrete diff and metadata for each buffer. It does not modify buffers — it produces a description of what to do.

**Patch generation per action** (all diffs should be git-compatible):
- `Change`: generate a diff replacing `target_range` content with `replacement` value. If no replacement is provided, set `cursor_destination` to the start of the cleared range and `mode_after` to `Edit` (placeholder for TUI).
- `Delete`: generate a diff removing `target_range` content. Cursor moves to start of deleted range.
- `Replace`: generate a diff with find-and-replace within `target_range`.
- `Yank`: no diff. Populate `yanked_content` with the text in `target_range`.
- `Append`: generate a diff inserting `replacement` after `target_range.end`.
- `Prepend`: generate a diff inserting `replacement` before `target_range.start`.
- `Insert`: generate a diff inserting `replacement` at cursor position within `target_range`.

```rust
pub struct ChordAction {
    pub buffer_name: String,
    pub diff: Option<UnifiedDiff>,
    pub yanked_content: Option<String>,
    pub cursor_destination: Option<CursorPosition>,
    pub mode_after: Option<EditorMode>,
    pub highlight_ranges: Vec<TextRange>,  // ranges to briefly highlight in TUI
    pub warnings: Vec<String>,             // non-fatal warnings (e.g., "this deletes entire file")
}

pub struct UnifiedDiff {
    pub hunks: Vec<DiffHunk>,
}

pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}
```

## Engine Entry Point

```rust
pub struct ChordEngine;

impl ChordEngine {
    // Full pipeline: parse → resolve → patch.
    // Returns one ChordAction per buffer.
    pub fn execute(
        chord_input: &str,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
    ) -> Result<HashMap<String, ChordAction>>;

    // Expose individual stages for testing and advanced use.
    pub fn parse(chord_input: &str) -> Result<ChordQuery>;
    pub fn resolve(
        query: &ChordQuery,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
    ) -> Result<ResolvedChord>;
    pub fn patch(resolved: &ResolvedChord) -> Result<HashMap<String, ChordAction>>;
}
```

The engine is stateless — it holds no data between calls. All state comes from the inputs (buffers, LSP).

**Caller contract**: The keys in the `buffers` map must be absolute file paths that match the paths known to the `LspEngine`. The ChordEngine does not touch the filesystem — it cannot resolve relative paths, follow symlinks, or discover where a buffer lives on disk. When the resolver queries LSP for symbol data, it passes these keys directly as document URIs. If the caller provides a key like `"main.rs"` but the LSP knows the file as `"/home/user/project/src/main.rs"`, symbol resolution will fail. The frontend (CLI or TUI) is responsible for providing canonical absolute paths when constructing the buffer map.

## Scalability of the Grammar

The engine avoids a combinatorial explosion by keeping each stage dimension-independent:

- **Parser** validates each dimension's value independently, then checks a small set of compatibility rules (scope × component validity). Adding a new action, positional, scope, or component requires adding one enum variant and updating the compatibility matrix — not writing new chord functions.
- **Resolver** dispatches on scope (6 branches) and component (7 branches) independently. A new scope adds one resolution strategy. A new component adds one sub-range finder.
- **Patcher** dispatches on action (7 branches). Each action's patch logic is generic over scope/component because the resolver has already computed exact ranges.

Total dispatch paths: `6 (scope resolution) × 7 (component resolution) × 7 (action patching) = 294` — but these are composed from `6 + 7 + 7 = 20` independent implementations, not 294 unique functions.

## Error Handling Strategy

Every stage produces structured errors with context:

```rust
pub enum ChordError {
    ParseError {
        input: String,
        position: usize,
        message: String,
        suggestion: Option<String>,
    },
    ResolveError {
        query: ChordQuery,
        buffer_name: String,
        message: String,
        available_symbols: Vec<String>,
    },
    PatchError {
        query: ChordQuery,
        buffer_name: String,
        message: String,
    },
    LspRequired {
        query: ChordQuery,
        lsp_state: String,
    },
}
```

All errors include enough context for the frontend to display a helpful message. The CLI prints them to stderr. The TUI displays them in a dialog.

## Performance Considerations

- **Lazy resolution**: When multiple buffers are provided, the resolver processes them independently. If the chord's scope is `Line` and only one buffer has the target line, others produce no-op results quickly.
- **Symbol caching**: The resolver may cache `document_symbols` results for a buffer within a single `execute()` call to avoid redundant LSP roundtrips when resolving multiple chords in sequence.
- **Diff generation**: The patcher uses line-level diffs (via the `similar` crate) for minimal, readable output. For large files, only the affected hunk is computed, not a full-file diff.
- **No cloning buffers**: The engine takes `&Buffer` references and reads content without copying entire file contents.

## Integration Points

### Frontend Traits (Layer 2)
The frontend receives a `ChordAction` and applies it according to its mode:
- **CLI**: Applies the diff to disk, prints the unified diff to stdout. Errors on cursor/mode fields as those are interactive-only.
- **TUI**: Applies the diff to the in-memory buffer, moves cursor to `cursor_destination`, enters `mode_after`, re-renders, highlights `highlight_ranges` briefly.

### LSPEngine (Layer 1)
The resolver calls LSPEngine for symbol data when the chord targets a scope with `requires_lsp: true`. If LSPEngine is not ready, the resolver returns `ChordError::LspRequired`. The buffer map keys passed to ChordEngine must be the same absolute file paths that the LSPEngine uses for document identification — the ChordEngine forwards these keys directly to LSP calls without modification.

### Data Layer (Layer 0)
Reads `Buffer` content and `chord_types` enum definitions. Does not write to Layer 0.
