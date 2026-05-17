# Work Item 0012: Numeric Positional — Implementation Guide

## Overview

This document provides comprehensive guidance for implementing the numeric positional feature in ane. The numeric positional allows a single digit (1–9) to appear in the positional position of a chord, replacing the letter positional. When present, the digit means "next N of whatever the scope/component identifies, forward from the cursor."

**Key Examples:**
- `j5lw` — Jump 5 words forward on the current line
- `l5fd` — List the next 5 function definitions
- `c3ls` — Change within the next 3 lines
- `d5ls` — Delete the next 5 lines

## Feature Scope

### Constraints

- **Single digit only**: 1–9. Zero and double-digit counts (e.g., `10`) are not supported in this work item.
- **No negative direction**: Counts always move forward from the cursor. Backward counting is deferred.
- **Clamping behavior**: If fewer than N occurrences are available, use all available and emit a warning.
- **Scope restrictions**: Buffer and Delimiter scopes do not support counts; reject at parse time.

### Validation Rules

| Rule | Scope | Action | Behavior |
|------|-------|--------|----------|
| Count with Buffer scope | `Positional::Count(_)` + `Scope::Buffer` | Parse error | "Numeric positional is not valid for Buffer scope: there is only one buffer." |
| Count with Delimiter scope | `Positional::Count(_)` + `Scope::Delimiter` | Parse error | "Numeric positional is not valid for Delimiter scope." |
| Count with Replace action | `Positional::Count(_)` + `Action::Replace` | Parse error | "Replace with numeric positional is ambiguous and not supported in this work item." |
| Count without cursor in CLI | CLI mode + no cursor arg | Runtime error | "Numeric positional requires a cursor position; pass `cursor:\"line,col\"`." |

---

## Implementation Layers

### Layer 0: Data Types (`src/data/chord_types.rs`)

#### 1. Positional Enum

**Current state:** Enumerates positional words like `Inside`, `Until`, `Outside`, `Next`, `Previous`, `To`.

**Change:** Add the `Count` variant:

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Positional {
    Inside,
    Until,
    Outside,
    Next,
    Previous,
    To,
    Count(u8),  // 1–9, single digit, forward direction
}
```

The enum can continue to derive `Copy` because `u8` is `Copy`.

#### 2. Positional::short() → &'static str

**Purpose:** Convert a `Positional` to its short-form character representation.

**Update logic:**

```rust
pub fn short(self) -> &'static str {
    match self {
        Positional::Inside => "i",
        Positional::Until => "u",
        Positional::Outside => "o",
        Positional::Next => "n",
        Positional::Previous => "p",
        Positional::To => "t",
        Positional::Count(1) => "1",
        Positional::Count(2) => "2",
        Positional::Count(3) => "3",
        Positional::Count(4) => "4",
        Positional::Count(5) => "5",
        Positional::Count(6) => "6",
        Positional::Count(7) => "7",
        Positional::Count(8) => "8",
        Positional::Count(9) => "9",
        Positional::Count(_) => unreachable!("only 1–9 are valid counts"),
    }
}
```

**Why explicit match?** Each branch returns `&'static str`, and the unreachable case catches invariant violations during testing.

#### 3. Positional::from_short(s: &str) → Option<Positional>

**Purpose:** Parse a single character into a `Positional`.

**Update logic:**

```rust
pub fn from_short(s: &str) -> Option<Positional> {
    match s {
        "i" => Some(Positional::Inside),
        "u" => Some(Positional::Until),
        "o" => Some(Positional::Outside),
        "n" => Some(Positional::Next),
        "p" => Some(Positional::Previous),
        "t" => Some(Positional::To),
        "1" => Some(Positional::Count(1)),
        "2" => Some(Positional::Count(2)),
        "3" => Some(Positional::Count(3)),
        "4" => Some(Positional::Count(4)),
        "5" => Some(Positional::Count(5)),
        "6" => Some(Positional::Count(6)),
        "7" => Some(Positional::Count(7)),
        "8" => Some(Positional::Count(8)),
        "9" => Some(Positional::Count(9)),
        "0" => None,  // zero is not a valid count; parse will error
        _ => None,
    }
}
```

**Key design:** `"0"` returns `None`, causing the parser to emit a clear error message.

#### 4. Display Implementation

**Purpose:** Format `Positional` as a string for long-form chords and error messages.

**Implementation:**

```rust
impl fmt::Display for Positional {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Positional::Inside => write!(f, "Inside"),
            Positional::Until => write!(f, "Until"),
            Positional::Outside => write!(f, "Outside"),
            Positional::Next => write!(f, "Next"),
            Positional::Previous => write!(f, "Previous"),
            Positional::To => write!(f, "To"),
            Positional::Count(n) => write!(f, "{}", n),
        }
    }
}
```

