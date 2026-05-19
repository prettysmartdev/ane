# Work Item 0015: Implementation Guide

**Title**: Address Agent Usage Feedback  
**Issue**: See [0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md) for full specification

---

## Overview

This guide provides step-by-step implementation instructions for fixing six agent-facing bugs in the `ane` chord system. These fixes enable code agents to use buffer-scope chords, append content at the correct line, replace function bodies without delimiter corruption, distinguish between Name and Definition components, and handle multi-line insertions correctly.

All fixes are localized to Layer 1 (`src/commands/`) with documentation updates in Layer 0. The implementation can be completed in the order presented, with each fix being independently testable before moving to the next.

---

## Implementation Order & Dependencies

1. **Fix 1** (Allow buffer-scope chords with empty args) — no dependencies
2. **Fix 2** (Append at correct location) — no dependencies  
3. **Fix 3** (Preserve delimiter lines in function body replacement) — independent
4. **Fix 4** (Document Name vs Definition component) — independent (documentation-only)
5. **Fix 5** (Auto-prefix newline for append at line end) — depends on understanding Fix 2
6. **Integration testing** — validates all fixes working together

Recommended implementation order: **1 → 2 → 3 → 5 → 4 → integration tests**

---

## Fix 1: Allow Buffer-Scope Chords Without Arguments

### Problem Statement

Buffer-scope chords like `yebs` (Yank Entire Buffer Self) require a meaningless `target:1` argument to pass validation, even though a buffer has no named symbol or line number. This forces agents to include dummy parameters.

### Root Cause

`src/commands/chord.rs`, function `execute_chord` has a guard that rejects all empty-args chords except List actions:

```rust
if args_are_empty(&chord.args) && chord.action != Action::List {
    bail!("exec mode requires explicit parameters …");
}
```

### Implementation

**File**: `src/commands/chord.rs`

1. Locate the `execute_chord` function (use `ane exec src/commands/chord.rs --chord "lefd"` to find it)
2. Find the args validation guard
3. Add a second exemption for `Scope::Buffer`:

```rust
if args_are_empty(&chord.args)
    && chord.action != Action::List
    && chord.scope != Scope::Buffer
{
    bail!("exec mode requires explicit parameters …");
}
```

The logic: Buffer scope is whole-file by definition, so no target is meaningful or required.

### Testing

Update existing test `error_message_exec_requires_explicit_params`:
- Find the test (it currently asserts that `yebs` with no args raises an error)
- Change the assertion to expect success instead of error

Add new test `execute_yank_entire_buffer_no_args_succeeds`:
```rust
#[test]
fn execute_yank_entire_buffer_no_args_succeeds() {
    let temp_dir = TempDir::new().unwrap();
    let file = temp_dir.path().join("test.txt");
    std::fs::write(&file, "line 1\nline 2\nline 3\n").unwrap();
    
    let output = execute_chord(
        /* args with empty_args_map */ ,
        &ChordQuery {
            action: Action::Yank,
            positional: Positional::Entire,
            scope: Scope::Buffer,
            component: Component::Self_,
            args: empty_map(), // no target
        },
        &file,
    ).unwrap();
    
    assert_eq!(output, "line 1\nline 2\nline 3\n");
}
```

### Verification

Run `cargo test error_message_exec_requires_explicit_params` and `cargo test execute_yank_entire_buffer_no_args_succeeds` to confirm both pass.

---

## Fix 2: Append After Line Inserts at Correct Position

### Problem Statement

`aals(target:N)` (Append After Line Self) inserts at EOF instead of immediately after line N. Similarly, `pbls(target:N)` (Prepend Before Line Self) inserts at buffer start instead of before line N.

### Root Cause

`src/commands/chord_engine/resolver.rs`, function `apply_positional` for `Positional::After`:

```rust
Positional::After => {
    let (end_line, end_col) = if query.component == Component::Self_ {
        let last = buffer.line_count().saturating_sub(1);
        (last, buffer.lines.get(last).map(|l| line_char_count(l)).unwrap_or(0))
    } else { … };
    Ok(vec![TextRange { start_line: component_range.end_line, start_col: component_range.end_col, end_line, end_col }])
}
```

When `component = Self_`, the resolved range spans from `component_range.end` to buffer end. The patcher then inserts at the range's end (EOF), not at the component's end.

### Implementation

**File**: `src/commands/chord_engine/resolver.rs`, function `apply_positional`

1. Locate `apply_positional` (use `ane exec src/commands/chord_engine/resolver.rs --chord "lefd"`)
2. Find the `Positional::After` match arm
3. Replace the entire `Self_` branch:

```rust
Positional::After => {
    if query.component == Component::Self_ {
        return Ok(vec![TextRange::point(component_range.end_line, component_range.end_col)]);
    }
    // existing else branch for other components
    let (end_line, end_col) = /* existing code for non-Self_ */;
    Ok(vec![TextRange { … }])
}
```

4. Locate the `Positional::Before` match arm in the same function
5. Apply the symmetric fix:

```rust
Positional::Before => {
    if query.component == Component::Self_ {
        return Ok(vec![TextRange::point(component_range.start_line, component_range.start_col)]);
    }
    // existing else branch for other components
    …
}
```

**Rationale**: After Fix 2, `TextRange::point(line, col)` creates a zero-length range. The patcher uses `range.end` as the insertion point for Append, so `TextRange::point(component_range.end_line, end_col)` inserts at the end of the component (line N), which is correct. Prepend uses `range.start`, so `TextRange::point(component_range.start_line, start_col)` inserts at the component start (before line N).

### Testing

Add two unit tests to the `tests` module in `resolver.rs`:

```rust
#[test]
fn append_after_line_self_inserts_at_line_end() {
    let buffer = buf(&["line 1", "line 2", "line 3", "line 4", "line 5"]);
    let query = query(
        Action::Append, Positional::After, Scope::Line, Component::Self_,
        /* target: Some("2") */
    );
    let ranges = apply_positional(&buffer, &query, &component_range_for_line_2).unwrap();
    
    // Should resolve to a point at end of line 2, not at EOF
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].start_line, 1); // 0-indexed
    assert_eq!(ranges[0].end_line, 1);
}

#[test]
fn prepend_before_line_self_inserts_at_line_start() {
    let buffer = buf(&["line 1", "line 2", "line 3"]);
    let query = query(
        Action::Prepend, Positional::Before, Scope::Line, Component::Self_,
        /* target: Some("2") */
    );
    let ranges = apply_positional(&buffer, &query, &component_range_for_line_2).unwrap();
    
    // Should resolve to a point at start of line 2, not at buffer start
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].start_line, 1);
    assert_eq!(ranges[0].start_col, 0);
}
```

### Edge Cases

- **On last line**: `component_range.end_line` is the last line, `end_col` is the last column. Insertion after that position is EOF, which is correct.
- **On empty file**: The `resolve_line_scope` will error if the target line doesn't exist. This existing behavior is correct and unchanged.
- **Interaction with Fix 5**: After Fix 2, `aals(target:N)` resolves to end-of-line-N. Fix 5 will auto-prefix `\n`, making insertion happen on line N+1 ✓.

### Verification

Run `cargo test` to confirm the new tests pass and existing positional tests (`positional_entire_returns_component`, etc.) still pass.

---

## Fix 3: Preserve Delimiter Lines in Function Body Replacement

### Problem Statement

`cifc(target:fn_name, value:-)` replaces a function body, but the opening `{` and closing `}` lines merge with the replacement content, producing malformed code.

Example:
```rust
fn foo() {
    let x = 1;
}
```

After `cifc(target:foo, value:"let y = 2;")`:
```rust
fn foo() { let y = 2;
}
```

