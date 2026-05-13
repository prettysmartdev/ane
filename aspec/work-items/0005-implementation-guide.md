# Jump Action Implementation Guide (Work Item 0005)

**Status**: Implementation Phase  
**Layers**: 0 (data), 1 (commands), 2 (frontend)  
**Modules**: `src/data/chord_types.rs`, `src/commands/chord.rs`, `src/commands/chord_engine/`, `src/frontend/`  
**Related Spec**: `aspec/work-items/0005-jump-action.md`

---

## Executive Summary

This guide provides a step-by-step implementation roadmap for **Work Item 0005**: adding Jump action, To positional, and Delimiter scope to the chord grammar. Jump moves the cursor to code landmarks without modifying text and is only valid in TUI mode. To positional works with all actions using inclusive-endpoint semantics. Delimiter scope finds innermost delimiter pairs using text-based scanning (no LSP required).

### Scope

- Add `Action::Jump` (short: `j`, long: `Jump`)
- Add `Positional::To` (short: `t`, long: `To`)
- Add `Scope::Delimiter` (short: `d`, long: `Delimiter`)
- Implement delimiter-pair detection algorithm
- Add `FrontendCapabilities` trait for interactivity checking
- Update TUI cursor and scroll handling for Jump chords
- Reject Jump chords on CLI with clear error message

### Success Criteria

- [ ] All new enum variants compile and parse correctly
- [ ] Jump chords parse and resolve without buffer I/O on CLI (early validation)
- [ ] CLI Jump chords fail with "interactive frontend" error message
- [ ] TUI Jump chords move cursor and scroll viewport correctly
- [ ] Delimiter scope resolves innermost delimiter pair
- [ ] To positional produces inclusive endpoint ranges
- [ ] All unit tests pass (enum, combination, parsing, resolution)
- [ ] Integration tests verify TUI cursor updates
- [ ] Manual test checklist passes

---

## Part 1: Layer 0 — Data and Chord Types

### 1.1: Update `src/data/chord_types.rs`

#### 1.1.1: Add `Action::Jump`

Add the new variant to the `Action` enum (keep variants in the order shown):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Change,
    Delete,
    Replace,
    Insert,
    Yank,
    Swap,
    Move,
    Jump,  // NEW: cursor navigation, no text modification
}
```

**Update Display implementation:**

```rust
impl Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // ... existing cases ...
            Self::Jump => write!(f, "Jump"),
        }
    }
}
```

**Update `short()` method:**

```rust
pub fn short(&self) -> &'static str {
    match self {
        // ... existing cases ...
        Self::Jump => "j",
    }
}
```

**Update `from_short()` method:**

```rust
pub fn from_short(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "j" => Some(Self::Jump),
        _ => None,
    }
}
```

**Update `from()` method for long-form parsing:**

```rust
pub fn from(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "Jump" => Some(Self::Jump),
        _ => None,
    }
}
```

#### 1.1.2: Add `requires_interactive()` method

Add this new method to the `Action` impl block:

```rust
impl Action {
    pub fn requires_interactive(&self) -> bool {
        matches!(self, Self::Jump)
    }
    // ... other methods ...
}
```

**Rationale**: This is a pure data-layer fact (no imports from higher layers). The method returns `true` only for Jump, allowing higher layers to check before committing to chord execution.

#### 1.1.3: Add `Positional::To`

Add the new variant to the `Positional` enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Positional {
    Inside,
    Entire,
    Before,
    After,
    Until,
    To,     // NEW: inclusive endpoint positional
    Next,
    Previous,
}
```

**Update Display:**

```rust
impl Display for Positional {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // ... existing cases ...
            Self::To => write!(f, "To"),
        }
    }
}
```

**Update `short()`:**

```rust
pub fn short(&self) -> &'static str {
    match self {
        // ... existing cases ...
        Self::To => "t",
    }
}
```

**Update `from_short()`:**

```rust
pub fn from_short(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "t" => Some(Self::To),
        _ => None,
    }
}
```

**Update `from()`:**

```rust
pub fn from(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "To" => Some(Self::To),
        _ => None,
    }
}
```

#### 1.1.4: Add `Scope::Delimiter`