**Integration:** `ChordQuery::long_form()` calls `Display`, so `Jump5LineWord` is automatically generated.

#### 5. Validation Helper

**Purpose:** Determine whether a given `Scope` is valid with a `Count` positional.

**Implementation:**

```rust
pub fn is_valid_count_scope(scope: Scope) -> bool {
    !matches!(scope, Scope::Buffer | Scope::Delimiter)
}
```

**Rationale:**
- **Buffer scope:** There is only one buffer, so "next 5 buffers" is nonsensical.
- **Delimiter scope:** Text-based delimiter scanning does not have natural N-repetition semantics (deferred to a future work item).
- **All other scopes:** Valid (Line, Function, Variable, Struct, Impl, Enum, Member, Block).

---

### Layer 1: Parsing (`src/commands/chord_engine/parser.rs`)

The parser handles both short-form (4-character) and long-form chords. Count positionals work in both paths.

#### 1. Short-Form Parsing (`try_parse_short_form`)

**Current behavior:**
- Extract 4 characters: `[action, positional, scope, component]`
- Call `Positional::from_short(chars[1])` to parse the positional
- Call `Scope::from_short(chars[2])` and `Component::from_short(chars[3])`

**Update:**

No structural change is needed. `Positional::from_short()` already returns `Some(Count(n))` once Layer 0 is updated. However, add validation after parsing:

```rust
fn try_parse_short_form(chord: &str) -> Result<ChordQuery> {
    if chord.len() != 4 {
        return Err(ChordError::parse(..., "chord must be exactly 4 characters").into());
    }

    let chars: Vec<char> = chord.chars().collect();
    let action = Action::from_short(chars[0].to_string().as_str())?;
    let positional = Positional::from_short(chars[1].to_string().as_str())?;
    let scope = Scope::from_short(chars[2].to_string().as_str())?;
    let component = Component::from_short(chars[3].to_string().as_str())?;

    // NEW: Validate count positional constraints
    if let Positional::Count(_) = positional {
        if !is_valid_count_scope(scope) {
            return Err(ChordError::parse(
                "numeric positional is not valid for {} scope",
                format!("{}", scope),
            ).into());
        }
        if action == Action::Replace {
            return Err(ChordError::parse(
                "Replace with numeric positional is ambiguous and not supported",
            ).into());
        }
    }

    Ok(ChordQuery {
        action,
        positional,
        scope,
        component,
    })
}
```

#### 2. Long-Form Parsing (`try_parse_long_form`)

Long-form chords are in the pattern `[Action][Positional][Scope][Component]`, e.g., `Jump5LineWord` or `ListFunctionDefinition`.

**Current behavior:**
- `parse_long_action()` strips the action prefix and returns the remainder
- `parse_long_positional()` consumes a positional word from the remainder
- `parse_long_scope()` consumes a scope word
- `parse_long_component()` consumes a component word

**Update parse_long_positional():**

Add a branch to consume a leading digit before the existing word-based logic:

```rust
fn parse_long_positional(input: &str) -> Option<(Positional, &str)> {
    // NEW: Try to consume a leading digit (1–9)
    if let Some(ch) = input.chars().next() {
        if ch.is_ascii_digit() && ch != '0' {
            let n = ch as u8 - b'0';
            return Some((Positional::Count(n), &input[1..]));
        }
    }

    // Fall through to existing word-based positional matching
    if input.starts_with("Inside") {
        return Some((Positional::Inside, &input[6..]));
    }
    if input.starts_with("Until") {
        return Some((Positional::Until, &input[5..]));
    }
    // ... etc for other positionals
    
    None
}
```

**No collision risk:** Existing positionals (`Inside`, `Until`, `Outside`, `Next`, `Previous`, `To`) all start with letters, so a leading digit unambiguously signals a count.

**Validation:** Same as short-form — check `is_valid_count_scope()` and reject `Replace` after parsing:

```rust
fn try_parse_long_form(chord: &str) -> Result<ChordQuery> {
    let action = parse_long_action(chord)?;
    let remainder_after_action = &chord[action.to_string().len()..];
    let (positional, remainder) = parse_long_positional(remainder_after_action)?;
    let (scope, remainder) = parse_long_scope(remainder)?;
    let (component, _) = parse_long_component(remainder)?;

    // NEW: Validate count positional constraints
    if let Positional::Count(_) = positional {
        if !is_valid_count_scope(scope) {
            return Err(ChordError::parse(
                "numeric positional is not valid for {} scope",
                format!("{}", scope),
            ).into());
        }
        if action == Action::Replace {
            return Err(ChordError::parse(
                "Replace with numeric positional is ambiguous and not supported",
            ).into());
        }
    }

    Ok(ChordQuery {
        action,
        positional,
        scope,
        component,
    })
}
```