The opening brace merged onto the first line of the replacement.

### Root Cause

`src/commands/chord_engine/resolver.rs`, function `find_brace_range` calls `scan_balanced` and `shrink_range`, which removes the `{` and `}` characters but leaves `start_col` on the same line as `{` if the brace is mid-line. When replacement doesn't begin with `\n`, it concatenates onto the same line.

### Implementation

**File**: `src/commands/chord_engine/resolver.rs`, function `find_brace_range`

1. Locate `find_brace_range` (use `ane exec src/commands/chord_engine/resolver.rs --chord "lefd"`)
2. After the `shrink_range` call, add logic to advance past newlines:

```rust
fn find_brace_range(buffer: &Buffer, scope_range: &TextRange, buffer_name: &str) -> Result<TextRange> {
    let mut range = scan_balanced(buffer, scope_range, '{', '}')
        .ok_or_else(|| ChordError::resolve(buffer_name, "no brace block found in scope"))?;

    // shrink past delimiters
    range = shrink_range(buffer, &range);

    // If start is at the end of the line (char after '{'), advance to next line start
    if range.start_col >= buffer.lines.get(range.start_line)
        .map(|l| line_char_count(l))
        .unwrap_or(0)
    {
        range.start_line += 1;
        range.start_col = 0;
    }

    // If end is at col 0 of the closing-brace line, retreat to end of previous line
    if range.end_col == 0 && range.end_line > range.start_line {
        range.end_line -= 1;
        range.end_col = buffer.lines.get(range.end_line)
            .map(|l| line_char_count(l))
            .unwrap_or(0);
    }

    Ok(range)
}
```

**Rationale**: 
- The first check (`start_col >= line_len`) detects when `start` is past the end of its line (i.e., after `{`). If so, advance to the next line's start.
- The second check (`end_col == 0 && end_line > start_line`) detects when `end` is at the very start of a line (i.e., before `}`). If so, move to the end of the previous line.
- Single-line functions like `fn foo() { x }` remain unchanged because start and end are on the same line, so neither condition triggers.

### Testing

Add three unit tests to `resolver.rs`:

```rust
#[test]
fn cifc_multiline_preserves_brace_lines() {
    let buffer = buf(&[
        "fn foo() {",
        "    let x = 1;",
        "    let y = 2;",
        "}"
    ]);
    
    let range = find_brace_range(&buffer, &full_function_scope, "test").unwrap();
    
    // Range should span only interior lines, not the { and } lines
    assert_eq!(range.start_line, 1); // "let x = 1;"
    assert_eq!(range.start_col, 4);  // indentation
    assert_eq!(range.end_line, 2);   // "let y = 2;"
}

#[test]
fn cifc_single_line_unchanged() {
    let buffer = buf(&["fn foo() { x }"]);
    
    let range = find_brace_range(&buffer, &full_function_scope, "test").unwrap();
    
    // Single-line function body: space after { to space before }
    assert_eq!(range.start_line, 0);
    assert_eq!(range.start_col, 10); // after "fn foo() {"
    assert_eq!(range.end_line, 0);
    // end_col should be just before the closing brace
}

#[test]
fn cifc_multiline_integration() {
    // Full integration: apply cifc and verify output
    let mut buffer = buf(&[
        "fn foo() {",
        "    let x = 1;",
        "}",
    ]);
    
    let new_content = "    let y = 2;";
    // (apply replacement via patcher)
    
    let lines = buffer.lines();
    assert_eq!(lines[0], "fn foo() {");
    assert_eq!(lines[1], "    let y = 2;");
    assert_eq!(lines[2], "}");
}
```

### Edge Cases