Add the new variant to the `Scope` enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Line,
    File,
    Function,
    Variable,
    Block,
    Struct,
    Impl,
    Enum,
    Member,
    Delimiter,  // NEW: text-based delimiter pair detection
}
```

**Update Display:**

```rust
impl Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // ... existing cases ...
            Self::Delimiter => write!(f, "Delimiter"),
        }
    }
}
```

**Update `short()`:**

```rust
pub fn short(&self) -> &'static str {
    match self {
        // ... existing cases ...
        Self::Delimiter => "d",
    }
}
```

**Update `from_short()`:**

```rust
pub fn from_short(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "d" => Some(Self::Delimiter),
        _ => None,
    }
}
```

**Update `from()`:**

```rust
pub fn from(s: &str) -> Option<Self> {
    match s {
        // ... existing cases ...
        "Delimiter" => Some(Self::Delimiter),
        _ => None,
    }
}
```

**Update `requires_lsp()` method:**

```rust
pub fn requires_lsp(&self) -> bool {
    !matches!(self, Self::Line | Self::File | Self::Delimiter)
}
```

**Rationale**: Delimiter scope uses pure text scanning; it doesn't require LSP startup like Function, Variable, Block, Struct, Impl, Enum, or Member scopes.

#### 1.1.5: Add `is_valid_jump_combination()` function

Add this new validation function to `chord_types.rs`:

```rust
/// Validates positional + component combinations for Jump action.
/// Jump does not modify text, so components describing text content are invalid.
pub fn is_valid_jump_combination(positional: Positional, component: Component) -> bool {
    match positional {
        Positional::Outside => {
            // Jump Outside requires an explicit direction (Beginning or End)
            matches!(component, Component::Beginning | Component::End)
        }
        _ => {
            // All other positionals reject Value, Parameters, Arguments
            !matches!(
                component,
                Component::Value | Component::Parameters | Component::Arguments
            )
        }
    }
}
```

**Why this function exists**: Jump is fundamentally different from other actions — it moves the cursor without modifying the buffer. This separate validation function prevents polluting `is_valid_combination` with Jump-specific logic and makes the constraint explicit.

#### 1.1.6: Update `is_valid_combination()` for Delimiter

Update the existing `is_valid_combination(scope, component)` function to add Delimiter scope rules:

Find the match expression that dispatches on `scope` and add this arm:

```rust
Scope::Delimiter => {
    // Delimiter supports: Beginning, Contents, End, Self_, Name
    // Does NOT support: Value, Parameters, Arguments
    matches!(
        component,
        Component::Beginning
            | Component::Contents
            | Component::End
            | Component::Self_
            | Component::Name
    )
}
```

**Placement**: Add this case in the appropriate alphabetical or logical position within the match expression.

#### 1.1.7: Update `each_position_has_unique_short_letters` test

This test in `src/data/chord_types.rs` ensures short letters are unique. Update it to include the new variants:

```rust
#[test]
fn each_position_has_unique_short_letters() {
    let actions = [
        Action::Change,
        Action::Delete,
        Action::Replace,
        Action::Insert,
        Action::Yank,
        Action::Swap,
        Action::Move,
        Action::Jump,  // ADD
    ];
    let positionals = [
        Positional::Inside,
        Positional::Entire,
        Positional::Before,
        Positional::After,
        Positional::Until,
        Positional::To,  // ADD
        Positional::Next,
        Positional::Previous,
    ];
    let scopes = [
        Scope::Line,
        Scope::File,
        Scope::Function,
        Scope::Variable,
        Scope::Block,
        Scope::Struct,
        Scope::Impl,
        Scope::Enum,
        Scope::Member,
        Scope::Delimiter,  // ADD
    ];
    // ... rest of test unchanged
}
```

**Summary of Layer 0 changes**:
- New enum variants: `Action::Jump`, `Positional::To`, `Scope::Delimiter`
- New methods: `Action::requires_interactive()`, `is_valid_jump_combination()`
- Updated methods: `short()`, `from_short()`, `Display` for all three enums
- Updated `requires_lsp()` to include Delimiter
- Updated `is_valid_combination()` with Delimiter rules
- Updated unit test to include new variants

---

## Part 2: Layer 1 — Chord Logic

### 2.1: Add `FrontendCapabilities` trait to `src/commands/chord.rs`

Add this trait at the top-level of the chord module (before `execute_chord`):

```rust
/// FrontendCapabilities allows Layer 1 to query frontend properties without importing Layer 2 types.
/// This trait follows the dependency-inversion principle: lower layers define the interfaces they need,
/// higher layers provide the implementation.
pub trait FrontendCapabilities {
    fn is_interactive(&self) -> bool;
}
```

### 2.2: Add `HeadlessContext` struct

Add this private struct right after the trait definition:

```rust
/// HeadlessContext is used by CLI (exec) callers that don't have a real frontend.
/// It implements FrontendCapabilities by always returning false for is_interactive.
struct HeadlessContext;