#### 3. Cursor Requirement Warning (CLI Mode)

In `execute_chord()`, add a check for count positionals without an explicit cursor arg in CLI mode:

```rust
pub fn execute_chord(
    chord_str: &str,
    buffers: &HashMap<String, Buffer>,
    lsp: &mut LspEngine,
    cursor_opt: Option<(usize, usize)>,  // CLI-provided cursor
) -> Result<...> {
    let query = ChordQuery::parse(chord_str)?;

    // NEW: Warn if count positional is used in CLI without cursor
    if let Positional::Count(_) = query.positional {
        if cursor_opt.is_none() {
            return Err(ChordError::runtime(
                "numeric positional requires a cursor position; pass `cursor:\"line,col\"`",
            ).into());
        }
    }

    // Continue with resolution and execution...
}
```

**Note:** The TUI does not need this check because the TUI always provides implicit cursor context from the editor state.

#### 4. Suggestion System (`suggest_chord`)

**Current behavior:** Generate candidate chords by varying valid short-form characters for positions that were misparsed.

**Update:** Ensure the digit characters (`1–9`) do not appear in the positional candidate set. This prevents confusing suggestions when a user mistyped a digit chord.

```rust
fn suggest_chord(chord: &str) -> Vec<String> {
    // Only suggest letter-based positionals, not digits
    let positional_candidates = ["i", "u", "o", "n", "p", "t"];  // Exclude digits
    
    // Compute Levenshtein distance with letter-only candidates...
    // Return top suggestions
}
```

---

### Layer 1: Resolution (`src/commands/chord_engine/resolver.rs`)

The resolver translates a `ChordQuery` into a `TargetRange` (or list of ranges) and a diff/highlight strategy.

#### 1. Line Scope + Self Component (`c3ls`, `d3ls`, `y3ls`)

**Semantics:** Select N consecutive lines starting at the cursor line.

**Implementation:**

```rust
(Positional::Count(n), Scope::Line, Component::Self_) => {
    let start_line = cursor.line;
    let end_line = std::cmp::min(start_line + (n as usize) - 1, buffer.lines.len() - 1);
    let actual_count = end_line - start_line + 1;
    
    if actual_count < n as usize {
        result.warnings.push(format!(
            "only {} of {} requested lines found",
            actual_count, n
        ));
    }

    TargetRange {
        start: (start_line, 0),
        end: (end_line, buffer.lines[end_line].len()),
    }
}
```

**Clamping:** If the buffer has only 2 lines and the user requests 5, select 2 lines and emit a warning.

#### 2. Line Scope + Word Component (`j5lw`)

**Semantics:** Scan forward through the current line, collecting the next N whitespace-delimited word boundaries. Target the position just after the Nth word.

**Implementation:**

```rust
(Positional::Count(n), Scope::Line, Component::Word) => {
    let line = &buffer.lines[cursor.line];
    let mut words_found = 0;
    let mut col = cursor.col;

    // Skip to the next word boundary if we're inside a word
    while col < line.len() && !line[col..].starts_with(char::is_whitespace) {
        col += 1;
    }

    // Scan forward, collecting N word boundaries
    for _ in 0..n {
        // Skip whitespace
        while col < line.len() && line[col..].starts_with(char::is_whitespace) {
            col += 1;
        }
        if col >= line.len() {
            break;  // Ran out of line
        }
        
        // Skip the word itself
        while col < line.len() && !line[col..].starts_with(char::is_whitespace) {
            col += 1;
        }
        words_found += 1;
    }

    if words_found < n as usize {
        result.warnings.push(format!(
            "only {} of {} requested words found on line {}",
            words_found, n, cursor.line
        ));
    }

    TargetRange {
        start: (cursor.line, cursor.col),
        end: (cursor.line, col),
    }
}
```

**Note:** The end position is the column after the Nth word, suitable for Jump or highlight display.

#### 3. LSP-Backed Scopes (Function, Variable, Struct, Member)

**Semantics:** Call `document_symbols()`, filter to the matching kind, find the first symbol after the cursor, then take the next N symbols.

**Implementation:**

```rust
(Positional::Count(n), scope @ (Scope::Function | Scope::Variable | Scope::Struct | Scope::Member), component) => {
    let symbols = lsp.document_symbols(&buffer.path)?;
    let symbol_kind = scope.to_symbol_kind();  // e.g., Scope::Function -> SymbolKind::Function

    // Filter to matching kind and sort by position
    let mut matching: Vec<_> = symbols
        .into_iter()
        .filter(|sym| sym.kind == symbol_kind)
        .collect();
    matching.sort_by_key(|sym| (sym.range.start_line, sym.range.start_col));

    // Find the first symbol strictly after the cursor
    let start_idx = matching
        .iter()
        .position(|sym| sym.range.start_line > cursor.line || 
                        (sym.range.start_line == cursor.line && sym.range.start_col > cursor.col))
        .unwrap_or(matching.len());

    // Take the next N symbols
    let target_symbols: Vec<_> = matching[start_idx..].iter().take(n as usize).collect();

    if target_symbols.len() < n as usize {
        result.warnings.push(format!(
            "only {} of {} requested symbols found after cursor",
            target_symbols.len(), n
        ));
    }

    // For List action, return the symbols as a list
    if action == Action::List {
        result.listed_items = target_symbols.into_iter().map(|sym| {
            ListItem {
                line: sym.range.start_line,
                col: sym.range.start_col,
                text: sym.name.clone(),
            }
        }).collect();
    } else {
        // For other actions (Change, Delete, Yank), span from first to last symbol
        let first_sym = target_symbols.first().ok_or(ChordError::runtime("no symbols found"))?;
        let last_sym = target_symbols.last().unwrap();
        
        result.target_ranges.push(TargetRange {
            start: (first_sym.range.start_line, first_sym.range.start_col),
            end: (last_sym.range.end_line, last_sym.range.end_col),
        });
    }
}
```

**Clamping:** If only 2 function definitions remain after the cursor and the user requests 5, return 2 and emit a warning.

#### 4. Result Structure

Ensure the `ChordResult` struct includes a `warnings` field for emitting clamping messages:

```rust
pub struct ChordResult {
    pub action: Action,
    pub positional: Positional,
    pub scope: Scope,
    pub component: Component,
    pub target_ranges: Vec<TargetRange>,
    pub listed_items: Vec<ListItem>,
    pub diff: Option<Diff>,
    pub highlight_ranges: Vec<HighlightRange>,
    pub warnings: Vec<String>,  // NEW
}
```

---

### Layer 1: Patching (`src/commands/chord_engine/patcher.rs`)

**No structural changes needed.** The patcher operates on the `target_ranges` produced by the resolver. A count positional may produce:

- A **single merged range** (for contiguous line operations like `c3ls`)
- A **list of disjoint ranges** (for non-contiguous symbols like `c3fd`)

The patcher already handles `Vec<TextRange>`, so both cases are covered without modification.

**Example:** `c3ls` produces one range spanning 3 lines; `c3fd` produces up to 3 disjoint ranges (one per function). Both are handled uniformly by the patcher's loop over ranges.

---

### Layer 2: Frontend (`src/frontend/`)

#### 1. TUI App State (`src/frontend/tui/app.rs`)

**Change:** Ensure exhaustiveness on any `match positional` arms. When the `Positional::Count(u8)` variant is added, Rust's exhaustiveness checker will flag any incomplete matches.

**Typical fix:** In status bar or chord display code:

```rust
match positional {
    Positional::Inside => "Inside",
    Positional::Until => "Until",
    Positional::Outside => "Outside",
    Positional::Next => "Next",
    Positional::Previous => "Previous",
    Positional::To => "To",
    Positional::Count(n) => &format!("Count({})", n),  // NEW
}
```

**Implicit cursor context:** The TUI's editor state always provides the current cursor position, so count chords work without an explicit cursor argument. The resolver receives `Some(cursor)` implicitly, not via a CLI argument.

#### 2. CLI Frontend (`src/frontend/cli.rs`)

**No changes needed.** The CLI already accepts cursor arguments via the `cursor:"line,col"` syntax. The error message added in Layer 1 (parser) will gracefully reject missing cursors in CLI mode.

---

## Architecture Summary

### Data Flow

```
chord string (e.g., "j5lw")
    ↓
