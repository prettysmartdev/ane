# ChordEngine Implementation Guide (Work Item 0002)

**Status**: Design Phase  
**Layer**: 1 (Commands)  
**Module**: `src/commands/chord_engine/`  
**Related**: `aspec/architecture/chord-engine.md`, `src/data/chord_types.rs`, `src/frontend/traits.rs`

## Overview

This document provides the complete implementation roadmap for Work Item 0002: **Implement ChordEngine — composable 3-stage chord execution pipeline**. It synthesizes architectural specifications with detailed implementation guidance, test strategies, and integration points.

The ChordEngine replaces the current monolithic chord system (`src/commands/chord.rs`) with a composable three-stage pipeline that is testable, extensible, and works across interactive (TUI) and non-interactive (CLI) frontends.

---

## Executive Summary

### What We're Building

A stateless, composable chord execution engine with three independent stages:

| Stage | Input | Output | Purpose |
|-------|-------|--------|---------|
| **Parser** | Chord string (short/long form) | `ChordQuery` | Validate syntax, parse arguments, check scope-component compatibility |
| **Resolver** | `ChordQuery` + buffers + LSP | `ResolvedChord` | Compute exact byte ranges, resolve symbols, determine cursor destination |
| **Patcher** | `ResolvedChord` | `HashMap<String, ChordAction>` | Generate diffs, compute highlights, collect warnings |

### Key Design Principles

1. **Separation of Concerns**: Parser doesn't touch buffers. Resolver doesn't generate diffs. Patcher is pure.
2. **Composability**: No individual chord has its own function. Grammar is 4 orthogonal dimensions.
3. **Statelessness**: Engine holds no data between calls. All state comes from inputs.
4. **Frontend Agnostic**: Engine produces structured output; frontends decide how to apply it.
5. **Path Transparency**: Engine has no filesystem access; caller provides canonical absolute paths.

### Success Criteria

- [ ] All 4 chord dimensions parse correctly (exhaustive test coverage)
- [ ] Parser validates scope-component compatibility at parse time
- [ ] Resolver correctly handles all 6 scopes × 7 components independently
- [ ] Patcher generates accurate git-compatible unified diffs
- [ ] Full pipeline works end-to-end for representative chord examples
- [ ] Integration tests verify behavior against real Rust source files
- [ ] CLI and TUI frontends integrate with new `ChordAction` output
- [ ] Error messages include context and helpful suggestions

---

## Part 1: Chord Grammar Reference

### Actions (7 variants)

| Short | Long | Requires Value | Description |
|-------|------|---|-------------|
| `c` | `Change` | Optional | Replace targeted content; if no value, cursor enters Edit mode at start of range |
| `r` | `Replace` | Yes | Find-and-replace within target (requires search/replace args) |
| `d` | `Delete` | No | Remove targeted content |
| `y` | `Yank` | No | Copy targeted content to clipboard (no modification) |
| `a` | `Append` | Yes | Insert content after the target |
| `p` | `Prepend` | Yes | Insert content before the target |
| `i` | `Insert` | Yes | Insert content at a position within the target |

### Positionals (8 variants)

| Short | Long | Description | Example |
|-------|------|-------------|---------|
| `i` | `Inside` | Content between delimiters (exclusive of boundaries) | Inside `(...)`: the content between parens |
| `u` | `Until` | From cursor/start position to boundary (exclusive) | Until function end: from cursor to `}` |
| `a` | `After` | Content following the component boundary | After function: all code after `}` |
| `b` | `Before` | Content preceding the component boundary | Before function: all code before `{` |
| `n` | `Next` | The next occurrence forward from cursor/name | Next variable: scan forward for next var |
| `p` | `Previous` | The previous occurrence backward from cursor | Previous parameter: scan backward |
| `e` | `Entire` | Full extent including delimiters | Entire function: from `def` to final `}` |
| `o` | `Outside` | Everything except the component (inverse of Inside) | Outside parameters: function signature minus params |

### Scopes (6 variants)

| Short | Long | Requires LSP | Description |
|-------|------|---|-------------|
| `l` | `Line` | No | A single line in the buffer (0-indexed) |
| `b` | `Buffer` | No | The entire buffer (file) |
| `f` | `Function` | Yes | A function/method definition |
| `v` | `Variable` | Yes | A variable or constant binding |
| `s` | `Struct` | Yes | A struct, class, or named aggregate type definition |
| `m` | `Member` | Yes | A member within an aggregate: struct field, enum variant |

### Components (7 variants)

| Short | Long | Description | Examples |
|-------|------|-------------|----------|
| `b` | `Beginning` | The start boundary of the scope | Line 5, column 0 |
| `e` | `End` | The end boundary of the scope | Line 5's final character |
| `v` | `Value` | Inner/assigned value: function body, variable RHS, struct/member type | `{ ... }` of function body |
| `p` | `Parameters` | Parameter list of a function signature | The `(x: i32, y: i32)` in `fn foo(...)` |
| `a` | `Arguments` | Argument list at a call site | The `(x, y)` in `foo(x, y)` |
| `n` | `Name` | Identifier/name of function, variable, struct, member | The `getData` in `fn getData(...)` |
| `s` | `Self` | The entire construct (equivalent to the scope itself) | Full function, full variable, full member |

### Scope-Component Validity Matrix

Not all scope-component combinations are valid. The parser enforces this:

|  | Beginning | End | Value | Parameters | Arguments | Name | Self |
|---|---|---|---|---|---|---|---|
| **Line** | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ |
| **Buffer** | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ | ✓ |
| **Function** | ✓ | ✓ | ✓ | ✓ | ✗ | ✓ | ✓ |
| **Variable** | ✓ | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ |
| **Struct** | ✓ | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ |
| **Member** | ✓ | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ |

**Reasoning**:
- **Parameters**, **Arguments**: Only meaningful for function constructs (Function scope at call site for Arguments).
- **Value**: Nonsensical for Line scope (a line's content is its value; use Inside instead).
- **Name**: Not meaningful for Line or Buffer (lines/buffers don't have identifiers).
- **Beginning**, **End**, **Self**: Valid for all scopes.

### Syntax Forms

#### Short Form
Four characters, one from each dimension: `{action}{positional}{scope}{component}`

```
cifp  → ChangeInsideFunctionParameters
dufe  → DeleteUntilFunctionEnd
aale  → AppendAfterLineEnd
yefv  → YankEntireFunctionValue
```

#### Long Form
PascalCase concatenation of full names, in the same order:

```
ChangeInsideFunctionParameters
DeleteUntilFunctionEnd
AppendAfterLineEnd
YankEntireFunctionValue
```

#### Arguments (Optional)
Parenthesized key-value pairs after the chord name:

```
cifp(target:getData, value:"from: int, to: int")
dufe(target:49)
dols
yefv
```

**Argument keys** (recognized by resolver):
- `target`: Identify what to operate on — a symbol name for LSP scopes, or a zero-indexed line number for Line scope
- `parent`: Disambiguate a member when the same name appears in multiple parents (Member scope)
- `cursor`: Cursor position as `"line,col"` (zero-indexed)
- `value`: Replacement text (for Change, Append, Prepend, Insert, Replace actions)
- `find`, `replace`: Find/replace terms (for Replace action)

**Argument values**:
- Unquoted: treated as identifiers (e.g., `target:getData`)
- Quoted with double quotes: treated as strings, escape sequences recognized (e.g., `value:"hello \"world\""`)

#### Cursor Context
In TUI mode, when arguments are omitted, the resolver uses cursor position to determine the target:

```
dols         # No args → resolver uses cursor_pos to find current line
yefv         # No args → resolver uses cursor_pos to find innermost containing function
```

---

## Part 2: Implementation Architecture

### Module Structure

```
src/commands/chord_engine/
├── mod.rs          # ChordEngine struct, public API, re-exports
├── parser.rs       # Stage 1: chord string → ChordQuery
├── resolver.rs     # Stage 2: ChordQuery + buffers + LSP → ResolvedChord
├── patcher.rs      # Stage 3: ResolvedChord → HashMap<String, ChordAction>
├── types.rs        # All struct/enum definitions
└── errors.rs       # ChordError enum and error handling
```

### Data Structures

#### Parser Output: `ChordQuery`

```rust
pub struct ChordQuery {
    pub action: Action,
    pub positional: Positional,
    pub scope: Scope,
    pub component: Component,
    pub args: ChordArgs,
    pub requires_lsp: bool,  // Set by parser based on scope
}

pub struct ChordArgs {
    pub target_name: Option<String>,      // function/variable/struct/member name
    pub target_line: Option<usize>,       // explicit line number
    pub cursor_pos: Option<(usize, usize)>,    // (line, col) from TUI
    pub value: Option<String>,            // replacement/insertion text
    pub search: Option<String>,           // for Replace action
    pub replace: Option<String>,          // for Replace action
}
```

#### Resolver Output: `ResolvedChord`

```rust
pub struct ResolvedChord {
    pub query: ChordQuery,
    pub resolutions: HashMap<String, BufferResolution>,  // buffer_name → resolution
}

pub struct BufferResolution {
    pub buffer_name: String,
    pub target_range: TextRange,          // exact range to operate on
    pub scope_range: TextRange,           // full containing scope
    pub replacement: Option<String>,      // resolved replacement (from args or computed)
    pub cursor_destination: Option<CursorPosition>,  // where cursor should land
    pub mode_after: Option<EditorMode>,   // Edit or Chord mode
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

#### Patcher Output: `ChordAction`

```rust
pub struct ChordAction {
    pub buffer_name: String,
    pub diff: Option<UnifiedDiff>,        // git-compatible diff
    pub yanked_content: Option<String>,   // for Yank action
    pub cursor_destination: Option<CursorPosition>,
    pub mode_after: Option<EditorMode>,
    pub highlight_ranges: Vec<TextRange>,  // ranges to briefly highlight in TUI
    pub warnings: Vec<String>,            // non-fatal warnings
}

pub struct UnifiedDiff {
    pub hunks: Vec<DiffHunk>,
}

pub struct DiffHunk {
    pub old_start: usize,      // line number in original
    pub old_count: usize,
    pub new_start: usize,      // line number in result
    pub new_count: usize,
    pub lines: Vec<DiffLine>,  // context, added, removed
}

pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}
```

#### Error Type: `ChordError`

```rust
pub enum ChordError {
    ParseError {
        input: String,
        position: usize,           // character position of error
        message: String,
        suggestion: Option<String>, // valid chord suggestion
    },
    ResolveError {
        query: ChordQuery,
        buffer_name: String,
        message: String,
        available_symbols: Vec<String>, // nearby valid targets
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

### Engine Public API

```rust
pub struct ChordEngine;

impl ChordEngine {
    /// Full pipeline: parse → resolve → patch.
    /// Returns one ChordAction per buffer.
    pub fn execute(
        chord_input: &str,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
        cursor_pos: Option<(usize, usize)>,  // TUI cursor position
    ) -> Result<HashMap<String, ChordAction>>;

    /// Stage 1: Parse chord string into a query.
    pub fn parse(chord_input: &str) -> Result<ChordQuery>;

    /// Stage 2: Resolve query against buffers and LSP.
    pub fn resolve(
        query: &ChordQuery,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
        cursor_pos: Option<(usize, usize)>,
    ) -> Result<ResolvedChord>;

    /// Stage 3: Generate diff and action from resolved chord.
    pub fn patch(resolved: &ResolvedChord, buffers: &HashMap<String, Buffer>) 
        -> Result<HashMap<String, ChordAction>>;
}
```

---

## Part 3: Implementation Details by Stage

### Stage 1: Parser (`src/commands/chord_engine/parser.rs`)

**Responsibilities**:
1. Detect short form vs. long form input
2. Parse 4 chord dimensions
3. Parse parenthesized key-value arguments
4. Validate scope-component compatibility
5. Generate helpful error messages with suggestions

**Algorithm**:

```
Input: chord_string, e.g., "cifp(target:getData, value:\"int x\")"

1. Strip whitespace
2. Detect parentheses: separate chord from args
   - chord = "cifp", args = "target:getData, value:\"int x\""
3. Detect form:
   - If len(chord) == 4 AND all chars match short form alphabet: SHORT FORM
   - Else: attempt LONG FORM (PascalCase decomposition)
4. Parse short form: map 4 chars to enum variants
   - c → Action::Change
   - i → Positional::Inside
   - f → Scope::Function
   - p → Component::Parameters
5. Parse long form: strip PascalCase prefixes in order
   - Start with chord string "ChangeInsideFunctionParameters"
   - Match and consume "Change" → Action::Change
   - Match and consume "Inside" → Positional::Inside
   - Match and consume "Function" → Scope::Function
   - Match and consume "Parameters" → Component::Parameters
   - If string not empty after 4 matches: error
6. Check scope-component validity in matrix
7. Parse arguments:
   - Split by comma (respect quotes)
   - For each pair: split key=value
   - Store in ChordArgs based on key
   - Parse quoted strings: unescape sequences
8. Set requires_lsp flag based on scope
9. Return ChordQuery or error with position + suggestion
```

**Argument Parsing Details**:

```rust
fn parse_arguments(args_str: &str) -> Result<ChordArgs> {
    let mut args = ChordArgs::default();
    
    // Split by comma, respecting quoted strings
    let pairs = split_respecting_quotes(args_str, ',');
    
    for pair in pairs {
        let (key, value) = parse_key_value(&pair)?;
        
        match key {
            "function" => args.target_name = Some(value),
            "variable" => args.target_name = Some(value),
            "struct" => args.target_name = Some(value),
            "member" => args.target_name = Some(value),
            "line" => args.target_line = Some(value.parse()?),
            "value" => args.value = Some(unescape_string(&value)),
            "search" => args.search = Some(unescape_string(&value)),
            "replace" => args.replace = Some(unescape_string(&value)),
            _ => return Err(ChordError::ParseError {
                input: args_str.to_string(),
                position: 0,
                message: format!("unknown argument key: {}", key),
                suggestion: None,
            }),
        }
    }
    
    Ok(args)
}
```

**Error Messages**:

When parsing fails, include position, message, and suggestion:

```
chord error: invalid component 'Parameters' for scope 'Line'
  input:  cipl
  at:     ^^^^ position 3
  reason: Line scope does not support Parameters component
  help:   Line scope components: Beginning(b), End(e), Self(s)
  suggestion: cifb (ChangeInsideLineBeginning)?
```

**Suggestions**:

For short form, use Levenshtein distance to find similar valid chords:
- `cipx` (x is invalid) → suggest nearest valid chord with distance ≤ 1

For long form, use prefix matching:
- `ChangeInsideFunctionParame` (incomplete) → suggest `ChangeInsideFunctionParameters`

**Test Coverage**:

- Exhaustive short form tests: all 1344 valid combinations parse correctly
- Exhaustive invalid short forms: out-of-range characters fail with suggestions
- Long form parsing: all 1344 combinations in long form parse correctly
- Argument parsing: quoted strings, escaped quotes, missing values, unknown keys
- Whitespace handling: leading/trailing space, spaces around commas
- Edge cases: empty arguments, missing closing paren, trailing garbage

### Stage 2: Resolver (`src/commands/chord_engine/resolver.rs`)

**Responsibilities**:
1. Determine scope: which entity the chord targets
2. Determine component: which part of the scope to operate on
3. Apply positional: compute final target range
4. Compute cursor destination and mode based on action
5. Handle both explicit targets (by name) and implicit (by cursor position)

**Scope Resolution** (6 branches):

#### Line Scope
```rust
fn resolve_line_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
) -> Result<TextRange> {
    let line_num = query.args.target_line
        .or_else(|| cursor_pos.map(|(l, _)| l))
        .ok_or(/* error: no line specified and no cursor */)?;
    
    if line_num >= buffer.line_count() {
        return Err(/* line out of range */);
    }
    
    // Return the entire line
    let start_col = 0;
    let end_col = buffer.line_len(line_num);
    Ok(TextRange { start_line: line_num, start_col, end_line: line_num, end_col })
}
```

#### Buffer Scope
```rust
fn resolve_buffer_scope(buffer: &Buffer) -> TextRange {
    // Entire buffer content
    TextRange {
        start_line: 0,
        start_col: 0,
        end_line: buffer.line_count() - 1,
        end_col: buffer.line_len(buffer.line_count() - 1),
    }
}
```

#### Function Scope
```rust
fn resolve_function_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
    lsp: &mut LspEngine,
    buffer_path: &str,
) -> Result<TextRange> {
    let symbols = lsp.document_symbols(buffer_path)?;
    
    let target_name = &query.args.target_name;
    let target_cursor = cursor_pos;
    
    let function_symbol = if let Some(name) = target_name {
        // Find by name
        find_symbol_by_name(&symbols, name, SymbolKind::Function)
            .ok_or(/* function not found */)?
    } else if let Some((cursor_line, cursor_col)) = target_cursor {
        // Find innermost function containing cursor
        find_innermost_containing(&symbols, cursor_line, cursor_col, SymbolKind::Function)
            .ok_or(/* no function at cursor */)?
    } else {
        return Err(/* need either target_name or cursor_pos */);
    };
    
    Ok(function_symbol.range)
}
```

#### Variable Scope
```rust
fn resolve_variable_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
    lsp: &mut LspEngine,
    buffer_path: &str,
) -> Result<TextRange> {
    let symbols = lsp.document_symbols(buffer_path)?;
    
    let var_symbol = if let Some(name) = &query.args.target_name {
        find_symbol_by_name(&symbols, name, SymbolKind::Variable)
            .ok_or(/* variable not found */)?
    } else if let Some((cursor_line, _)) = cursor_pos {
        find_innermost_containing(&symbols, cursor_line, _, SymbolKind::Variable)
            .ok_or(/* no variable at cursor */)?
    } else {
        return Err(/* need either target_name or cursor_pos */);
    };
    
    Ok(var_symbol.range)
}
```

#### Struct Scope
```rust
fn resolve_struct_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
    lsp: &mut LspEngine,
    buffer_path: &str,
) -> Result<TextRange> {
    let symbols = lsp.document_symbols(buffer_path)?;
    
    let struct_symbol = if let Some(name) = &query.args.target_name {
        find_symbol_by_name(&symbols, name, SymbolKind::Struct)
            .or_else(|| find_symbol_by_name(&symbols, name, SymbolKind::Class))
            .ok_or(/* struct not found */)?
    } else if let Some((cursor_line, _)) = cursor_pos {
        find_innermost_containing(&symbols, cursor_line, _, SymbolKind::Struct)
            .or_else(|| find_innermost_containing(&symbols, cursor_line, _, SymbolKind::Class))
            .ok_or(/* no struct at cursor */)?
    } else {
        return Err(/* need target or cursor */);
    };
    
    // Include entire struct body
    Ok(struct_symbol.range)
}
```

#### Member Scope
```rust
fn resolve_member_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
    lsp: &mut LspEngine,
    buffer_path: &str,
) -> Result<TextRange> {
    let symbols = lsp.document_symbols(buffer_path)?;
    
    // Find containing struct/enum first
    let container = if let Some((cursor_line, _)) = cursor_pos {
        find_innermost_containing(&symbols, cursor_line, _, 
            SymbolKind::Struct | SymbolKind::Enum)?
    } else if let Some(name) = &query.args.target_name {
        // If explicit member name given, search within all types
        // This is ambiguous; we may need to require a parent arg
        return Err(/* ambiguous: member name without parent type */);
    } else {
        return Err(/* need cursor or explicit target */);
    };
    
    let member_symbol = if let Some(name) = &query.args.target_name {
        find_child_by_name(&container.children, name)
            .ok_or(/* member not found in struct */)?
    } else if let Some((cursor_line, _)) = cursor_pos {
        find_innermost_child_at(&container.children, cursor_line, _)
            .ok_or(/* no member at cursor */)?
    } else {
        return Err(/* need member name or cursor */);
    };
    
    Ok(member_symbol.range)
}
```

**Component Resolution** (7 branches):

Within a resolved scope, locate the component:

```rust
fn resolve_component(
    scope_range: TextRange,
    component: Component,
    scope: Scope,
    buffer: &Buffer,
    lsp_symbol: &Symbol,  // for detailed range data
) -> Result<TextRange> {
    match component {
        Component::Beginning => {
            // First character of scope
            Ok(TextRange {
                start_line: scope_range.start_line,
                start_col: scope_range.start_col,
                end_line: scope_range.start_line,
                end_col: scope_range.start_col + 1,
            })
        },
        Component::End => {
            // Last character of scope
            Ok(TextRange {
                start_line: scope_range.end_line,
                start_col: scope_range.end_col.saturating_sub(1),
                end_line: scope_range.end_line,
                end_col: scope_range.end_col,
            })
        },
        Component::Name => {
            // Identifier span (from LSP)
            lsp_symbol.name_range.ok_or(/* no name data from LSP */)
        },
        Component::Parameters => {
            // Function parameter list: from '(' to ')'
            // Extract from LSP or scan buffer
            find_parameters_range(&buffer, &lsp_symbol.range)?
        },
        Component::Arguments => {
            // Argument list at call site: from '(' to ')'
            // Requires searching for call site (not in symbol data typically)
            Err(/* Arguments component requires buffer scan, complex */)
        },
        Component::Value => {
            resolve_value_component(&buffer, scope, scope_range, lsp_symbol)?
        },
        Component::Self_ => {
            // Entire scope
            Ok(scope_range)
        },
    }
}

fn resolve_value_component(
    buffer: &Buffer,
    scope: Scope,
    scope_range: TextRange,
    lsp_symbol: &Symbol,
) -> Result<TextRange> {
    match scope {
        Scope::Buffer => {
            // Entire buffer content = its value
            Ok(scope_range)
        },
        Scope::Function => {
            // Function body: from '{' to '}'
            find_function_body_range(&buffer, &scope_range)?
        },
        Scope::Variable => {
            // RHS of assignment: from '=' to ';' or EOL
            find_variable_value_range(&buffer, &scope_range)?
        },
        Scope::Struct => {
            // Struct body: from '{' to '}'
            find_struct_body_range(&buffer, &scope_range)?
        },
        Scope::Member => {
            // Type annotation or variant data
            find_member_value_range(&buffer, &scope_range)?
        },
        Scope::Line => {
            // A line's value is the line itself
            Ok(scope_range)
        },
    }
}
```

**Positional Application** (8 branches):

After locating the component, apply the positional modifier:

```rust
fn apply_positional(
    component_range: TextRange,
    positional: Positional,
    scope_range: TextRange,
    buffer: &Buffer,
    cursor_pos: Option<(usize, usize)>,
) -> Result<TextRange> {
    match positional {
        Positional::Inside => {
            // Exclusive of delimiters: inside parens, inside braces, etc.
            // Typically [component.start+1, component.end-1]
            Ok(TextRange {
                start_line: component_range.start_line,
                start_col: component_range.start_col + 1,
                end_line: component_range.end_line,
                end_col: component_range.end_col.saturating_sub(1),
            })
        },
        Positional::Until => {
            // From cursor to component boundary (exclusive)
            let (cursor_line, cursor_col) = cursor_pos
                .ok_or(/* Until requires cursor position */)?;
            Ok(TextRange {
                start_line: cursor_line,
                start_col: cursor_col,
                end_line: component_range.end_line,
                end_col: component_range.end_col,
            })
        },
        Positional::After => {
            // After component, within scope
            Ok(TextRange {
                start_line: component_range.end_line,
                start_col: component_range.end_col,
                end_line: scope_range.end_line,
                end_col: scope_range.end_col,
            })
        },
        Positional::Before => {
            // Before component, within scope
            Ok(TextRange {
                start_line: scope_range.start_line,
                start_col: scope_range.start_col,
                end_line: component_range.start_line,
                end_col: component_range.start_col,
            })
        },
        Positional::Entire => {
            // Full component span including delimiters
            Ok(component_range)
        },
        Positional::Next => {
            // Next occurrence forward from cursor or end of component
            let search_from = cursor_pos.unwrap_or((
                component_range.end_line,
                component_range.end_col,
            ));
            find_next_occurrence(&buffer, scope, search_from)?
        },
        Positional::Previous => {
            // Previous occurrence backward from cursor or start of component
            let search_from = cursor_pos.unwrap_or((
                component_range.start_line,
                component_range.start_col,
            ));
            find_previous_occurrence(&buffer, scope, search_from)?
        },
        Positional::Outside => {
            // Everything in scope except component (may be multi-range)
            // For now, treat as union of [scope.start, component.start) and [component.end, scope.end)
            // Patcher must handle multi-range if needed
            Ok(TextRange {
                start_line: scope_range.start_line,
                start_col: scope_range.start_col,
                end_line: component_range.start_line,
                end_col: component_range.start_col, // first range
                // Note: second range would be [component.end, scope.end)
                // This is complex; may require returning Vec<TextRange>
            })
        },
    }
}
```

**Cursor Destination and Mode**:

After resolving the target range, determine where the cursor should move and what mode to enter:

```rust
fn compute_cursor_and_mode(
    action: Action,
    target_range: TextRange,
    replacement: Option<&str>,
) -> (Option<CursorPosition>, Option<EditorMode>) {
    match action {
        Action::Change => {
            // If no replacement given, cursor at start of range, mode=Edit (TUI placeholder)
            if replacement.is_some() {
                (Some(CursorPosition { line: target_range.start_line, col: target_range.start_col }), Some(EditorMode::Chord))
            } else {
                (Some(CursorPosition { line: target_range.start_line, col: target_range.start_col }), Some(EditorMode::Edit))
            }
        },
        Action::Delete => {
            // Cursor at start of deleted range, stay in Chord mode
            (Some(CursorPosition { line: target_range.start_line, col: target_range.start_col }), Some(EditorMode::Chord))
        },
        Action::Append | Action::Prepend | Action::Insert => {
            // Cursor at insertion point, mode=Edit
            (Some(CursorPosition { line: target_range.start_line, col: target_range.start_col }), Some(EditorMode::Edit))
        },
        Action::Yank => {
            // No cursor movement
            (None, None)
        },
        Action::Replace => {
            // Cursor at start of replaced range
            (Some(CursorPosition { line: target_range.start_line, col: target_range.start_col }), Some(EditorMode::Chord))
        },
    }
}
```

**Error Handling**:

- Symbol not found: return list of available symbols
- LSP not ready: return `ChordError::LspRequired`
- Ambiguous target (e.g., multiple functions with same name): list candidates

**Test Coverage**:

- Line scope with explicit line number
- Line scope with cursor position
- Buffer scope resolution (entire file)
- Function scope by name
- Function scope by cursor position
- Variable scope by name and by cursor
- Struct scope by name and by cursor
- Member scope with explicit member and by cursor within struct body
- Component extraction for all valid scope-component combinations
- Positional application for all valid combinations
- Cursor context resolution
- Multi-buffer resolution
- LSP unavailable handling

### Stage 3: Patcher (`src/commands/chord_engine/patcher.rs`)

**Responsibilities**:
1. Extract old content from buffer using target_range
2. Compute new content based on action type
3. Generate git-compatible unified diff
4. Collect highlight ranges (modified regions)
5. Emit warnings for destructive operations

**Diff Generation**:

Use the `similar` crate to generate unified diffs at line granularity:

```rust
fn generate_diff(
    buffer: &Buffer,
    old_range: TextRange,
    new_content: &str,
) -> Result<UnifiedDiff> {
    let old_content = buffer.extract_range(&old_range);
    
    // Split into lines
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();
    
    // Use similar crate for diff
    let diff = similar::text::unified_diff(
        &old_lines,
        &new_lines,
        3,  // context lines
        Some((format!("original"), format!("modified"))),
    );
    
    // Convert to our UnifiedDiff structure
    let mut hunks = Vec::new();
    for hunk in diff.hunks() {
        let mut hunk_lines = Vec::new();
        for change in hunk.iter() {
            match change {
                similar::Change::Equal(line) => hunk_lines.push(DiffLine::Context(line.to_string())),
                similar::Change::Delete(line) => hunk_lines.push(DiffLine::Removed(line.to_string())),
                similar::Change::Insert(line) => hunk_lines.push(DiffLine::Added(line.to_string())),
            }
        }
        hunks.push(DiffHunk {
            old_start: hunk.new_start(),
            old_count: hunk.old_size(),
            new_start: hunk.new_start(),
            new_count: hunk.new_size(),
            lines: hunk_lines,
        });
    }
    
    Ok(UnifiedDiff { hunks })
}
```

**Action-Specific Patch Logic**:

```rust
fn patch_action(
    resolved: &ResolvedChord,
    buffer: &Buffer,
    action: Action,
) -> Result<(String, Vec<TextRange>, Vec<String>)> {  // (new_content, highlights, warnings)
    let old_content = buffer.extract_range(&resolved.target_range);
    let target_range = &resolved.target_range;
    let replacement = &resolved.replacement;
    
    let (mut new_content, mut highlights, mut warnings) = match action {
        Action::Change => {
            let replacement = replacement.as_ref().unwrap_or(&String::new());
            (replacement.clone(), vec![target_range.clone()], vec![])
        },
        Action::Replace => {
            let search = &resolved.search.ok_or(/* Replace requires search arg */)?;
            let replace = &resolved.replace.ok_or(/* Replace requires replace arg */)?;
            let new_content = old_content.replace(search, replace);
            (new_content, vec![target_range.clone()], vec![])
        },
        Action::Delete => {
            if is_entire_buffer(&target_range, buffer) {
                (String::new(), vec![target_range.clone()], 
                    vec!["Deleting entire buffer content".to_string()])
            } else {
                (String::new(), vec![target_range.clone()], vec![])
            }
        },
        Action::Yank => {
            // No new content, yanked_content is set separately
            (old_content.clone(), vec![], vec![])
        },
        Action::Append => {
            let after_content = replacement.as_ref().unwrap_or(&String::new());
            (format!("{}{}", old_content, after_content), vec![target_range.clone()], vec![])
        },
        Action::Prepend => {
            let before_content = replacement.as_ref().unwrap_or(&String::new());
            (format!("{}{}", before_content, old_content), vec![target_range.clone()], vec![])
        },
        Action::Insert => {
            // Insert within the old content at cursor position
            let insert_content = replacement.as_ref().unwrap_or(&String::new());
            // Assumes resolved.insertion_point is set
            (format!("{}{}...{}", &old_content[..insertion_point], insert_content, &old_content[insertion_point..]), 
                vec![target_range.clone()], vec![])
        },
    };
    
    Ok((new_content, highlights, warnings))
}
```

**Warnings**:

Emit warnings for destructive operations:

```rust
fn check_warnings(action: Action, target_range: &TextRange, buffer: &Buffer) -> Vec<String> {
    let mut warnings = Vec::new();
    
    match action {
        Action::Delete => {
            if is_entire_buffer(target_range, buffer) {
                warnings.push("This operation deletes the entire buffer".to_string());
            } else if is_entire_scope(target_range, Scope::Function, buffer) {
                warnings.push("This operation deletes an entire function".to_string());
            }
        },
        Action::Replace if is_entire_buffer(target_range, buffer) => {
            warnings.push("Replace will modify entire buffer content".to_string());
        },
        _ => {},
    }
    
    warnings
}
```

---

## Part 4: Integration Points

### Frontend Traits (Layer 2)

Update `src/frontend/traits.rs` to accept `ChordAction` instead of `ParsedChord`:

```rust
// Old approach (one trait per action)
pub trait ChangeFrontend {
    fn change(&mut self, old: &str, new: &str) -> Result<()>;
}

// New approach (single trait with unified interface)
pub trait ApplyChordAction {
    fn apply(&mut self, action: &ChordAction) -> Result<()>;
}
```

Both CLI and TUI implement:

```rust
impl ApplyChordAction for CliFrontend {
    fn apply(&mut self, action: &ChordAction) -> Result<()> {
        // Apply diff to file on disk
        // Print unified diff to stdout
        // Ignore cursor_destination and mode_after (CLI is non-interactive)
        if let Some(diff) = &action.diff {
            self.apply_diff_to_file(&action.buffer_name, diff)?;
            self.print_diff_to_stdout(diff)?;
        }
        if let Some(yanked) = &action.yanked_content {
            self.copy_to_clipboard(yanked)?;
        }
        Ok(())
    }
}

impl ApplyChordAction for TuiFrontend {
    fn apply(&mut self, action: &ChordAction) -> Result<()> {
        // Apply diff to in-memory buffer
        // Move cursor to cursor_destination
        // Enter mode_after
        // Highlight changed ranges briefly
        if let Some(diff) = &action.diff {
            self.apply_diff_to_buffer(&action.buffer_name, diff)?;
        }
        if let Some(cursor_dest) = &action.cursor_destination {
            self.move_cursor_to(cursor_dest);
        }
        if let Some(mode) = &action.mode_after {
            self.set_mode(*mode);
        }
        self.highlight_ranges(&action.highlight_ranges, Duration::from_millis(200));
        if !action.warnings.is_empty() {
            self.show_warnings(&action.warnings);
        }
        Ok(())
    }
}
```

**Caller Contract for Buffer Paths**:

When the frontend constructs the buffer map to pass to ChordEngine:

```rust
// CLI: resolve relative paths to absolute
let absolute_path = std::fs::canonicalize(path)?;
buffers.insert(absolute_path.to_string_lossy().to_string(), buffer);

// TUI: maintain absolute paths for all open buffers
// Ensure LSPEngine knows the same paths
for (path, buffer) in editor.open_buffers.iter() {
    // path should already be absolute
    buffers.insert(path.clone(), buffer.clone());
}
```

### LSPEngine Integration

The resolver calls `LspEngine` for symbol data:

```rust
// ChordEngine resolver
let symbols = lsp.document_symbols(buffer_path)?;  // buffer_path is an absolute path from caller

// LSPEngine has document identification tied to this path
// ChordEngine passes paths through directly without modification
```

### Data Layer (Layer 0)

Updated chord type enums in `src/data/chord_types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Change,
    Replace,
    Delete,
    Yank,
    Append,
    Prepend,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Positional {
    Inside,
    Until,
    After,
    Before,
    Next,
    Previous,
    Entire,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Line,
    Buffer,
    Function,
    Variable,
    Struct,
    Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Beginning,
    End,
    Value,
    Parameters,
    Arguments,
    Name,
    Self_,
}

// Helper methods
impl Scope {
    pub fn requires_lsp(&self) -> bool {
        matches!(self, Scope::Function | Scope::Variable | Scope::Struct | Scope::Member)
    }
}

impl Component {
    pub fn is_valid_for_scope(&self, scope: Scope) -> bool {
        // Implement scope-component matrix
        match (scope, self) {
            (Scope::Line, Component::Parameters) => false,
            (Scope::Line, Component::Arguments) => false,
            (Scope::Line, Component::Value) => false,
            (Scope::Line, Component::Name) => false,
            // ... etc
            _ => true,
        }
    }
}
```

---

## Part 5: Testing Strategy

### Unit Tests: Parser

```rust
#[cfg(test)]
mod parser_tests {
    use super::*;
    
    #[test]
    fn test_short_form_valid() {
        // Test a representative sample of the 1344 valid combinations
        let cases = vec![
            ("cifb", (Action::Change, Positional::Inside, Scope::Function, Component::Beginning)),
            ("dufe", (Action::Delete, Positional::Until, Scope::Function, Component::End)),
            ("yefv", (Action::Yank, Positional::Entire, Scope::Function, Component::Value)),
            // ... more cases
        ];
        for (input, expected) in cases {
            let query = ChordEngine::parse(input).expect("parse failed");
            assert_eq!((query.action, query.positional, query.scope, query.component), expected);
        }
    }
    
    #[test]
    fn test_invalid_component_for_scope() {
        // cipl = Change Inside Line Parameters (invalid)
        let result = ChordEngine::parse("cipL");
        assert!(result.is_err());
        if let Err(ChordError::ParseError { suggestion, .. }) = result {
            assert!(suggestion.is_some(), "should include suggestion");
        }
    }
    
    #[test]
    fn test_long_form() {
        let query = ChordEngine::parse("ChangeInsideFunctionParameters").unwrap();
        assert_eq!(query.action, Action::Change);
        assert_eq!(query.positional, Positional::Inside);
        assert_eq!(query.scope, Scope::Function);
        assert_eq!(query.component, Component::Parameters);
    }
    
    #[test]
    fn test_arguments_parsing() {
        let query = ChordEngine::parse("cifp(target:getData, value:\"x: int\")").unwrap();
        assert_eq!(query.args.target_name, Some("getData".to_string()));
        assert_eq!(query.args.value, Some("x: int".to_string()));
    }
    
    #[test]
    fn test_argument_escaping() {
        let query = ChordEngine::parse("cifp(value:\"hello \\\"world\\\"\")").unwrap();
        assert_eq!(query.args.value, Some("hello \"world\"".to_string()));
    }
}
```

### Unit Tests: Resolver

```rust
#[cfg(test)]
mod resolver_tests {
    use super::*;
    
    #[test]
    fn test_line_scope_explicit() {
        let mut lsp = MockLspEngine::new();
        let buffer = Buffer::from_str("line 0\nline 1\nline 2\n");
        let query = ChordEngine::parse("dols(target:1)").unwrap();
        
        let resolved = ChordEngine::resolve(&query, &[(buffer_path, &buffer)].into(), &mut lsp, None).unwrap();
        
        let resolution = resolved.resolutions.get(buffer_path).unwrap();
        assert_eq!(resolution.target_range.start_line, 1);
        assert_eq!(resolution.target_range.end_line, 1);
    }
    
    #[test]
    fn test_function_scope_by_cursor() {
        // Chord targets innermost function containing cursor
        let mut lsp = MockLspEngine::new();
        lsp.add_symbol(Symbol {
            name: "getData".to_string(),
            kind: SymbolKind::Function,
            range: TextRange { start_line: 5, start_col: 0, end_line: 15, end_col: 0 },
            ..Default::default()
        });
        
        let buffer = Buffer::from_str("...");
        let query = ChordEngine::parse("yefv").unwrap();
        
        let resolved = ChordEngine::resolve(&query, &buffers, &mut lsp, Some((10, 0))).unwrap();
        
        assert!(resolved.resolutions.values().next().unwrap().scope_range.start_line >= 5);
    }
    
    #[test]
    fn test_struct_scope_member_resolution() {
        let mut lsp = MockLspEngine::new();
        let struct_symbol = Symbol {
            name: "MyStruct".to_string(),
            kind: SymbolKind::Struct,
            range: TextRange { start_line: 0, start_col: 0, end_line: 10, end_col: 0 },
            children: vec![
                Symbol { name: "field1".to_string(), kind: SymbolKind::Field, .. },
                Symbol { name: "field2".to_string(), kind: SymbolKind::Field, .. },
            ],
        };
        lsp.add_symbol(struct_symbol);
        
        let buffer = Buffer::from_str("struct MyStruct { field1: i32, field2: String }");
        let query = ChordEngine::parse("cimn(target:field1)").unwrap();
        
        let resolved = ChordEngine::resolve(&query, &buffers, &mut lsp, None).unwrap();
        // Should find field1 and target its name component
    }
}
```

### Unit Tests: Patcher

```rust
#[cfg(test)]
mod patcher_tests {
    use super::*;
    
    #[test]
    fn test_change_action_diff() {
        let resolved = ResolvedChord {
            query: ChordQuery { action: Action::Change, .. },
            resolutions: vec![(buffer_path, BufferResolution {
                target_range: TextRange { start_line: 0, start_col: 0, end_line: 0, end_col: 5 },
                replacement: Some("world".to_string()),
                ..
            })].into(),
        };
        let buffer = Buffer::from_str("hello");
        
        let actions = ChordEngine::patch(&resolved, &buffers).unwrap();
        let action = actions.get(buffer_path).unwrap();
        
        assert!(action.diff.is_some());
        assert_eq!(action.yanked_content, None);
    }
    
    #[test]
    fn test_yank_action() {
        let resolved = ResolvedChord {
            query: ChordQuery { action: Action::Yank, .. },
            resolutions: vec![(buffer_path, BufferResolution {
                target_range: TextRange { start_line: 0, start_col: 0, end_line: 0, end_col: 5 },
                ..
            })].into(),
        };
        let buffer = Buffer::from_str("hello");
        
        let actions = ChordEngine::patch(&resolved, &buffers).unwrap();
        let action = actions.get(buffer_path).unwrap();
        
        assert!(action.diff.is_none());
        assert_eq!(action.yanked_content, Some("hello".to_string()));
    }
    
    #[test]
    fn test_delete_entire_buffer_warning() {
        let resolved = ResolvedChord {
            query: ChordQuery { action: Action::Delete, scope: Scope::Buffer, .. },
            resolutions: vec![(buffer_path, BufferResolution {
                target_range: /* entire buffer */,
                ..
            })].into(),
        };
        let buffer = Buffer::from_str("content");
        
        let actions = ChordEngine::patch(&resolved, &buffers).unwrap();
        let action = actions.get(buffer_path).unwrap();
        
        assert!(!action.warnings.is_empty());
        assert!(action.warnings[0].contains("entire buffer"));
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    
    #[test]
    fn test_full_pipeline_change_function_parameters() {
        let source = r#"
fn getData(x: i32, y: i32) -> String {
    format!("{} {}", x, y)
}
"#;
        let buffer = Buffer::from_str(source);
        let mut lsp = MockLspEngine::with_rust_source(source);
        
        let chord = "cifp(target:getData, value:\"a: u64, b: u64\")";
        
        let actions = ChordEngine::execute(chord, &buffers, &mut lsp, None).unwrap();
        let action = actions.get(buffer_path).unwrap();
        
        assert!(action.diff.is_some());
        // Verify diff shows change from "(x: i32, y: i32)" to "(a: u64, b: u64)"
        
        // Apply diff and verify result
        let new_buffer = apply_diff(&buffer, &action.diff.unwrap());
        assert!(new_buffer.contains("fn getData(a: u64, b: u64)"));
    }
    
    #[test]
    fn test_full_pipeline_delete_until_function_end() {
        let source = r#"
fn main() {
    println!("start");
    println!("end");
}
"#;
        let buffer = Buffer::from_str(source);
        let mut lsp = MockLspEngine::with_rust_source(source);
        
        // Cursor on line 2 (println!("start"))
        let actions = ChordEngine::execute("dufe", &buffers, &mut lsp, Some((2, 4))).unwrap();
        let action = actions.get(buffer_path).unwrap();
        
        // Should delete from cursor to end of function
        let new_buffer = apply_diff(&buffer, &action.diff.unwrap());
        assert!(new_buffer.contains("println!(\"start\")"));
        assert!(!new_buffer.contains("println!(\"end\")"));
    }
    
    #[test]
    fn test_cross_file_refactor() {
        let buffer1 = Buffer::from_str("fn oldName() { }");
        let buffer2 = Buffer::from_str("let x = oldName();");
        
        // Assuming chord can rename across files (advanced feature)
        let mut buffers: HashMap<String, Buffer> = [
            ("path/to/file1.rs".to_string(), buffer1),
            ("path/to/file2.rs".to_string(), buffer2),
        ].into();
        
        let mut lsp = MockLspEngine::new();
        // ... set up symbols
        
        let actions = ChordEngine::execute("rifs(target:oldName, value:newName)", &buffers, &mut lsp, None).unwrap();
        
        // Should produce actions for both files
        assert_eq!(actions.len(), 2);
    }
}
```

---

## Part 6: Edge Cases and Error Handling

### Parser Edge Cases

1. **Short form ambiguity**: None (each position has unique chars). Exhaustively tested.
2. **Case sensitivity**: `cifb` vs `CIFB` — normalize to lowercase or reject.
3. **Extra whitespace**: `cif b` — reject or strip?
4. **Incomplete long form**: `ChangeInsideFunctionPara` → suggest completion.
5. **Trailing garbage**: `cifb garbage` → error with position.

### Resolver Edge Cases

1. **Function with no parameters**: `cifp` on `fn foo()` selects empty range between parens.
2. **Variable with no value**: `civv` on `let x: i32;` → error ("variable has no assigned value").
3. **Multi-line parameter lists**: Parameters span multiple lines; resolver correctly handles.
4. **Nested functions/closures**: Resolver finds innermost function at cursor.
5. **Empty struct**: `cisv` on `struct Foo {}` selects empty range between braces.
6. **Enum variant without data**: `cimv` on `None` variant (no associated data) → error.
7. **Enum variant with tuple data**: `Some(T)` — can resolve value.
8. **Enum variant with struct data**: `Variant { field: T }` — can resolve value.
9. **Member resolution ambiguity**: If same field name in multiple structs, use containing struct from cursor or error if ambiguous.
10. **Struct with no members**: Returns empty children; member-scoped chords → error ("struct has no members").
11. **Unicode identifiers**: LSP returns byte offsets or character offsets? Ensure consistency.
12. **Empty buffer**: All stages handle gracefully.
13. **Chord with no matching symbol**: Resolver → descriptive error ("Function 'foo' not found; available: bar, baz").
14. **Buffer with syntax errors**: LSP may return partial/no symbols; resolver falls back to text matching for Line/Buffer scopes.
15. **Cursor out of bounds**: Cursor position beyond EOF → error.
16. **Target line out of bounds**: `dols(target:999)` on 50-line file → error.

### Patcher Edge Cases

1. **Deleting entire buffer**: Warn user.
2. **Replace with no matches**: Still generates diff (no-op diff).
3. **Multi-range Outside positional**: May require two separate hunks in diff.
4. **Very large diffs**: Limit hunk context or use streaming?
5. **Binary content in buffer**: Treat as text (may corrupt); could add content-type check.

---

## Part 7: Performance Considerations

1. **Lazy resolution**: When multiple buffers provided, process independently. Line scope chords on files without target line complete quickly.
2. **Symbol caching**: Within a single `execute()` call, cache `document_symbols` results per buffer.
3. **Diff generation**: Use line-level diffs (via `similar` crate); compute only affected hunks.
4. **No buffer cloning**: Work with `&Buffer` references, extract content only when needed.
5. **Next/Previous scanning**: Linear scan forward/backward through buffer; for large scopes, may be slow. Could optimize with index.

---

## Part 8: Migration from Current Implementation

### Current State
- `src/commands/chord.rs`: `parse_chord()`, `execute_chord()`, `ParsedChord`, `ChordResult`
- `src/data/chord_types.rs`: old enum variants

### Migration Steps

1. **Create new module**: `src/commands/chord_engine/` with submodules.
2. **Update `src/data/chord_types.rs`**: Replace enum variants in place.
3. **Write chord_engine code**: Parser, Resolver, Patcher, types, errors.
4. **Add extensive tests**: Parser, resolver, patcher, integration.
5. **Update frontend traits**: Remove old trait methods, add `ApplyChordAction`.
6. **Update CLI frontend**: Consume `ChordAction`, apply diff to disk, print to stdout.
7. **Update TUI frontend**: Consume `ChordAction`, apply diff to buffer, move cursor, change mode.
8. **Remove old code**: Delete `src/commands/chord.rs` and old enum variants.
9. **Update documentation**: README, CLAUDE.md, user guide.

---

## Part 9: Codebase Integration

### Architecture Layer Rules (MUST FOLLOW)

- **ChordEngine** lives in Layer 1 (`src/commands/chord_engine/`)
- **Chord type enums** stay in Layer 0 (`src/data/chord_types.rs`)
- **ChordEngine imports**:
  - Layer 0: `data::chord_types`, `data::buffer`, `data::*` types
  - Layer 1: `commands::lsp_engine`
  - **Never** imports from Layer 2 (frontend)
- **Frontend traits** in Layer 2 import `ChordAction` from Layer 1
- **No circular dependencies**

### Error Handling

- Use `anyhow::Result<T>` for all public methods
- Use `ChordError` enum for structured errors with context
- All errors include buffer names, input strings, suggestions

### Testing

- Tests live in `#[cfg(test)] mod tests` blocks within each source file
- Use `MockLspEngine` (implement `LspEngine` trait) for resolver tests
- No tests require a running LSP server

### Code Style

- Rust edition 2021, stable toolchain
- No comments unless "why" is non-obvious
- Use descriptive names (e.g., `resolve_function_scope` not `resolve_f_scope`)

---

## Appendix A: Example Chords and Expected Behavior

### Example 1: Change Function Parameters

**Chord**: `cifp(target:getData, value:"a: u64, b: u64")`

```
Before:
fn getData(x: i32, y: i32) -> String { ... }

After:
fn getData(a: u64, b: u64) -> String { ... }

Diff:
- fn getData(x: i32, y: i32) -> String {
+ fn getData(a: u64, b: u64) -> String {
```

### Example 2: Delete Until Function End

**Chord**: `dufe` with cursor at (10, 4)

```
Before:
fn main() {
    let x = 1;      // cursor here
    let y = 2;
    println!("{}", x + y);
}

After:
fn main() {
    let x = 1;
}

Diff:
    let x = 1;
-   let y = 2;
-   println!("{}", x + y);
```

### Example 3: Yank Function Body

**Chord**: `yefv(target:getData)`

```
Yanked content:
{
    let result = x + y;
    format!("{}", result)
}
```

No diff (Yank doesn't modify).

### Example 4: Append After Line

**Chord**: `aale(line:5, value:"// TODO: refactor")`

```
Before:
5: println!("done");

After:
5: println!("done");
   // TODO: refactor

Diff:
    println!("done");
+   // TODO: refactor
```

---

## Appendix B: Test Data

### Rust Source File for Integration Tests

```rust
// test_data.rs
pub struct User {
    pub id: u32,
    pub name: String,
    pub email: String,
}

impl User {
    pub fn new(id: u32, name: String, email: String) -> Self {
        User { id, name, email }
    }

    pub fn from_str(s: &str) -> Result<Self, ParseError> {
        let parts: Vec<_> = s.split(',').collect();
        Ok(User {
            id: parts[0].parse()?,
            name: parts[1].to_string(),
            email: parts[2].to_string(),
        })
    }

    pub fn display(&self) -> String {
        format!("{}: {} <{}>", self.id, self.name, self.email)
    }
}

fn main() {
    let user = User::new(1, "Alice".to_string(), "alice@example.com");
    println!("{}", user.display());
}
```

This file exercises:
- Struct definition with multiple fields
- Impl block with multiple methods
- Parameters in different function signatures
- String literals and interpolation
- Control flow (match, if)

---

## Appendix C: Success Checklist

Use this checklist to track implementation progress:

- [ ] Parser module complete and tested
  - [ ] Short form parsing working
  - [ ] Long form parsing working
  - [ ] Argument parsing working
  - [ ] Validation and error messages complete
  - [ ] All tests passing
- [ ] Resolver module complete and tested
  - [ ] Line scope resolution working
  - [ ] Buffer scope resolution working
  - [ ] Function scope resolution working
  - [ ] Variable scope resolution working
  - [ ] Struct scope resolution working
  - [ ] Member scope resolution working
  - [ ] All components resolvable for valid combinations
  - [ ] Positional modifiers applied correctly
  - [ ] All tests passing
- [ ] Patcher module complete and tested
  - [ ] Diff generation working
  - [ ] All action types patching correctly
  - [ ] Warnings emitted appropriately
  - [ ] Highlights computed correctly
  - [ ] All tests passing
- [ ] Integration tests passing
  - [ ] End-to-end pipeline working
  - [ ] Real Rust files tested
  - [ ] Cross-file operations working
- [ ] Frontend integration complete
  - [ ] CLI frontend adapted
  - [ ] TUI frontend adapted
  - [ ] Old chord code removed
- [ ] Documentation complete
  - [ ] CLAUDE.md updated
  - [ ] Architecture docs updated
  - [ ] User guide updated

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-10  
**Author**: Implementation Team  
**Status**: Ready for Implementation