impl FrontendCapabilities for HeadlessContext {
    fn is_interactive(&self) -> bool {
        false
    }
}
```

**Rationale**: This avoids duplicating the "non-interactive" check in CLI code. All CLI code paths pass `&HeadlessContext` to `execute_chord`.

### 2.3: Update `execute_chord` signature and early validation

Find the `execute_chord` function in `src/commands/chord.rs` and update its signature:

**Old signature:**
```rust
pub fn execute_chord(path: &Path, chord: &ChordQuery, lsp: &mut LspEngine) -> Result<ChordResult> {
```

**New signature:**
```rust
pub fn execute_chord(
    frontend: &dyn FrontendCapabilities,
    path: &Path,
    chord: &ChordQuery,
    lsp: &mut LspEngine,
) -> Result<ChordResult> {
```

**Add early Jump validation at the start of the function body:**

```rust
pub fn execute_chord(
    frontend: &dyn FrontendCapabilities,
    path: &Path,
    chord: &ChordQuery,
    lsp: &mut LspEngine,
) -> Result<ChordResult> {
    // Early validation: Jump requires interactive frontend
    if chord.action.requires_interactive() && !frontend.is_interactive() {
        bail!("Jump action requires an interactive frontend; use ane in TUI mode");
    }

    // ... rest of function body unchanged
}
```

**Rationale**: This check runs before any file I/O, LSP startup, or resolver work. On CLI, Jump chords fail immediately with a clear error. On TUI, the check succeeds and execution continues.

### 2.4: Update all `execute_chord` call sites

Find all places that call `execute_chord` and add `&HeadlessContext` as the first argument:

**Typical before:**
```rust
execute_chord(&path, &chord, &mut lsp)?
```

**Typical after:**
```rust
execute_chord(&HeadlessContext, &path, &chord, &mut lsp)?
```

**Common locations to update:**
- `src/commands/cli_handler.rs` or `src/main.rs` in the `ane exec` code path
- Any test code that calls `execute_chord` directly
- Any integration tests that simulate CLI execution

Use your editor's find-and-replace (with regex if needed) to ensure all call sites are updated.

### 2.5: Parser additions to `src/commands/chord_engine/parser.rs`

#### 2.5.1: Add short-form parser mappings

Find the function or match expression that maps single-character short forms (e.g., in `parse_action_short` or `from_short`), and ensure these mappings exist or are added:

```rust
// Ensure these mappings exist in the parser:
// "j" → Action::Jump
// "t" → Positional::To
// "d" → Scope::Delimiter
```

**Note**: These should already be handled by the updated `Action::from_short()`, `Positional::from_short()`, and `Scope::from_short()` methods in Layer 0. The parser calls these methods, so if the methods are correct, parsing automatically works.

#### 2.5.2: Add long-form parser mappings

Ensure the long-form parser (which may use `Action::from()`, `Positional::from()`, etc.) correctly maps:

```rust
// "Jump" → Action::Jump
// "To" → Positional::To
// "Delimiter" → Scope::Delimiter
```

Again, these should be handled by the Layer 0 `from()` methods.

#### 2.5.3: Add Jump-specific post-parse validation

After parsing all four parts (action, positional, scope, component), add validation for Jump chords. Find where the parser constructs the final `ChordQuery` and add this check before returning:

```rust
// After parsing action, positional, scope, component:
if action == Action::Jump {
    // Validate positional + component combination
    if !is_valid_jump_combination(positional, component) {
        // Provide helpful error messages
        let msg = match positional {
            Positional::Outside => {
                "Jump with Outside positional requires Beginning or End component to specify direction"
            }
            _ => {
                "Jump does not operate on Value, Parameters, or Arguments components"
            }
        };
        return Err(ChordError::parse(input, msg));
    }

    // Reject value arguments for Jump
    if args.value.is_some() {
        return Err(ChordError::parse(
            input,
            "Jump does not accept a value argument"
        ));
    }
}
```

**Location**: This validation fires after all four parts have been parsed but before the `ChordQuery` is finalized. Place it in the parser where validation checks are grouped.

### 2.6: Resolver additions to `src/commands/chord_engine/resolver.rs`

#### 2.6.1: Add `Positional::To` handling in `apply_positional`

Find the `apply_positional` function and add a new match arm for `Positional::To`:

```rust
Positional::To => {
    let cursor = query.args.cursor_pos.ok_or_else(|| {
        ChordError::resolve(buffer_name, "'To' positional requires a cursor position")
    })?;
    Ok(vec![TextRange {
        start_line: cursor.0,
        start_col: cursor.1,
        end_line: component_range.end_line,
        end_col: component_range.end_col,
    }])
}
```

**Placement**: Add this case in the match expression that handles all positionals. Place it near `Positional::Until` for reference.

**Explanation**: `To` mirrors `Until` but uses the component's **end** position rather than its **start** position, creating an inclusive range from cursor to the target endpoint.

#### 2.6.2: Add `Action::Jump` handling in `resolve_cursor_and_mode`

Find the `resolve_cursor_and_mode` function (if it exists as a separate function, or within `resolve_buffer`) and add:

```rust
Action::Jump => {
    let cursor = CursorPosition {
        line: target_range.start_line,
        col: target_range.start_col,
    };
    (Some(cursor), Some(EditorMode::Edit))
}
```

**Placement**: Add this case in the match expression on `action`. The match dispatches different actions and returns a tuple of `(Option<CursorPosition>, Option<EditorMode>)`.

**Explanation**: Jump always transitions to Edit mode after landing, so the user can immediately start editing. The cursor position is the start of the resolved target range.

#### 2.6.3: Handle `Positional::Outside` + `Action::Jump` range selection

If the resolver uses an `outside_ranges` function that returns two ranges (before and after the scope), add logic to select the appropriate one for Jump:

```rust
// After apply_positional returns ranges:
// For Jump + Outside, select one range based on component:
if matches!(action, Action::Jump) && matches!(positional, Positional::Outside) {
    let ranges = apply_positional(/* ... */)?;
    let selected = match component {
        Component::Beginning => ranges[0].clone(), // before range
        Component::End => ranges[1].clone(),       // after range
        _ => unreachable!("Jump + Outside validation prevents other components")
    };
    target_range = selected;
}
```

**Rationale**: `outside_ranges` returns both the "before" and "after" ranges because other actions (Change, Delete) might need both. Jump only needs one, selected by component.

#### 2.6.4: Add `Scope::Delimiter` handling in `resolve_scope`

Find the `resolve_scope` function and add a new match arm:

```rust
Scope::Delimiter => resolve_delimiter_scope(query, buffer, buffer_name),
```

**Placement**: Add this case in the match on `scope`. Keep scopes roughly alphabetical or grouped by LSP-required vs. non-LSP.

#### 2.6.5: Implement `resolve_delimiter_scope` function

Add this function to `resolver.rs` (typically at module level, after helper functions):

```rust
fn resolve_delimiter_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    buffer_name: &str,
) -> Result<TextRange> {
    let (line, col) = query.args.cursor_pos.ok_or_else(|| {
        ChordError::resolve(buffer_name, "Delimiter scope requires a cursor position")
    })?;
    find_innermost_delimiter(buffer, line, col, buffer_name)
}
```

**Rationale**: This wrapper function handles the common "resolve a scope from buffer + cursor" pattern, delegating the actual algorithm to `find_innermost_delimiter`.

#### 2.6.6: Implement `find_innermost_delimiter` function

This is the core algorithm. Add it to `resolver.rs`:

```rust
fn find_innermost_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    buffer_name: &str,
) -> Result<TextRange> {
    // Supported delimiter pairs
    const PAIRED_DELIMITERS: &[(&str, &str)] = &[
        ("(", ")"),
        ("{", "}"),
        ("[", "]"),
    ];
    const SELF_PAIRED: &[&str] = &["\"", "'", "`"];

    let mut candidates: Vec<(TextRange, usize)> = Vec::new(); // (range, tightness)

    // Scan paired delimiters
    for (open, close) in PAIRED_DELIMITERS {
        if let Some(range) = find_paired_delimiter(buffer, cursor_line, cursor_col, open, close) {
            let tightness = range.end_line * 1000 + range.end_col - range.start_line * 1000 - range.start_col;
            candidates.push((range, tightness));
        }
    }

    // Scan self-paired delimiters
    for delim in SELF_PAIRED {
        if let Some(range) = find_self_paired_delimiter(buffer, cursor_line, cursor_col, delim) {
            let tightness = range.end_line * 1000 + range.end_col - range.start_line * 1000 - range.start_col;
            candidates.push((range, tightness));
        }
    }

    if candidates.is_empty() {
        return Err(ChordError::resolve(
            buffer_name,
            "no enclosing delimiter found at cursor position",
        )
        .into());
    }

    // Select tightest span (smallest range)
    candidates.sort_by_key(|(_, tightness)| *tightness);
    Ok(candidates[0].0.clone())
}
```

**Algorithm explanation**:
1. Collect all candidate delimiter pairs that enclose the cursor
2. Compute "tightness" (span size) for each candidate
3. Sort by tightness (smallest first)
4. Return the tightest pair

#### 2.6.7: Implement `find_paired_delimiter` helper

Add this function to `resolver.rs`:

```rust
fn find_paired_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    open: &str,
    close: &str,
) -> Option<TextRange> {
    let open_char = open.chars().next().unwrap();
    let close_char = close.chars().next().unwrap();
    let cursor_abs = line_col_to_absolute(buffer, cursor_line, cursor_col);

    // Backward scan from cursor: find closest opening delimiter
    let mut depth = 0;
    let mut open_pos = None;

    for (abs_pos, ch) in buffer.iter_reverse_from(cursor_abs) {
        if ch == close_char {
            depth += 1;
        } else if ch == open_char {
            if depth == 0 {
                open_pos = Some(abs_pos);
                break;
            }
            depth -= 1;
        }
    }

    let open_abs = open_pos?;
    let open_line_col = absolute_to_line_col(buffer, open_abs);

    // Forward scan from opening delimiter: find matching closing delimiter
    let mut depth = 0;
    let mut close_pos = None;

    for (abs_pos, ch) in buffer.iter_forward_from(open_abs + open.len()) {
        if ch == open_char {
            depth += 1;
        } else if ch == close_char {
            if depth == 0 {
                close_pos = Some(abs_pos + close.len());
                break;
            }
            depth -= 1;
        }
    }

    let close_abs = close_pos?;
    let close_line_col = absolute_to_line_col(buffer, close_abs);

    // Ensure cursor is inside the pair
    if cursor_abs < open_abs || cursor_abs >= close_abs {
        return None;
    }

    Some(TextRange {
        start_line: open_line_col.0,
        start_col: open_line_col.1,
        end_line: close_line_col.0,
        end_col: close_line_col.1,
    })
}
```

**Note**: This pseudocode assumes `buffer` provides iteration methods like `iter_reverse_from` and `iter_forward_from`. Adapt to your actual Buffer API (likely using lines and character indexing within lines).

#### 2.6.8: Implement `find_self_paired_delimiter` helper

Add this function to `resolver.rs`:

```rust
fn find_self_paired_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    delim: &str,
) -> Option<TextRange> {
    let delim_char = delim.chars().next().unwrap();
    let cursor_abs = line_col_to_absolute(buffer, cursor_line, cursor_col);

    // Count occurrences before cursor (ignoring escaped)
    let mut count_before = 0;
    for (abs_pos, ch) in buffer.iter_from_start() {
        if abs_pos >= cursor_abs {
            break;
        }
        if ch == delim_char && !is_escaped(buffer, abs_pos) {
            count_before += 1;
        }
    }

    // If count is even, cursor is outside this delimiter type
    if count_before % 2 == 0 {
        return None;
    }

    // Cursor is inside. Scan backward to find opening instance
    let mut open_pos = None;
    for (abs_pos, ch) in buffer.iter_reverse_from(cursor_abs) {
        if ch == delim_char && !is_escaped(buffer, abs_pos) {
            open_pos = Some(abs_pos);
            break;
        }
    }

    // Scan forward to find closing instance
    let mut close_pos = None;
    for (abs_pos, ch) in buffer.iter_forward_from(cursor_abs) {
        if ch == delim_char && !is_escaped(buffer, abs_pos) {
            close_pos = Some(abs_pos + delim.len());
            break;
        }
    }

    let open_line_col = absolute_to_line_col(buffer, open_pos?);
    let close_line_col = absolute_to_line_col(buffer, close_pos?);

    Some(TextRange {
        start_line: open_line_col.0,
        start_col: open_line_col.1,
        end_line: close_line_col.0,
        end_col: close_line_col.1,
    })
}
```

#### 2.6.9: Implement `is_escaped` helper

Add this helper:

```rust
fn is_escaped(buffer: &Buffer, pos: usize) -> bool {
    pos > 0 && {
        let prev_char = buffer.char_at(pos - 1);
        prev_char == Some('\\')
    }
}
```

#### 2.6.10: Update `resolve_contents_component` for Delimiter

Find the `resolve_contents_component` function and add a branch for Delimiter scope:

```rust
fn resolve_contents_component(
    scope: Scope,
    scope_range: &TextRange,
) -> TextRange {
    match scope {
        // ... existing scopes ...
        Scope::Delimiter => {
            // Contents: everything between the delimiters (exclusive of delimiters)
            TextRange {
                start_line: scope_range.start_line,
                start_col: scope_range.start_col + 1, // skip opening delimiter
                end_line: scope_range.end_line,
                end_col: scope_range.end_col, // exclusive end (before closing delimiter)
            }
        }
        // ... rest of match ...
    }
}
```

#### 2.6.11: Update `apply_positional` to reject Next/Previous with Delimiter

Find where `apply_positional` dispatches on positional, and add validation:

```rust
// At the start of apply_positional or before any logic:
if matches!(positional, Positional::Next | Positional::Previous)
    && matches!(scope, Scope::Delimiter)
{
    return Err(ChordError::resolve(
        buffer_name,
        "Next/Previous positional is not valid for Delimiter scope"
    )
    .into());
}
```

**Rationale**: Next/Previous navigate symbol lists via LSP. Delimiter scope has no LSP symbol list, so these positionals are meaningless.

### 2.7: Patcher additions to `src/commands/chord_engine/patcher.rs`

#### 2.7.1: Add Jump action handling

Find the `patch` function or the match expression that dispatches on action. Add a Jump arm:

```rust
Action::Jump => ChordAction {
    buffer_name: buffer_name.to_string(),
    diff: None, // Jump produces no diff
    yanked_content: None,
    cursor_destination: resolution.cursor_destination,
    mode_after: resolution.mode_after,
    highlight_ranges: vec![resolution.component_range],
    warnings: vec![],
},
```

**Placement**: Add this case in the match on `action`. Place it near the other action arms.

**Explanation**:
- `diff: None` — Jump doesn't modify the buffer
- `highlight_ranges: vec![resolution.component_range]` — TUI can briefly flash where the cursor landed
- `cursor_destination` and `mode_after` come from the resolver's `resolve_cursor_and_mode` logic

**No changes for Delimiter scope in the patcher**: The patcher operates on resolved ranges and is scope-agnostic. Delimiter scope resolution happens in the resolver; the patcher doesn't need to know about it.

---

## Part 3: Layer 2 — Frontend Implementation

### 3.1: Implement `FrontendCapabilities` in `src/frontend/cli_frontend.rs`

Add this implementation block in the CLI frontend:

```rust
use crate::commands::chord::FrontendCapabilities;