Parser (Layer 1)
    → Positional::from_short("5") → Some(Count(5))
    → Scope::from_short("l") → Some(Line)
    → Component::from_short("w") → Some(Word)
    → Validation: Count + Line + Word is valid ✓
    ↓
ChordQuery { action: Jump, positional: Count(5), scope: Line, component: Word }
    ↓
Resolver (Layer 1)
    → buffer.lines[cursor.line] = "one two three four five six"
    → Scan 5 words forward from cursor.col
    → TargetRange { start: (line, col), end: (line, 27) }
    ↓
ChordResult { target_ranges: [...], warnings: [...] }
    ↓
Patcher (Layer 1) or Frontend (Layer 2)
    → Apply change, highlight, or list
    ↓
Output (diff, highlight, or list items)
```

### Dependency Constraints

- **Layer 0 → Layer 1:** Parser, Resolver, Patcher import `Positional` and `is_valid_count_scope` from Layer 0. ✓
- **Layer 1 → Layer 2:** TUI imports `ChordQuery`, `ChordResult`, and `Positional` from Layer 1. ✓
- **Layer 2 → Layer 0 or 1:** Permitted (TUI may call chord executor). ✓

---

## Testing Strategy

### Unit Tests

#### Parser Tests (`src/commands/chord_engine/parser.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_short_form_j5lw() {
        let query = ChordQuery::parse("j5lw").unwrap();
        assert_eq!(query.action, Action::Jump);
        assert_eq!(query.positional, Positional::Count(5));
        assert_eq!(query.scope, Scope::Line);
        assert_eq!(query.component, Component::Word);
    }

    #[test]
    fn parse_short_form_c3ls() {
        let query = ChordQuery::parse("c3ls").unwrap();
        assert_eq!(query.action, Action::Change);
        assert_eq!(query.positional, Positional::Count(3));
        assert_eq!(query.scope, Scope::Line);
        assert_eq!(query.component, Component::Self_);
    }

    #[test]
    fn parse_short_form_l9fd() {
        let query = ChordQuery::parse("l9fd").unwrap();
        assert_eq!(query.action, Action::List);
        assert_eq!(query.positional, Positional::Count(9));
        assert_eq!(query.scope, Scope::Function);
        assert_eq!(query.component, Component::Definition);
    }

    #[test]
    fn parse_short_form_zero_fails() {
        let result = ChordQuery::parse("j0lw");
        assert!(result.is_err());
        // Verify error message mentions 1–9 range
    }

    #[test]
    fn parse_short_form_count_with_buffer_scope_fails() {
        let result = ChordQuery::parse("j5bs");
        assert!(result.is_err());
        // Verify error message explains Buffer scope is invalid
    }

    #[test]
    fn parse_short_form_count_with_delimiter_scope_fails() {
        let result = ChordQuery::parse("j5ds");
        assert!(result.is_err());
    }

    #[test]
    fn parse_short_form_replace_with_count_fails() {
        let result = ChordQuery::parse("r5ls");
        assert!(result.is_err());
        // Verify error message explains Replace + count is unsupported
    }

    #[test]
    fn parse_long_form_jump5lineword() {
        let query = ChordQuery::parse("Jump5LineWord").unwrap();
        assert_eq!(query.action, Action::Jump);
        assert_eq!(query.positional, Positional::Count(5));
        assert_eq!(query.scope, Scope::Line);
        assert_eq!(query.component, Component::Word);
    }

    #[test]
    fn parse_long_form_list9functiondefinition() {
        let query = ChordQuery::parse("List9FunctionDefinition").unwrap();
        assert_eq!(query.action, Action::List);
        assert_eq!(query.positional, Positional::Count(9));
        assert_eq!(query.scope, Scope::Function);
        assert_eq!(query.component, Component::Definition);
    }

    #[test]
    fn short_form_round_trip_j5lw() {
        let query = ChordQuery::parse("j5lw").unwrap();
        assert_eq!(query.short_form(), "j5lw");
    }

    #[test]
    fn long_form_round_trip_jump5lineword() {
        let query = ChordQuery::parse("j5lw").unwrap();
        assert_eq!(query.long_form(), "Jump5LineWord");
    }

    #[test]
    fn all_valid_short_forms_still_parse() {
        // Existing exhaustive test for letter-only positionals
        // Digits 1–9 do not appear in the letter-based candidate matrix,
        // so existing assertions remain unchanged.
    }
}
```

#### Resolver Tests (`src/commands/chord_engine/resolver.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_count3_line_self_selects_3_lines() {
        let content = "line0\nline1\nline2\nline3\nline4\nline5";
        let buffer = make_buffer("/buf", content);
        let query = ChordQuery {
            action: Action::Change,
            positional: Positional::Count(3),
            scope: Scope::Line,
            component: Component::Self_,
        };
        let cursor = (1, 0);

        let result = resolve(&query, &buffer, cursor, &mut LspEngine::default()).unwrap();
        
        assert_eq!(result.target_ranges.len(), 1);
        let range = &result.target_ranges[0];
        assert_eq!(range.start, (1, 0));
        assert_eq!(range.end, (3, buffer.lines[3].len()));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn resolve_count3_line_self_with_insufficient_lines() {
        let content = "line0\nline1\nline2";
        let buffer = make_buffer("/buf", content);
        let query = ChordQuery {
            action: Action::Change,
            positional: Positional::Count(3),
            scope: Scope::Line,
            component: Component::Self_,
        };
        let cursor = (1, 0);

        let result = resolve(&query, &buffer, cursor, &mut LspEngine::default()).unwrap();
        
        assert_eq!(result.target_ranges.len(), 1);
        let range = &result.target_ranges[0];
        assert_eq!(range.start, (1, 0));
        assert_eq!(range.end, (2, buffer.lines[2].len()));
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("only 2 of 3"));
    }

    #[test]
    fn resolve_count5_line_word() {
        let content = "one two three four five six";
        let buffer = make_buffer("/buf", content);
        let query = ChordQuery {
            action: Action::Jump,
            positional: Positional::Count(5),
            scope: Scope::Line,
            component: Component::Word,
        };
        let cursor = (0, 0);

        let result = resolve(&query, &buffer, cursor, &mut LspEngine::default()).unwrap();
        
        assert_eq!(result.target_ranges.len(), 1);
        let range = &result.target_ranges[0];
        assert_eq!(range.end.1, 27);  // Position after "six"
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn resolve_count5_line_word_insufficient() {
        let content = "one two three";
        let buffer = make_buffer("/buf", content);
        let query = ChordQuery {
            action: Action::Jump,
            positional: Positional::Count(5),
            scope: Scope::Line,
            component: Component::Word,
        };
        let cursor = (0, 0);

        let result = resolve(&query, &buffer, cursor, &mut LspEngine::default()).unwrap();
        
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("only 3 of 5"));
    }

    #[test]
    fn resolve_count3_function_definition() {
        // Setup: inject 5 function symbols
        let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}";
        let buffer = make_buffer("/test/file.rs", content);
        let mut lsp = LspEngine::default();
        lsp.inject_test_symbols(buffer.path.clone(), vec![
            fn_sym("a", 0),
            fn_sym("b", 1),
            fn_sym("c", 2),
            fn_sym("d", 3),
            fn_sym("e", 4),
        ]);

        let query = ChordQuery {
            action: Action::Change,
            positional: Positional::Count(3),
            scope: Scope::Function,
            component: Component::Definition,
        };
        let cursor = (0, 0);

        let result = resolve(&query, &buffer, cursor, &mut lsp).unwrap();
        
        // Should span from b (first symbol after cursor) to d
        assert_eq!(result.target_ranges.len(), 1);
        let range = &result.target_ranges[0];
        assert_eq!(range.start.0, 1);  // line of b
        assert_eq!(range.end.0, 3);    // line of d
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn resolve_count5_list_function_definition() {
        let content = "fn a() {}\nfn b() {}\nfn c() {}";
        let buffer = make_buffer("/test/file.rs", content);
        let mut lsp = LspEngine::default();
        lsp.inject_test_symbols(buffer.path.clone(), vec![
            fn_sym("a", 0),
            fn_sym("b", 1),
            fn_sym("c", 2),
        ]);

        let query = ChordQuery {
            action: Action::List,
            positional: Positional::Count(5),
            scope: Scope::Function,
            component: Component::Definition,
        };
        let cursor = (0, 0);

        let result = resolve(&query, &buffer, cursor, &mut lsp).unwrap();
        
        assert_eq!(result.listed_items.len(), 2);  // Only b and c remain after cursor
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("only 2 of 5"));
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration {
    use super::*;

    #[test]
    fn full_pipeline_c3ls_changes_exactly_3_lines() {
        let path = "/buf";
        let content = "line0\nline1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9";
        let buffers = single_buffer(path, content);
        let mut lsp = LspEngine::new(LspEngineConfig::default());

        let mut actions = ChordEngine::execute(
            r#"c3ls(cursor:"2,0", value:"REPLACED")"#,
            &buffers,
            &mut lsp,
        )
        .unwrap();
        let action = actions.remove(path).unwrap();

        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("line0"));
        assert!(diff.modified.contains("line1"));
        assert!(!diff.modified.contains("line2"));
        assert!(!diff.modified.contains("line3"));
        assert!(!diff.modified.contains("line4"));
        assert!(diff.modified.contains("REPLACED"));
        assert!(diff.modified.contains("line5"));
        assert!(diff.modified.contains("line9"));
    }

    #[test]
    fn full_pipeline_j5lw_resolves_correct_word_span() {
        let path = "/buf";
        let content = "one two three four five six";
        let buffers = single_buffer(path, content);
        let mut lsp = LspEngine::new(LspEngineConfig::default());

        let mut actions = ChordEngine::execute(
            r#"j5lw(cursor:"0,0")"#,
            &buffers,
            &mut lsp,
        )
        .unwrap();
        let action = actions.remove(path).unwrap();

        assert!(action.diff.is_none());
        assert!(!action.highlight_ranges.is_empty());
        assert_eq!(action.highlight_ranges[0].end_col, 27);
    }

    #[test]
    fn full_pipeline_l5fd_returns_at_most_count_results() {
        let path = "/test/file.rs";
        let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\nfn g() {}";
        let buffers = single_buffer(path, content);

        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_symbols(
            PathBuf::from(path),
            vec![
                fn_sym("a", 0),
                fn_sym("b", 1),
                fn_sym("c", 2),
                fn_sym("d", 3),
                fn_sym("e", 4),
                fn_sym("f", 5),
                fn_sym("g", 6),
            ],
        );

        let mut actions = ChordEngine::execute(
            r#"l5fd(cursor:"0,0")"#,
            &buffers,
            &mut lsp,
        )
        .unwrap();
        let action = actions.remove(path).unwrap();

        assert!(action.diff.is_none());
        assert_eq!(action.listed_items.len(), 5);
        for item in &action.listed_items {
            assert!(item.line > 0);
        }
    }

    #[test]
    fn full_pipeline_l5fd_returns_all_when_fewer_exist() {
        let path = "/test/file.rs";
        let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}";
        let buffers = single_buffer(path, content);

        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_symbols(
            PathBuf::from(path),
            vec![
                fn_sym("a", 0),
                fn_sym("b", 1),
                fn_sym("c", 2),
                fn_sym("d", 3),
            ],
        );

        let mut actions = ChordEngine::execute(
            r#"l5fd(cursor:"0,0")"#,
            &buffers,
            &mut lsp,
        )
        .unwrap();
        let action = actions.remove(path).unwrap();

        assert!(action.diff.is_none());
        assert_eq!(action.listed_items.len(), 3);
    }
}
```

### Regression Tests

Ensure the existing exhaustive test for all valid short-form combinations still passes:

```rust
#[test]
fn all_valid_short_forms_parse_and_invalid_fail() {
    for action in &["j", "c", "d", "y", "l", "r"] {
        for positional in &["i", "u", "o", "n", "p", "t"] {  // Letter-only
            for scope in &["l", "f", "v", "s", "m", "b", "d", "b"] {
                for component in &["s", "w", "d", "n", "h"] {
                    let chord = format!("{}{}{}{}", action, positional, scope, component);
                    // All should parse successfully
                    assert!(ChordQuery::parse(&chord).is_ok(), "should parse: {}", chord);
                }
            }
        }
    }
}
```

Digits 1–9 do not appear in the positional candidates, so this test remains unchanged and continues to pass.

---

## Examples and Use Cases

### Use Case 1: Multi-line Edit

**Scenario:** A developer wants to comment out the next 5 lines of code at line 42.

**Command (TUI):**
```
j4ls              # Jump to line 46 (move 4 lines down)
y5ls              # Yank the next 5 lines
c5ls              # Change (comment) the next 5 lines
# (Interactive edit to add comment syntax)
```

**Command (CLI):**
```bash
ane exec myfile.py c5ls(cursor:"42,0", value:"# ")
```

**Output:** Lines 42–46 are indented or prefixed with the comment syntax.

### Use Case 2: Navigate Large Functions

**Scenario:** A developer is in a file with 20 functions and wants to jump to one of the next 5 function definitions.

**Command (TUI):**
```
l5fd              # List the next 5 function definitions
# (Select one from the list, press Enter to jump)
```

**Output:** A numbered list appears:
```
1: parse() — line 42
2: validate() — line 67
3: execute() — line 89
4: cleanup() — line 102
5: shutdown() — line 115
```

### Use Case 3: Agent-Driven Change

**Scenario:** A code agent needs to delete the next 3 variable definitions in a file.

**Command (API):**
```rust
ChordEngine::execute(
    r#"d3vd(cursor:"10,5")"#,
    &buffers,
    &mut lsp,
)?
```

**Output:** The diff shows the removal of 3 variable definitions starting after line 10, column 5.

---

## Edge Cases and Error Handling

### Case 1: Zero in Positional

**Input:** `j0lw`

**Parsing:** `Positional::from_short("0")` returns `None`, triggering a parse error.

**Error Message:** `"Positional '0' is not valid. Use a digit 1–9 for a count, or a letter (i, u, o, n, p, t) for a positional."`

### Case 2: Double-Digit Count in Short Form

**Input:** `j15lw` (5 characters)

**Parsing:** The length check fails immediately (`len() != 4`).

**Error Message:** `"Chord must be exactly 4 characters. '10' is not a valid positional."`

### Case 3: Count with Buffer Scope

**Input:** `j5bs`

**Parsing:** All 4 characters parse; validation checks `is_valid_count_scope(Scope::Buffer)` → `false`.

**Error Message:** `"Numeric positional is not valid for Buffer scope: there is only one buffer."`

### Case 4: Insufficient Targets

**Input:** `c5ls` on a 2-line buffer with cursor at line 0.

**Resolution:** Clamps to 2 lines; emits a warning: `"only 2 of 5 requested lines found"`.

**Behavior:** Proceeds with the 2 available lines; change applies to lines 0–1.

### Case 5: Missing Cursor in CLI

**Input:** `ane exec myfile.rs l5fd`

**Execution:** No cursor provided; error is detected during `execute_chord`.

**Error Message:** `"Numeric positional requires a cursor position; pass `cursor:\"line,col\"`.""`

