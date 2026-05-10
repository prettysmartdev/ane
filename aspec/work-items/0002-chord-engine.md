# Work Item: Feature

Title: Implement ChordEngine — composable 3-stage chord execution pipeline
Issue: N/A

## Summary:
- Build the ChordEngine component as described in `aspec/architecture/chord-engine.md`
- Replaces the current monolithic `parse_chord()` / `execute_chord()` in `src/commands/chord.rs` with a 3-stage pipeline: Parser → Resolver → Patcher
- Implements the updated chord grammar (Actions: c/r/d/y/a/p/i, Positionals: i/u/a/b/n/p/e/o, Scopes: l/b/f/v/s/m, Components: b/e/v/p/a/n/s)
- Produces `ChordAction` objects that frontends consume — the engine never modifies buffers or interacts with the UI
- Operates over multiple named buffers, returning a `HashMap<String, ChordAction>`

## User Stories

### User Story 1:
As a: Developer

I want to:
Type a chord like `cifp` in the TUI and have the engine figure out which function my cursor is in, locate its parameter list, and prepare an action that clears the parameters and puts me in edit mode at that position

So I can:
Edit function signatures with a single chord instead of manual navigation and selection

### User Story 2:
As a: Code Agent

I want to:
Run `ane exec --chord 'rifv(function:getData, value:"(x: i32) -> String")' src/main.rs` and receive a unified diff on stdout showing the function's return value was replaced

So I can:
Make precise, semantic code edits with minimal tokens and without parsing the file myself

### User Story 3:
As a: Developer

I want to:
Receive a clear, helpful error when I type an invalid chord, including what went wrong and what I probably meant

So I can:
Learn the chord grammar quickly and recover from typos without consulting documentation

### User Story 4:
As a: Code Agent

I want to:
Apply a chord across multiple open buffers simultaneously and receive a per-file diff map

So I can:
Perform cross-file refactors (e.g., rename a function and update call sites) in a single operation

## Implementation Details:

### Research Phase
- Study the current chord implementation in `src/commands/chord.rs` and `src/data/chord_types.rs` to understand what exists and what needs to change
- Research parser design patterns suitable for a fixed-length grammar with optional arguments: recursive descent vs. table-driven parsing for the 4-position chord, standard approaches for parenthesized argument parsing
- Study how composable editing operations work in other editors (vim's operator-motion-text-object model, kakoune's selection-first model, helix's similar approach) to validate the Action-Positional-Scope-Component decomposition
- Research the `similar` crate's API for generating unified diffs from string pairs — determine the best approach for line-level vs character-level diffs
- Study LSP `DocumentSymbol` response structure to understand how to extract component ranges (parameter lists, function bodies, variable values, names) from symbol data
- Research how to determine "what is at this cursor position" from a symbol tree — innermost-containing-range algorithm

### Update Layer 0 Types (`src/data/chord_types.rs`)
- Replace the existing `Action` enum variants with the new set: `Change`, `Replace`, `Delete`, `Yank`, `Append`, `Prepend`, `Insert`
- Replace the existing `Positional` enum variants: `Inside`, `Until`, `After`, `Before`, `Next`, `Previous`, `Entire`, `Outside`
- Replace `Scope` variants: `Line`, `Buffer`, `Function`, `Variable`, `Struct`, `Member`
- Replace `Component` variants: `Beginning`, `End`, `Value`, `Parameters`, `Arguments`, `Name`, `Self_` (using `Self_` since `Self` is a Rust keyword)
- Update all `short()`, `from_short()`, `Display` implementations for the new variants
- Update `Scope::requires_lsp()` — `Line` and `Buffer` return false, `Function`, `Variable`, `Struct`, and `Member` return true
- Add a scope-component compatibility matrix: a `fn is_valid_combination(scope: Scope, component: Component) -> bool` that encodes which components are meaningful for which scopes
- Remove `ChordSpec` — it is replaced by `ChordQuery` in the engine
- Keep the module in Layer 0 since it defines data types only

### Stage 1: Parser (`src/commands/chord_engine/parser.rs`)
- Implement short form parsing: consume exactly 4 characters, map each to its enum variant
- Implement long form parsing: strip PascalCase prefixes in order (Action → Positional → Scope → Component)
- Implement argument parsing: detect `(...)` after the chord name, parse comma-separated `key:value` pairs, handle quoted string values
- Build `ChordQuery` struct with parsed chord dimensions + `ChordArgs`
- Validate scope-component compatibility at parse time
- Generate structured `ChordError::ParseError` with:
  - The exact input and position of the error
  - A human-readable message explaining what's wrong
  - A suggestion for a valid chord if the input is close (Levenshtein distance on short form, prefix matching on long form)
- Handle edge cases: extra whitespace, missing arguments for actions that need them, trailing garbage after valid chord

### Stage 2: Resolver (`src/commands/chord_engine/resolver.rs`)
- Accept `ChordQuery` + `HashMap<String, Buffer>` + `&mut LspEngine`
- The keys in the buffer map must be absolute file paths matching those the LSPEngine uses for document identification. The resolver passes these keys directly to LSP calls — it does not resolve, canonicalize, or otherwise touch the filesystem. The caller (frontend) is responsible for constructing the map with correct absolute paths.
- For each buffer, determine the target range based on scope:
  - **Line**: direct lookup by line number from args or cursor position
  - **Buffer**: the entire buffer content (lines 0 to end)
  - **Function**: query LSPEngine for `document_symbols()`, find the function by name (from args) or by cursor position (walk symbol tree to find innermost function containing cursor)
  - **Variable**: same as Function but filter for variable/const symbols
  - **Struct**: query LSPEngine for `document_symbols()`, find the struct/class/type definition by name or cursor position. Scope range covers the entire definition including body.
  - **Member**: query LSPEngine for `document_symbols()`, find the containing struct or enum, then locate the specific field or variant by name or cursor position among its children. Scope range covers the individual member declaration. If targeted by name, the name should match a child symbol of the nearest enclosing type; if by cursor, walk the symbol tree to find the innermost field/variant at cursor.
- Within the resolved scope, locate the component:
  - **Beginning**: first character of the scope range → point range (line, col 0 or first non-whitespace)
  - **End**: last character of the scope range → point range
  - **Name**: the identifier span from LSP symbol data
  - **Parameters**: for functions, the span from opening `(` to closing `)` of the parameter list. This requires either LSP detailed range data or a lightweight parenthesis-matching scan of the buffer content within the function's signature range
  - **Arguments**: similar to Parameters but at call sites — may require searching the buffer for call expressions
  - **Value**: for variables, the RHS after `=`; for functions, the body between `{` and `}`; for structs, the body between `{` and `}`; for members, the type annotation of a struct field or the associated data of an enum variant
  - **Self**: the entire scope range (equivalent to the scope itself)
- Apply the positional modifier to compute the final `TextRange`:
  - **Inside**: the content between the component's delimiters (exclusive of `(`, `)`, `{`, `}`)
  - **Until**: from cursor position to the component boundary (exclusive)
  - **After**: from after the component's end to the end of the scope (or line)
  - **Before**: from start of scope (or line) to before the component's start
  - **Entire**: the component's full span including delimiters
  - **Next**: scan forward from cursor to find the next occurrence of scope
  - **Previous**: scan backward from cursor to find the previous occurrence
  - **Outside**: everything in the scope except the component (may produce a multi-range result)
- Determine `cursor_destination` and `mode_after` based on the action:
  - `Change` with no value → cursor at start of cleared range, mode → Edit
  - `Delete` → cursor at start of deleted range, mode → Chord
  - `Append`/`Prepend`/`Insert` with no value → cursor at insertion point, mode → Edit
  - `Yank` → cursor unchanged, mode unchanged
- Build `ResolvedChord` with a `BufferResolution` per buffer
- Handle cursor context: `resolve_cursor_pos[line, col]` input triggers LSP-based scope discovery

### Stage 3: Patcher (`src/commands/chord_engine/patcher.rs`)
- Accept `ResolvedChord`, produce `HashMap<String, ChordAction>`
- For each `BufferResolution`:
  - Compute the old content (extract from buffer at `target_range`)
  - Compute the new content based on action type:
    - `Change`: replace old content with `replacement` (or empty if no replacement — placeholder)
    - `Replace`: find-and-replace within old content (requires search/replace args)
    - `Delete`: new content is empty (remove the range)
    - `Yank`: no new content, populate `yanked_content`
    - `Append`: insert `replacement` after the range
    - `Prepend`: insert `replacement` before the range
    - `Insert`: insert `replacement` at cursor position within the range
  - Generate `UnifiedDiff` from old/new buffer content using the `similar` crate
  - Populate `highlight_ranges` with the changed regions for TUI highlighting
  - Populate `warnings` for destructive operations (e.g., deleting entire buffer)
- Build `ChordAction` with diff, cursor info, mode info, and any yanked content

### Module Structure
```
src/commands/chord_engine/
├── mod.rs          # ChordEngine struct, execute/parse/resolve/patch methods, re-exports
├── parser.rs       # Stage 1: chord string → ChordQuery
├── resolver.rs     # Stage 2: ChordQuery + buffers + LSP → ResolvedChord
├── patcher.rs      # Stage 3: ResolvedChord → HashMap<String, ChordAction>
├── types.rs        # ChordQuery, ChordArgs, ResolvedChord, BufferResolution, ChordAction, UnifiedDiff, etc.
└── errors.rs       # ChordError enum with ParseError, ResolveError, PatchError variants
```

### Frontend Integration
- Update `src/frontend/traits.rs`: replace `ParsedChord` parameter with `ChordAction` in all frontend trait methods
- The trait methods no longer need to understand chord semantics — they receive a fully resolved action with exact diffs, cursor positions, and mode transitions
- **Caller responsibility for buffer paths**: When the frontend constructs the `HashMap<String, Buffer>` to pass to ChordEngine, the keys must be absolute file paths identical to those the LSPEngine knows. The ChordEngine has no filesystem access and cannot resolve paths — it forwards buffer keys directly to LSP calls. Both CLI and TUI frontends must canonicalize paths before building the buffer map.
- **CLI frontend**: receives `ChordAction`, applies the diff to the file on disk, prints the unified diff to stdout, exits
- **TUI frontend**: receives `ChordAction`, applies the diff to the in-memory buffer, moves cursor, changes mode, highlights changed ranges, re-renders
- Add new traits for new actions: `AppendFrontend`, `PrependFrontend`, `ReplaceFrontend` (or collapse into a single `ApplyChordAction` trait since the action is now fully described by `ChordAction`)
- Consider simplifying to a single `trait ApplyChordAction { fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<()>; }` since the ChordAction already encodes all the behavior

### Migration from Current Implementation
- The current `src/commands/chord.rs` (`parse_chord`, `execute_chord`, `ParsedChord`, `ChordResult`) will be replaced
- The current `src/data/chord_types.rs` enums will be updated in place (same file, new variants)
- Existing tests in `src/commands/chord.rs` should be migrated to the new parser tests, updating assertions for the new grammar
- The `execute_chord` function's direct buffer manipulation logic moves into the resolver + patcher stages

## Edge Case Considerations:
- Short form ambiguity: the new grammar has no collisions (each position has unique characters), but validate this exhaustively in tests
- `Self` as a component name conflicts with Rust keyword: use `Self_` internally, accept `Self` and `s` in parsing
- `Outside` positional on `Line` scope: means "everything except this line" — potentially large result, must handle efficiently
- `Until` positional requires a cursor position: error clearly if no cursor context is provided in CLI mode
- `Next`/`Previous` with no cursor position: error clearly
- Function with no parameters: `cifp` on a `fn foo()` should select the empty range between the parens
- Variable with no value: `civv` on a `let x: i32;` should error ("variable has no value")
- Multi-line parameter lists: the resolver must handle parameters spanning multiple lines
- Nested functions/closures: the resolver should find the innermost function containing the cursor
- Empty struct: `cisv` on `struct Foo {}` should select the empty range between the braces
- Enum variant without data: `cimv` on `None` in `enum Option { None, Some(T) }` should error ("member has no value")
- Enum variant with tuple data vs struct data: `cimv` must handle both `Some(T)` and `Variant { field: T }` forms
- Member resolution ambiguity: if the same field name appears in multiple structs, the resolver must use the containing struct (from cursor position or explicit parent arg) to disambiguate
- Struct with no members: the resolver should return an empty children list, and member-scoped chords should error clearly
- Unicode in identifiers: ensure `TextRange` works with byte offsets or character offsets consistently (prefer line+col, matching LSP convention)
- Empty buffers: all stages should handle gracefully
- Chord with no matching symbol in buffer: resolver returns descriptive error listing available symbols
- Buffer with syntax errors: LSP may return partial/no symbols — resolver should fall back to text-based matching where possible for Line/Buffer scopes

## Test Considerations:
- **Parser unit tests**:
  - All valid short forms parse correctly (exhaustive test of all 4×8×6×7 = 1344 combinations for validity, spot-check representative set for correctness)
  - All valid long forms parse correctly (same set, long form)
  - Invalid short forms produce helpful errors with suggestions
  - Argument parsing: key-value pairs, quoted values, missing values, extra commas
  - Scope-component compatibility enforcement
- **Resolver unit tests**:
  - Line scope resolution with explicit line number
  - Line scope resolution with cursor position
  - Buffer scope resolution (entire file)
  - Function scope resolution with mock LSP data (provide a fake `LspEngine` or trait-based mock)
  - Variable scope resolution with mock LSP data
  - Struct scope resolution with mock LSP data (find struct by name, find struct by cursor position)
  - Member scope resolution with mock LSP data: resolve a struct field by name, resolve an enum variant by name, resolve a member by cursor position within a struct body
  - Member scope edge cases: member not found within enclosing type, cursor between members, enum variant with and without associated data
  - Component extraction: Beginning, End, Name, Parameters, Value for each scope
  - Positional application: Inside, Until, After, Before, Entire, Next, Previous, Outside
  - Cursor context resolution: cursor on a function name resolves to that function
  - Multi-buffer resolution: two buffers provided, chord applies to each independently
  - Error cases: symbol not found, LSP not ready, ambiguous target
- **Patcher unit tests**:
  - Change action produces correct diff
  - Delete action produces correct diff
  - Yank action produces no diff but captures content
  - Append/Prepend produce correct insertion diffs
  - Cursor destination is correctly computed for each action
  - Mode transitions are correctly set
  - Highlight ranges match changed regions
  - Warnings emitted for destructive operations
- **Integration tests**:
  - Full pipeline: chord string → ChordAction, verified against expected diff output
  - Round-trip: apply the generated diff to the original buffer, verify the result matches expectations
  - Test with real Rust source files as buffer content
  - Test the CLI's handling of ChordAction (apply diff, print to stdout)

## Codebase Integration:
- Follow established conventions, best practices, testing, and architecture patterns from the project's aspec
- ChordEngine lives in Layer 1 (`src/commands/chord_engine/`)
- Updated chord type enums stay in Layer 0 (`src/data/chord_types.rs`)
- ChordEngine depends on Layer 0 types and Layer 1 `LspEngine` — never imports from Layer 2
- Frontend traits in Layer 2 are updated to consume `ChordAction` instead of `ParsedChord`
- Use `anyhow::Result` for all public methods, `ChordError` enum for structured error variants
- Use `similar` crate (already in Cargo.toml) for diff generation
- Tests must not require a running LSP server — use mock/fake LSP data