impl FrontendCapabilities for CliFrontend {
    fn is_interactive(&self) -> bool {
        false
    }
}
```

**Placement**: Add this somewhere in the `cli_frontend.rs` file, typically after the main `CliFrontend` impl block.

**Rationale**: CLI is never interactive. This implementation signals to the chord executor that Jump chords should be rejected.

### 3.2: Implement `FrontendCapabilities` in `src/frontend/tui/tui_frontend.rs`

Add this implementation block in the TUI frontend:

```rust
use crate::commands::chord::FrontendCapabilities;

impl FrontendCapabilities for TuiFrontend {
    fn is_interactive(&self) -> bool {
        true
    }
}
```

**Rationale**: TUI is interactive. This implementation signals to the chord executor that Jump chords are allowed.

### 3.3: Update `TuiFrontend::apply` to handle Jump

Find the `apply` method in `TuiFrontend` that applies chord actions to editor state. Add Jump handling:

```rust
impl ApplyChordAction for TuiFrontend {
    fn apply(&self, state: &mut EditorState, action: &ChordAction) -> Result<()> {
        // Handle Jump: update cursor and scroll
        if let Some(dest) = action.cursor_destination.as_ref() {
            let line_count = state
                .current_buffer()
                .map(|b| b.line_count())
                .unwrap_or(1);
            state.cursor_line = dest.line.min(line_count.saturating_sub(1));

            let line_len = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
                .map(|l| l.chars().count())
                .unwrap_or(0);
            state.cursor_col = dest.col.min(line_len);

            // Bring cursor into viewport
            let visible_height = state.visible_height(); // or compute from last-known frame area
            if state.cursor_line < state.scroll_offset {
                state.scroll_offset = state.cursor_line;
            } else if state.cursor_line >= state.scroll_offset + visible_height {
                state.scroll_offset = state.cursor_line.saturating_sub(visible_height.saturating_sub(1));
            }
        }

        // Handle mode transition
        if let Some(mode) = action.mode_after {
            state.mode = mode;
        }

        // Existing diff application logic for non-Jump actions
        if let Some(diff) = action.diff.as_ref() {
            // ... existing diff application code ...
        }

        Ok(())
    }
}
```

**Placement**: This code should integrate into the existing `apply` method. If your current `apply` only handles diffs, add the Jump logic before the diff handling.

**Key details**:
- Jump produces `diff: None`, so the diff application logic is skipped
- Cursor position is clamped to buffer bounds
- Scroll offset is adjusted so the destination line is visible
- Mode is transitioned to Edit (from `action.mode_after`)

**Note on `visible_height`**: This should be derived from the frame area stored after each draw. If `EditorState` doesn't have this, compute it from the current terminal size or the last-known render area.

### 3.4: Update TUI dispatch path

The TUI dispatch path (where chord mode input is processed) calls `ChordEngine::resolve` and `ChordEngine::patch` directly, not `execute_chord`. This is correct — the TUI doesn't need the early Jump validation because it always calls with a `TuiFrontend` that reports `is_interactive() = true`. The validation in `execute_chord` is only for CLI safety.

**No changes needed** in the TUI dispatch path.

---

## Part 4: Testing Strategy

### 4.1: Unit Tests — Layer 0

Add these tests to `src/data/chord_types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_jump_short_form() {
        assert_eq!(Action::Jump.short(), "j");
        assert_eq!(Action::from_short("j"), Some(Action::Jump));
    }

    #[test]
    fn action_jump_long_form() {
        assert_eq!(Action::from("Jump"), Some(Action::Jump));
    }

    #[test]
    fn action_jump_display() {
        assert_eq!(format!("{}", Action::Jump), "Jump");
    }

    #[test]
    fn action_jump_requires_interactive() {
        assert!(Action::Jump.requires_interactive());
        assert!(!Action::Change.requires_interactive());
        assert!(!Action::Delete.requires_interactive());
        // ... test all other actions return false
    }

    #[test]
    fn positional_to_short_form() {
        assert_eq!(Positional::To.short(), "t");
        assert_eq!(Positional::from_short("t"), Some(Positional::To));
    }

    #[test]
    fn positional_to_long_form() {
        assert_eq!(Positional::from("To"), Some(Positional::To));
    }

    #[test]
    fn scope_delimiter_short_form() {
        assert_eq!(Scope::Delimiter.short(), "d");
        assert_eq!(Scope::from_short("d"), Some(Scope::Delimiter));
    }

    #[test]
    fn scope_delimiter_long_form() {
        assert_eq!(Scope::from("Delimiter"), Some(Scope::Delimiter));
    }

    #[test]
    fn scope_delimiter_requires_lsp() {
        assert!(!Scope::Delimiter.requires_lsp());
        assert!(Scope::Function.requires_lsp());
        assert!(Scope::Variable.requires_lsp());
    }

    #[test]
    fn is_valid_jump_combination_outside_valid() {
        assert!(is_valid_jump_combination(
            Positional::Outside,
            Component::Beginning
        ));
        assert!(is_valid_jump_combination(
            Positional::Outside,
            Component::End
        ));
    }

    #[test]
    fn is_valid_jump_combination_outside_invalid() {
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Contents
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Value
        ));
    }

    #[test]
    fn is_valid_jump_combination_rejects_content_components() {
        assert!(!is_valid_jump_combination(
            Positional::Inside,
            Component::Value
        ));
        assert!(!is_valid_jump_combination(
            Positional::Entire,
            Component::Parameters
        ));
        assert!(!is_valid_jump_combination(
            Positional::Before,
            Component::Arguments
        ));
    }

    #[test]
    fn is_valid_jump_combination_accepts_valid_components() {
        assert!(is_valid_jump_combination(Positional::Inside, Component::Contents));
        assert!(is_valid_jump_combination(Positional::Entire, Component::Self_));
        assert!(is_valid_jump_combination(Positional::To, Component::Name));
    }

    #[test]
    fn is_valid_combination_delimiter_scope() {
        assert!(is_valid_combination(Scope::Delimiter, Component::Beginning));
        assert!(is_valid_combination(Scope::Delimiter, Component::Contents));
        assert!(is_valid_combination(Scope::Delimiter, Component::End));
        assert!(is_valid_combination(Scope::Delimiter, Component::Self_));
        assert!(is_valid_combination(Scope::Delimiter, Component::Name));

        assert!(!is_valid_combination(Scope::Delimiter, Component::Value));
        assert!(!is_valid_combination(Scope::Delimiter, Component::Parameters));
        assert!(!is_valid_combination(Scope::Delimiter, Component::Arguments));
    }

    #[test]
    fn each_position_has_unique_short_letters() {
        let actions = [
            Action::Change,
            Action::Delete,
            Action::Replace,
            Action::Insert,
            Action::Yank,
            Action::Swap,
            Action::Move,
            Action::Jump,
        ];
        let positionals = [
            Positional::Inside,
            Positional::Entire,
            Positional::Before,
            Positional::After,
            Positional::Until,
            Positional::To,
            Positional::Next,
            Positional::Previous,
        ];
        let scopes = [
            Scope::Line,
            Scope::File,
            Scope::Function,
            Scope::Variable,
            Scope::Block,
            Scope::Struct,
            Scope::Impl,
            Scope::Enum,
            Scope::Member,
            Scope::Delimiter,
        ];

        // Check uniqueness
        let mut action_shorts = vec![];
        for a in &actions {
            action_shorts.push(a.short());
        }
        assert_eq!(action_shorts.len(), action_shorts.iter().collect::<std::collections::HashSet<_>>().len());

        let mut pos_shorts = vec![];
        for p in &positionals {
            pos_shorts.push(p.short());
        }
        assert_eq!(pos_shorts.len(), pos_shorts.iter().collect::<std::collections::HashSet<_>>().len());

        let mut scope_shorts = vec![];
        for s in &scopes {
            scope_shorts.push(s.short());
        }
        assert_eq!(scope_shorts.len(), scope_shorts.iter().collect::<std::collections::HashSet<_>>().len());
    }
}
```

### 4.2: Unit Tests — Layer 1 (Resolver)

Add tests to `src/commands/chord_engine/resolver.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_to_produces_inclusive_range() {
        // Buffer: "fn foo() { x }" with cursor at (0,11) on 'x'
        // To + Function + Contents should include the closing '}'
        // whereas Until + Function + Contents would stop at start of '}'
        let buffer = Buffer::from_string("fn foo() { x }");
        let query = ChordQuery {
            action: Action::Change,
            positional: Positional::To,
            scope: Scope::Function,
            component: Component::Contents,
            args: ChordArgs {
                cursor_pos: Some((0, 11)),
                ..Default::default()
            },
        };

        // Resolve To positional
        let ranges = apply_positional(&query, &buffer, "test.rs")?;

        // Verify endpoint is inclusive (at the closing '}')
        assert_eq!(ranges[0].end_col, 13); // position after '}'
    }

    #[test]
    fn find_innermost_delimiter_parentheses() {
        let buffer = Buffer::from_string("foo(bar, baz)");
        let range = find_innermost_delimiter(&buffer, 0, 6, "test.rs")?;

        assert_eq!(range.start_col, 3);  // position of '('
        assert_eq!(range.end_col, 13);   // position after ')'
    }

    #[test]
    fn find_innermost_delimiter_braces() {
        let buffer = Buffer::from_string("if x { y } else { z }");
        let range = find_innermost_delimiter(&buffer, 0, 8, "test.rs")?;

        // Cursor on 'y', innermost pair is { y }
        assert_eq!(range.start_col, 6);  // position of '{'
        assert_eq!(range.end_col, 10);   // position after '}'
    }

    #[test]
    fn find_innermost_delimiter_quotes() {
        let buffer = Buffer::from_string("let s = \"hello\";");
        let range = find_innermost_delimiter(&buffer, 0, 11, "test.rs")?;

        // Cursor on 'hello', innermost pair is "hello"
        assert_eq!(range.start_col, 8);   // position of opening '"'
        assert_eq!(range.end_col, 14);    // position after closing '"'
    }

    #[test]
    fn find_innermost_delimiter_nested() {
        let buffer = Buffer::from_string("foo({ bar })");
        let range = find_innermost_delimiter(&buffer, 0, 7, "test.rs")?;

        // Cursor on 'bar', innermost pair is { bar }, not ({ bar })
        assert_eq!(range.start_col, 4);   // position of inner '{'
        assert_eq!(range.end_col, 10);    // position after inner '}'
    }

    #[test]
    fn find_innermost_delimiter_not_found() {
        let buffer = Buffer::from_string("just some text");
        let result = find_innermost_delimiter(&buffer, 0, 5, "test.rs");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no enclosing delimiter"));
    }

    #[test]
    fn find_innermost_delimiter_empty_pair() {
        let buffer = Buffer::from_string("f()");
        let range = find_innermost_delimiter(&buffer, 0, 1, "test.rs")?;

        assert_eq!(range.start_col, 1);  // position of '('
        assert_eq!(range.end_col, 2);    // position after ')'
    }

    #[test]
    fn delimiter_contents_component() {
        // Resolve cids (ChangeInsideDelimiterSelf) with cursor inside {content}
        let buffer = Buffer::from_string("{ content }");
        let query = ChordQuery {
            action: Action::Change,
            positional: Positional::Inside,
            scope: Scope::Delimiter,
            component: Component::Self_,
            args: ChordArgs {
                cursor_pos: Some((0, 5)),
                ..Default::default()
            },
        };

        let range = resolve(&query, &buffer, "test.rs")?;

        // Inside + Self for Delimiter = Contents (everything between delimiters)
        assert_eq!(range.start_col, 2);   // after '{'
        assert_eq!(range.end_col, 10);    // before '}'
    }

    #[test]
    fn jump_resolver_produces_no_diff() {
        let buffer = Buffer::from_string("fn foo() { x }");
        let query = ChordQuery {
            action: Action::Jump,
            positional: Positional::To,
            scope: Scope::Function,
            component: Component::Contents,
            args: ChordArgs {
                cursor_pos: Some((0, 11)),
                ..Default::default()
            },
        };

        let resolution = resolve(&query, &buffer, "test.rs")?;

        assert!(resolution.cursor_destination.is_some());
        // Jump doesn't produce a diff; the patcher handles this
    }

    #[test]
    fn jump_outside_beginning() {
        let buffer = Buffer::from_string("fn foo() { x }");
        let query = ChordQuery {
            action: Action::Jump,
            positional: Positional::Outside,
            scope: Scope::Function,
            component: Component::Beginning,
            args: ChordArgs {
                cursor_pos: Some((0, 11)),
                ..Default::default()
            },
        };

        let resolution = resolve(&query, &buffer, "test.rs")?;

        // Should jump to the line before the function
        let dest = resolution.cursor_destination.unwrap();
        assert!(dest.line < 1); // Before function starts (line 0)
    }
}
```

### 4.3: Parser Tests

Add tests to `src/commands/chord_engine/parser.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jump_short_form() {
        let query = parse("jtfc")?;
        assert_eq!(query.action, Action::Jump);
        assert_eq!(query.positional, Positional::To);
        assert_eq!(query.scope, Scope::Function);
        assert_eq!(query.component, Component::Contents);
    }

    #[test]
    fn parse_jump_long_form() {
        let query = parse("JumpToFunctionContents")?;
        assert_eq!(query.action, Action::Jump);
        assert_eq!(query.positional, Positional::To);
    }

    #[test]
    fn parse_jump_outside_requires_direction() {
        let result = parse("jofv");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Beginning or End"));
    }

    #[test]
    fn parse_jump_rejects_value_argument() {
        let result = parse("jtfc(my_fn)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("value argument"));
    }

    #[test]
    fn parse_delimiter_short_form() {
        let query = parse("cids")?;
        assert_eq!(query.action, Action::Change);
        assert_eq!(query.scope, Scope::Delimiter);
    }

    #[test]
    fn parse_delimiter_long_form() {
        let query = parse("ChangeInsideDelimiterSelf")?;
        assert_eq!(query.scope, Scope::Delimiter);
    }

    #[test]
    fn parse_positional_to_short_form() {
        let query = parse("ctfe")?;
        assert_eq!(query.positional, Positional::To);
    }

    #[test]
    fn parse_jump_auto_submit() {
        // Test that try_auto_submit_short works for Jump chords
        let query = ChordEngine::try_auto_submit_short("jtfc", 0, 5)?;
        assert_eq!(query.action, Action::Jump);
    }
}
```

### 4.4: Frontend Capability Tests

Add tests to `src/frontend/cli_frontend.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::chord::FrontendCapabilities;

    #[test]
    fn cli_frontend_not_interactive() {
        let cli = CliFrontend::new();
        assert!(!cli.is_interactive());
    }
}
```

Add tests to `src/frontend/tui/tui_frontend.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::chord::FrontendCapabilities;

    #[test]
    fn tui_frontend_is_interactive() {
        let tui = TuiFrontend::new();
        assert!(tui.is_interactive());
    }
}
```

### 4.5: Integration Tests

Add to `/workspace/tests/work_item_0005.rs`:

```rust
#[test]
fn execute_chord_rejects_jump_on_cli() {
    use ane::commands::chord;
    use ane::commands::lsp_engine::LspEngine;
    use std::path::Path;

    let nonexistent = Path::new("/does/not/exist");
    let query = ChordQuery {
        action: Action::Jump,
        positional: Positional::To,
        scope: Scope::Function,
        component: Component::Contents,
        args: Default::default(),
    };

    let mut lsp = LspEngine::new(Default::default());
    let result = chord::execute_chord(&HeadlessContext, nonexistent, &query, &mut lsp);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("interactive"));
    // Critically: error fires before file I/O, so no "file not found" error
}
```

### 4.6: Manual Testing Checklist

Before marking the work item complete, manually test these scenarios in TUI mode:

- [ ] Open a multi-function file (e.g., Rust source)
- [ ] In chord mode, type `jnfc` — cursor jumps to start of next function's body
- [ ] Type `jpfn` — cursor jumps back to previous function's name
- [ ] Type `jtfe` — cursor jumps to closing brace of current function
- [ ] Type `jofb` — cursor jumps to line just before function
- [ ] Type `jofe` — cursor jumps to line just after function
- [ ] Type `jofv` — chord box turns red with direction-hint error message
- [ ] Type `ctfe` — change from cursor through function end (text deleted, edit mode entered)
- [ ] Type `ctlb` — change to line beginning (ChangeToLineBeginning)
- [ ] Position cursor inside `{ block }` and type `cids` — text between braces deleted, delimiters remain
- [ ] Position cursor inside `{ block }` and type `ceds` — entire `{ block }` including braces deleted
- [ ] Position cursor inside `"string"` and type `cids` — text between quotes deleted, quotes remain
- [ ] Position cursor in nested `foo({ bar })` and type `didc` — only inner `{ bar }` contents deleted
- [ ] Type `jtdb` inside a block — cursor jumps to the opening brace
- [ ] Run `ane exec file.rs jtfc` in CLI — stderr shows "Jump action requires interactive frontend", exit code 1
- [ ] Jump to a function far off-screen (200+ lines away) — verify `scroll_offset` updates and cursor is visible in viewport
- [ ] Press arrow keys after Jump to confirm edit mode is active

---

## Part 5: Integration Checklist

### Compiler-Driven Integration

Adding three new enum variants will produce non-exhaustive match errors throughout the codebase. Use these as your integration checklist:

- [ ] `Action::Jump` added to `chord_types.rs`
- [ ] `Positional::To` added to `chord_types.rs`
- [ ] `Scope::Delimiter` added to `chord_types.rs`
- [ ] All Display impls updated
- [ ] All `short()` methods updated
- [ ] All `from_short()` methods updated
- [ ] All `from()` methods updated
- [ ] `requires_lsp()` updated to include Delimiter
- [ ] `requires_interactive()` method added
- [ ] `is_valid_jump_combination()` function added
- [ ] `is_valid_combination()` updated for Delimiter
- [ ] Uniqueness test updated
- [ ] `FrontendCapabilities` trait defined
- [ ] `HeadlessContext` struct added
- [ ] `execute_chord` signature updated (add `frontend` parameter)
- [ ] All `execute_chord` call sites updated
- [ ] Parser mappings added for short and long forms
- [ ] Jump post-parse validation added
- [ ] `apply_positional` updated for `To`
- [ ] `resolve_cursor_and_mode` updated for Jump
- [ ] `resolve_scope` updated for Delimiter
- [ ] `resolve_delimiter_scope` function implemented
- [ ] `find_innermost_delimiter` function implemented
- [ ] `find_paired_delimiter` and `find_self_paired_delimiter` helpers implemented
- [ ] `resolve_contents_component` updated for Delimiter
- [ ] `apply_positional` validation for Next/Previous + Delimiter added
- [ ] Patcher updated for Jump action (no diff case)
- [ ] `CliFrontend` implements `FrontendCapabilities`
- [ ] `TuiFrontend` implements `FrontendCapabilities`
- [ ] `TuiFrontend::apply` updated for Jump cursor/scroll handling
- [ ] All unit tests updated
- [ ] All parser tests added
- [ ] All resolver tests added
- [ ] Integration tests added
- [ ] Manual test checklist completed

### Build Verification

After all changes, run:

```bash
cargo build              # Should compile with no errors
cargo test               # All tests should pass
cargo clippy -- -D warnings  # No clippy warnings
cargo fmt --check        # Code is properly formatted
```

---

## Part 6: Common Pitfalls and Tips

### Pitfall 1: Jump validation timing

**Issue**: You add Jump validation inside the resolver instead of before. This causes unnecessary buffer I/O and LSP startup before rejecting the chord.

**Solution**: Add the early check in `execute_chord` before any file operations:
```rust
if chord.action.requires_interactive() && !frontend.is_interactive() {
    bail!("Jump action requires an interactive frontend");
}
```

### Pitfall 2: To positional endpoint confusion

**Issue**: You implement To the same as Until, forgetting that To should include the endpoint.

**Solution**: Remember:
- `Until` ends at `component_range.start` (before the target)
- `To` ends at `component_range.end` (through the target)

### Pitfall 3: Delimiter algorithm complexity

**Issue**: The nested-delimiter-scanning algorithm is tricky. You accidentally count the wrong nesting depth or mix up the direction of forward/backward scans.

**Solution**:
- Test paired-delimiter logic independently from self-paired logic
- Use small buffers in unit tests (e.g., `"(x)"`, `"{ y }"`)
- Print intermediate state during debugging
- Remember: backward scan finds candidates, forward scan finds matches

### Pitfall 4: Missing call-site updates

**Issue**: You update `execute_chord` signature but forget to update one call site, causing a compile error.

**Solution**: Use your editor's find-and-replace to locate all `execute_chord(` calls:
```bash
grep -r "execute_chord(" src/ tests/
```

### Pitfall 5: FrontendCapabilities not imported

**Issue**: You define the trait in `chord.rs` but forget to import it in the frontend files.

**Solution**: Add at the top of CLI and TUI frontend files:
```rust
use crate::commands::chord::FrontendCapabilities;
```

### Pitfall 6: Scroll offset clamping

**Issue**: After Jump, the cursor is off-screen or the scroll offset is negative.

**Solution**: Always clamp before assignment:
```rust
state.scroll_offset = state.cursor_line.min(some_max_offset);
```

---

## Acceptance Criteria

The work item is complete when:

1. **Code compiles**: `cargo build --release` succeeds
2. **Tests pass**: `cargo test` shows all tests passing
3. **Linting passes**: `cargo clippy -- -D warnings` produces no errors
4. **Formatting**: `cargo fmt --check` passes
5. **Manual testing**: All scenarios in the checklist work correctly
6. **Documentation**: This guide is followed without major deviations
7. **Architecture**: All changes respect the 3-layer architecture; no layer dependencies are violated
8. **Error messages**: Jump chords on CLI produce clear, actionable error messages

---

## Success Indicators

Once complete, you will have:

✅ Added Jump action (short: `j`, long: `Jump`)  
✅ Added To positional (short: `t`, long: `To`)  
✅ Added Delimiter scope (short: `d`, long: `Delimiter`)  
✅ Implemented text-based delimiter-pair detection  
✅ Added FrontendCapabilities trait for interactivity checking  
✅ Updated TUI cursor and scroll handling  
✅ Rejected Jump on CLI with clear error  
✅ Full test coverage (unit, parser, resolver, integration)  
✅ Manual testing passed  
✅ Code review ready  

---

## Related Files

- **Specification**: `/workspace/aspec/work-items/0005-jump-action.md`
- **Test template**: `/workspace/tests/work_item_0005.rs`
- **Architecture**: `/workspace/CLAUDE.md`
- **Foundation**: `/workspace/aspec/foundation.md`