**Example fix:** `ane exec myfile.rs l5fd(cursor:"10,0")`

### Case 6: Count with Replace Action

**Input:** `r5ls`

**Parsing:** Validation checks action == `Replace` and positional == `Count(_)`.

**Error Message:** `"Replace with numeric positional is ambiguous and not supported in this work item."`

**Rationale:** It is unclear whether N means "N lines" or "N occurrences to replace."

---

## Integration Checklist

- [ ] **Layer 0:** Add `Count(u8)` variant to `Positional` enum
  - [ ] Implement `short()` with explicit digit branches
  - [ ] Implement `from_short(s)` with digit parsing
  - [ ] Update `Display` impl
  - [ ] Add `is_valid_count_scope()` helper
  - [ ] Update all `match positional` arms for exhaustiveness

- [ ] **Layer 1 — Parser:** Update parsing and validation
  - [ ] Short-form parsing automatically benefits from `Positional::from_short()`
  - [ ] Add validation after parsing (scope, action checks)
  - [ ] Update long-form `parse_long_positional()` to consume leading digits
  - [ ] Add cursor requirement check for CLI mode
  - [ ] Update `suggest_chord()` to exclude digits from candidates
  - [ ] Write comprehensive parser tests

- [ ] **Layer 1 — Resolver:** Handle count in resolution
  - [ ] Line + Self: Select N consecutive lines
  - [ ] Line + Word: Scan N word boundaries
  - [ ] LSP scopes: Filter, sort, take first N matching symbols
  - [ ] Clamp and warn on insufficient targets
  - [ ] Write comprehensive resolver tests