- **Single-line function** `fn foo() { x }`: Both start and end are on line 0. The start-advance condition (`start_col >= line_len`) is false (start is mid-line), so no change. ✓
- **Function body starting on same line as `{`** (e.g., `fn foo() { let x = 1;\n}`): After shrink, start is after `{` mid-line. Condition `start_col >= line_len` is false, so no advance. Range starts at `let x = 1;` on the same line. ✓
- **Empty function body** (e.g., `fn foo() {\n}`): After shrink, start and end might be on the same line (the blank line between braces). Behavior is correct for the empty-body case.

### Verification

Run `cargo test cifc_multiline_preserves_brace_lines` and related tests. Also run existing tests like `cifc_resolves_to_brace_contents` to ensure no regression.

---

## Fix 4: Document Name vs Definition Component

### Problem Statement

Agents confuse the `n` (Name) and `d` (Definition) components for Function scope. `cifn` targets only the bare identifier, while agents often need to change the full declaration including visibility and signature — which requires `cefd` (Definition). The skill and tool docs don't explain this distinction.

### Root Cause

No chord resolver bug. The resolver correctly implements both components. The issue is **documentation clarity**.

### Implementation

**File 1**: `src/data/ane-skill.md`

1. Open the skill content (use `ane exec src/data/ane-skill.md --chord "yebs"` to read it)
2. After the "Editing" section, add a new "Component guide: Name vs Definition" section:

```markdown
## Component guide: Name vs Definition

`n=Name` and `d=Definition` are frequently confused for Function scope:

- **`n` (Name)** — the bare identifier only (`foo` in `fn foo()`). Use it to rename.
- **`d` (Definition)** — the full declaration: visibility modifiers + `fn` + name + parameters + return type, but not the body. Use it to change visibility, add modifiers, or rewrite the signature.

Examples:

```
# Rename a function — Name component
ane exec f.rs --chord "cifn(target:old_name, value:\"new_name\")"

# Add visibility or change signature — Definition component
ane exec f.rs --chord "cefd(target:foo, value:\"pub(super) fn foo(x: i32) -> bool\")"
```

When unsure what a component covers, yank it first to inspect:

```
ane exec f.rs --chord "yefn(target:foo)"   # shows the identifier only
ane exec f.rs --chord "yefd(target:foo)"   # shows the full declaration
```
```

3. Verify the updated skill file is under 800 words (see Fix 4 test below)

**File 2**: `src/core.rs`, constant `TOOL_DESCRIPTION`

1. Locate the TOOL_DESCRIPTION (use `ane exec src/core.rs --chord "levd"` to find it)
2. Find the Edit examples block
3. Update the `cifn` example and add a `cefd` example:

```
  cifn(target:getData, value:"fetch") -> rename function (identifier only)
  cefd(target:handler, value:"pub fn handler(args)") -> change full declaration incl. visibility
```

Replace the single-line `cifn` example.

**File 3**: `src/data/skill.rs`

1. Locate or create the test module `#[cfg(test)] mod tests`
2. Add a word-count test for the skill content:

```rust
#[test]
fn skill_content_under_800_words() {
    let word_count = SKILL_CONTENT.split_whitespace().count();
    assert!(
        word_count <= 800,
        "SKILL_CONTENT has {word_count} words, expected <= 800"
    );
}
```

3. Run the test: `cargo test skill_content_under_800_words`
4. If the count exceeds 800, trim the guide to stay within the budget

### Verification

1. Run `cargo test skill_content_under_800_words` and confirm it passes
2. Run `cargo clippy -- -D warnings` to ensure no linting issues with the new markdown
3. Visually inspect the updated skill content in the TUI to confirm readability

---

## Fix 5: Auto-Prefix Newline for Append at Line End

### Problem Statement

`aebs(value:"new line")` or `aals(target:N, value:"new line")` when appending at line-end doesn't produce a blank separator line if the replacement doesn't explicitly start with `\n`. The leading `\n` is "swallowed" by the splice.

Example:
```
File content:
line 1
line 2

Append at EOF with value "new":
line 1
line 2new       ← concatenated to last char, no newline prefix
```