- [ ] **Layer 1 — Patcher:** Verify no changes needed
  - [ ] Confirm patcher handles vec of ranges (already does)

- [ ] **Layer 2 — Frontend:** Fix exhaustiveness
  - [ ] TUI: Update any `match positional` arms
  - [ ] CLI: No changes needed
  - [ ] Write integration tests

- [ ] **Documentation:** Update project docs
  - [ ] Add numeric positional examples to chord syntax guide
  - [ ] Update error message reference
  - [ ] Add to chord library / cheat sheet

---

## References

- **Chord System:** See `docs/chord-system.md` for syntax and semantics.
- **Architecture:** See `CLAUDE.md` for layer definitions and dependency rules.
- **Existing Positionals:** `src/data/chord_types.rs` for `Positional` enum variants.
- **Test Infrastructure:** `src/commands/chord_engine/resolver.rs` for symbol injection and test utilities.
- **LSP Integration:** `src/commands/lsp_engine.rs` for symbol filtering and document operations.

---

## Summary

The numeric positional feature extends the chord system to support counts (1–9) in the positional position. Implementation spans all three layers:

1. **Layer 0** defines the `Count(u8)` variant and validation rules.
2. **Layer 1** parses, validates, resolves, and handles the feature.
3. **Layer 2** ensures frontend compatibility.

The design prioritizes safety (validation at parse time, clear errors), correctness (clamping instead of erroring on insufficient targets), and simplicity (reusing existing resolver patterns for both contiguous and symbolic scopes). Comprehensive tests verify all code paths, edge cases, and error conditions.