### Root Cause

`src/commands/chord_engine/patcher.rs`, function `build_action`, Append branch. When the insertion point is at end-of-line (col = line length), the replacement is concatenated directly. A leading `\n` advances to the next line but doesn't produce a blank separator — it just moves to where the file's trailing newline would have placed the cursor.

### Implementation

**File**: `src/commands/chord_engine/patcher.rs`, function `build_action`

1. Locate the `build_action` function (use `ane exec src/commands/chord_engine/patcher.rs --chord "lefd"`)
2. Find the `Action::Append` match arm
3. Update the Append logic to auto-prefix `\n` when inserting at line-end:

```rust
Action::Append => {
    let insertion = resolution.replacement.as_deref().unwrap_or("");
    let last = resolution.target_ranges.last().copied().unwrap();
    let point = TextRange::point(last.end_line, last.end_col);
    
    // Check if inserting at end of a line
    let line_len = buffer.lines.get(point.start_line)
        .map(|l| line_char_count(l))
        .unwrap_or(0);
    
    let prefixed;
    let effective = if point.start_col >= line_len && !insertion.starts_with('\n') {
        // At line-end and insertion doesn't start with newline: auto-prefix
        prefixed = format!("\n{insertion}");
        &prefixed
    } else {
        // Mid-line or insertion already starts with newline: use as-is
        insertion
    };
    
    apply_single_replacement(buffer, &point, effective)
}
```

**Rationale**:
- `point.start_col >= line_len` detects insertion at end-of-line (col equals or exceeds line length)
- `!insertion.starts_with('\n')` avoids double-newline if the caller explicitly provides one
- The auto-prefixed `\n` ensures the new content starts on a fresh line

### Testing

Add three unit tests to `patcher.rs`:

```rust
#[test]
fn append_at_line_end_auto_prefixes_newline() {
    let mut buffer = buf(&["line 1", "line 2"]);
    let range = TextRange::point(1, 6); // end of "line 2"
    
    let mut patcher = Patcher::new(&buffer);
    patcher.add_replacement(&range, "new text");
    patcher.apply_mut(&mut buffer).unwrap();
    
    // Verify "new text" is on a new line (line 3), not concatenated
    assert_eq!(buffer.lines()[2], "new text");
}

#[test]
fn append_mid_line_no_auto_prefix() {
    let mut buffer = buf(&["hello world"]);
    let range = TextRange::point(0, 5); // in middle of word (after "hello")
    
    let mut patcher = Patcher::new(&buffer);
    patcher.add_replacement(&range, " there");
    patcher.apply_mut(&mut buffer).unwrap();
    
    // Mid-line append: no auto-prefix, should concatenate
    assert_eq!(buffer.lines()[0], "hello there world");
}

#[test]
fn aebs_value_starts_on_new_line() {
    let mut buffer = buf(&["existing line"]);
    // Simulate aebs (Append Entire Buffer Self) at EOF
    let range = TextRange::point(0, 13); // end of buffer
    
    let mut patcher = Patcher::new(&buffer);
    patcher.add_replacement(&range, "new line");
    patcher.apply_mut(&mut buffer).unwrap();
    
    // Should append on a new line
    assert!(buffer.to_content().ends_with("\nnew line\n"));
}
```

### Edge Cases

- **Inline mid-line append** (col < line_len): `point.start_col >= line_len` is false, so no auto-prefix. Existing inline append (e.g., "hello" → "hel_there_lo") is unaffected. ✓
- **Value already starts with `\n`**: Condition `!insertion.starts_with('\n')` prevents double newline. ✓
- **Empty value with Append**: `insertion = ""`, auto-prefix produces `"\n"`, inserting a blank line. This is correct — "append an empty line after" is valid. ✓
- **Interaction with Fix 2**: After Fix 2, `aals(target:N, value:"text")` resolves to end-of-line-N. Fix 5 auto-prefixes `\n`, so insertion happens on line N+1. Together, they implement the expected double-fix behavior. ✓

### Verification

Run `cargo test append_at_line_end_auto_prefixes_newline` and related tests. Also verify that existing inline-append tests like `append_action_inserts_after_range_end` still pass without modification.

---

## Integration Testing

### Full Workflow Tests

After all five fixes are implemented, verify they work together:

**Test**: Multi-line function body replacement with line-end append

```rust
#[test]
fn integration_replace_function_and_append_content() {
    let mut buffer = buf(&[
        "fn main() {",
        "    println!(\"old\");",
        "}",
        "",
    ]);
    
    // First: replace function body (Fix 3)
    // Second: append new function at EOF (Fix 2 + Fix 5)
    // Expected: both operations succeed, output is well-formed
}
```

**Test**: Buffer scope with content append

```rust
#[test]
fn integration_buffer_scope_append_newline_prefix() {
    let mut buffer = buf(&["line 1", "line 2"]);
    
    // Append to entire buffer without newline in value (Fix 1 + Fix 5)
    // Expected: auto-newline prefix, content on fresh line
}
```

### Regression Test Suite

Run the full test suite after all fixes:

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Expected: all tests pass, no new warnings or formatting issues.

---

## Checklist for Completion

- [ ] **Fix 1**: Buffer scope guard updated, tests passing
- [ ] **Fix 2**: `apply_positional` updated for After/Before Self_, tests passing
- [ ] **Fix 3**: `find_brace_range` preserves delimiter lines, tests passing
- [ ] **Fix 5**: `build_action` Append auto-prefixes newline, tests passing
- [ ] **Fix 4**: Skill content updated with Name/Definition guide, word count ≤ 800
- [ ] **Fix 4**: TOOL_DESCRIPTION updated with both `cifn` and `cefd` examples
- [ ] **Fix 4**: Word count test added to `skill.rs`
- [ ] **Integration tests**: Multi-fix workflows validated
- [ ] **Regression suite**: `cargo test`, `cargo clippy`, `cargo fmt` all pass
- [ ] **Code review**: CLAUDE.md style guidelines followed (no unnecessary comments, idiomatic Rust)

---

## Common Pitfalls

1. **Fix 2 scope**: Ensure you update both `Positional::After` and `Positional::Before`. They are symmetric bugs.

2. **Fix 3 column arithmetic**: The `line_char_count(l)` function returns the character count, not the byte count. Use it consistently when comparing `start_col` or `end_col`.

3. **Fix 5 interaction**: The `point.start_col >= line_len` check must use the **line length**, not the length of a single character. Verify the line is fetched before the comparison.

4. **Fix 1 test**: After updating the guard, the old test `error_message_exec_requires_explicit_params` will need to be modified to exclude `yebs` from the error assertion. Do not delete the test; update it.

5. **Fix 4 word count**: Monitor the skill content word count carefully. The new section should fit comfortably under 800 words. If it doesn't, trim examples or condense explanations.

---

## Helpful Commands

Use `ane` to explore and edit the files:

```bash
# List all functions in a file
ane exec src/commands/chord.rs --chord "lefd"

# Read a specific function
ane exec src/commands/chord.rs --chord "yefs(target:execute_chord)"

# Change a function
ane exec src/commands/chord.rs --chord "cifc(target:execute_chord, value:-)"

# List all variables in a file
ane exec src/commands/chord_engine/resolver.rs --chord "levd"
```

---

## References

- **Full specification**: [0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md)
- **Architecture**: See `src/` layout and Layer 0/1/2 boundaries in [CLAUDE.md](/workspace/CLAUDE.md)
- **Chord system**: [docs/01-chord-system.md](/workspace/docs/01-chord-system.md)
- **Test examples**: Existing unit tests in `src/commands/chord_engine/resolver.rs` and `patcher.rs`
