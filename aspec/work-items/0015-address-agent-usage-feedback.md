# Work Item: Bug

Title: address agent usage feedback
Issue: issuelink

## Summary:
Fix six ane agent-usage bugs surfaced in `ane-findings.md` so that buffer-scope chords, line-append, function-body replacement, function-name editing, and content-append all behave as a code agent expects them to. Findings are reproduced below for reference.

- **Finding 1**: Buffer-scope chords (`yebs`, `aebs`, `debs`, etc.) reject execution when no args are passed, requiring a meaningless `target:1` workaround.
- **Finding 2**: `aals(target:N)` inserts at end of file instead of immediately after line N.
- **Finding 3**: `cifc` merges the opening `{` with the first replacement line and the closing `}` with the last replacement line when replacing a multi-line function body.
- **Finding 4**: `cifn(value:"pub(super) fn foo")` produces `fn pub(super) fn foo` — the agent used the wrong component. `cifn` (Name component) correctly targets only the bare identifier; the right chord for changing the full declaration including visibility is `cifd`/`cefd` (Definition component). The skill and tool definitions do not explain this distinction, so agents make the wrong chord choice.
- **Finding 5**: Appending with `aebs(value:-)` where stdin begins with `\n` does not produce a blank separator line; the leading newline is swallowed by the splice.
- **Finding 6**: Consequence of Findings 2, 3, and 5 — large multi-line insertions fell back to built-in Edit. Resolved when the above are fixed.


## User Stories

### User Story 1:
As a: code agent

I want to:
Read or operate on an entire file buffer using a chord like `yebs` or `aebs` without supplying a dummy `target` argument

So I can:
Use buffer-scope chords idiomatically — a buffer has no named symbol or line number, so requiring one is both confusing and undocumented

### User Story 2:
As a: code agent

I want to:
Append content immediately after a specific line using `aals(target:N)`, with the new content starting on the following line

So I can:
Insert code or text at exact positions in large files without the insertion landing at the end of the file

### User Story 3:
As a: code agent

I want to:
Replace a function body with `cifc(target:fn_name, value:-)` and have the opening `{` and closing `}` lines remain intact and on their own lines

So I can:
Rewrite a function's implementation in one chord without producing malformed code that requires a follow-up fix


## Implementation Details:

### Fix 1 — Allow buffer-scope chords with empty args

**File**: `src/commands/chord.rs`, function `execute_chord`

The guard that rejects empty args:

```rust
if args_are_empty(&chord.args) && chord.action != Action::List {
    bail!("exec mode requires explicit parameters …");
}
```

Add a second exemption for buffer scope:

```rust
if args_are_empty(&chord.args)
    && chord.action != Action::List
    && chord.scope != Scope::Buffer
{
    bail!("exec mode requires explicit parameters …");
}
```

`Scope::Buffer` is whole-file by definition. No target is meaningful, so no target should be required. Update the test `error_message_exec_requires_explicit_params` to assert the error is NOT raised for a `yebs` chord with empty args, and add a new passing test `execute_yank_entire_buffer_no_args_succeeds` that reads a temp file with `yebs` (no args) and asserts the full file content is returned.

### Fix 2 — `aals` appends at wrong location

**File**: `src/commands/chord_engine/resolver.rs`, function `apply_positional`

`Positional::After` with `Component::Self_` currently computes `end_line/end_col` as the last line of the buffer:

```rust
Positional::After => {
    let (end_line, end_col) = if query.component == Component::Self_ {
        let last = buffer.line_count().saturating_sub(1);
        (last, buffer.lines.get(last).map(|l| line_char_count(l)).unwrap_or(0))
    } else { … };
    Ok(vec![TextRange {
        start_line: component_range.end_line,
        start_col: component_range.end_col,
        end_line,
        end_col,
    }])
}
```

The patcher's Append action uses `last.end_line, last.end_col` of the resolved range as its insertion point. When `end` is the buffer tail, the insertion lands at EOF.

Change the `Self_` branch to use `component_range.end` for both start and end, producing a zero-length point range:

```rust
if query.component == Component::Self_ {
    return Ok(vec![TextRange::point(component_range.end_line, component_range.end_col)]);
}
```

Apply the same correction to `Positional::Before + Self_` (which has the symmetric bug — it spans from buffer start to component start, so Prepend inserts at buffer start rather than at component start):

```rust
Positional::Before => {
    if query.component == Component::Self_ {
        return Ok(vec![TextRange::point(component_range.start_line, component_range.start_col)]);
    }
    …
}
```

The Append patcher already uses `last.end` as its insertion point. After this fix, `last.end` equals `component_range.end` (end of line N), which is correct. Prepend uses `first.start`; after the fix, `first.start` equals `component_range.start` (start of line N), which is also correct.

Verify that `aebs` (Entire positional, not After) is unaffected — it continues to resolve the full buffer range via `Positional::Entire` and inserts at buffer end.

Add tests:
- `append_after_line_self_inserts_at_line_end` — verify `aals(target:2)` on a 5-line buffer inserts after line 2, not at EOF.
- `prepend_before_line_self_inserts_at_line_start` — verify `pbls(target:3)` on a 5-line buffer inserts before line 3, not at buffer start.

### Fix 3 — `cifc` merges delimiter lines with replacement

**File**: `src/commands/chord_engine/resolver.rs`, function `find_brace_range` (called by `resolve_contents_component`)

`scan_balanced` returns a range spanning from the `{` character to one past `}`. `shrink_range` strips one character from each end, leaving the range starting at the character immediately after `{` — still on the same line as `{` if the opening brace is mid-line. When the replacement does not begin with `\n`, the new content runs onto the same line as `{`.

After `find_brace_range` returns, advance `start` past any immediately-following newline, and retreat `end` before any immediately-preceding newline:

```rust
fn find_brace_range(buffer: &Buffer, scope_range: &TextRange, buffer_name: &str) -> Result<TextRange> {
    let mut range = scan_balanced(buffer, scope_range, '{', '}')
        .ok_or_else(|| ChordError::resolve(buffer_name, "no brace block found in scope"))?;

    // shrink past delimiters
    range = shrink_range(buffer, &range);

    // if the character right after '{' is a newline, advance to the next line start
    if range.start_col >= buffer.lines.get(range.start_line).map(|l| line_char_count(l)).unwrap_or(0) {
        range.start_line += 1;
        range.start_col = 0;
    }

    // if the character right before '}' is a newline (i.e., end is at col 0 of closing-brace line),
    // retreat to the end of the previous line
    if range.end_col == 0 && range.end_line > range.start_line {
        range.end_line -= 1;
        range.end_col = buffer.lines.get(range.end_line).map(|l| line_char_count(l)).unwrap_or(0);
    }

    Ok(range)
}
```

This makes the resolved range span only the interior lines of the function body, leaving the `{` and `}` delimiter lines untouched. Replacement values need no leading or trailing newline for standard multi-line bodies.

Note: single-line functions (`fn foo() { body }`) remain handled by the existing `shrink_range` path and do not trigger the newline-advance logic since start and end are on the same line.

Add tests:
- `cifc_multiline_preserves_brace_lines` — assert that after `cifc` on a multi-line function, the opening `{` line and closing `}` line are unchanged.
- `cifc_single_line_unchanged` — assert single-line function body replacement still works.

### Fix 4 — Clarify Name (`n`) vs Definition (`d`) component in skill and tool docs

No chord resolver code changes. The `cifn` behaviour is correct: `n` (Name) targets only the bare identifier. The fix is to teach agents which component to use for which task by updating both the built-in skill document and the MCP tool description.

**File 1**: `src/data/ane-skill.md`

Add a "Component guide" section after the Editing section. The skill currently runs ~491 words; the new section may bring the total to at most 800 words. Add a test `skill_content_under_800_words` in `src/data/skill.rs` to enforce this budget.

Content to add:

```markdown
## Component guide: Name vs Definition

`n=Name` and `d=Definition` are frequently confused for Function scope:

- **`n` (Name)** — the bare identifier only (`foo` in `fn foo()`). Use it to rename.
- **`d` (Definition)** — the full declaration: visibility modifiers + `fn` + name + parameters + return type, but not the body. Use it to change visibility, add modifiers, or rewrite the signature.

```
# rename a function — Name component
ane exec f.rs --chord "cifn(target:old_name, value:\"new_name\")"

# add visibility or change signature — Definition component
ane exec f.rs --chord "cefd(target:foo, value:\"pub(super) fn foo(x: i32) -> bool\")"
```

When unsure what a component covers, yank it first to inspect the range:

```
ane exec f.rs --chord "yefn(target:foo)"   # shows the identifier only
ane exec f.rs --chord "yefd(target:foo)"   # shows the full declaration
```
```

**File 2**: `src/core.rs`, constant `TOOL_DESCRIPTION`

Update the Edit examples block to show both `cifn` and `cefd` with explicit labels, replacing the existing single `cifn` line:

```
  cifn(target:getData, value:"fetch") -> rename function (identifier only)
  cefd(target:handler, value:"pub fn handler(args)") -> change full declaration incl. visibility
```

The TOOL_DESCRIPTION test (`tool_description_under_250_words`) currently allows ≤250 words; the added line keeps the count well under that ceiling. No test change needed unless the total exceeds 250 after the edit — verify with `cargo test` and adjust the limit if necessary.

**File 3**: `src/data/skill.rs` — add test:

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

### Fix 5 — `aebs` swallows leading newline

**File**: `src/commands/chord_engine/patcher.rs`, `build_action` Append branch

When inserting at end-of-line (where the insertion point's column equals the length of that line), the replacement is concatenated directly after the last character of the line. A leading `\n` in the replacement advances to the next line but doesn't produce a blank line — it merely moves to where the file's own trailing newline would have taken the cursor.

Change the Append action to check whether the insertion point is at the end of a line and, if so, prefix the replacement with `\n` automatically:

```rust
Action::Append => {
    let insertion = resolution.replacement.as_deref().unwrap_or("");
    let last = resolution.target_ranges.last().copied().unwrap();
    let point = TextRange::point(last.end_line, last.end_col);
    let line_len = buffer.lines.get(point.start_line)
        .map(|l| line_char_count(l))
        .unwrap_or(0);
    let prefixed;
    let effective = if point.start_col >= line_len && !insertion.starts_with('\n') {
        prefixed = format!("\n{insertion}");
        &prefixed
    } else {
        insertion
    };
    apply_single_replacement(buffer, &point, effective)
}
```

This ensures that `aebs(value:"## New Section")` inserts on a genuinely new line rather than concatenating to the last character of the buffer. The same applies to `aals(target:N)` after Fix 2 — inserting at end-of-line N gets a `\n` prefix automatically, so the appended content starts on line N+1. Inline mid-line append (e.g. `aals` on a partially-resolved in-line range) is unaffected because `point.start_col < line_len`.

Add tests:
- `append_at_line_end_auto_prefixes_newline` — verify that appending at col = line length automatically prepends `\n`.
- `append_mid_line_no_auto_prefix` — verify that appending mid-line (col < line length) does not add `\n`.
- `aebs_value_starts_on_new_line` — full integration: `aebs` on a file ending in `\n`, value `"new"`, assert content ends with `\nnew\n`.


## Edge Case Considerations:

- **Fix 1 — `cebs` without value**: After exempting Buffer scope from the empty-args guard, `cebs` (Change Entire Buffer Self) with no value would silently clear the file. Since `cebs` carries a `value` arg, it still naturally fails at the resolver level when `value` is absent; the guard exemption does not create an accidental wipe path. Confirm this in the test for `cebs` with no args.
- **Fix 2 — `aals` on last line of file**: `component_range.end_line` may already be the last line and `end_col` may be the last column. Fix 5's auto-`\n` prefix covers this: the insertion produces a new line even at EOF. Ensure `lines_to_content` doesn't add a double newline.
- **Fix 2 — `aals` on empty file**: Buffer has 0 lines. `component_range` from `resolve_line_scope` would error when the target line doesn't exist. Existing behaviour (error) is correct; Fix 2 does not change this path.
- **Fix 3 — single-line function `fn foo() { x }`**: `shrink_range` returns a range on the same line; the newline-advance logic checks `start_col >= line_len`, which is false for a mid-line start. Falls through unchanged. Verify no regression with `cifc_single_line_unchanged`.
- **Fix 3 — function body starting on same line as `{`**: e.g. `fn foo() { let x = 1;\n}`. After shrink, start is after `{` mid-line. The newline-advance condition is not met (start_col < line_len), so the range starts at `let x = 1;`. Replacement starting with `let y = 2;` gives `{ let y = 2;\n}`. This is correct Entire-vs-Inside semantics.
- **Fix 4 — skill word count**: adding the Component guide section is expected to bring the skill to roughly 650–700 words. Verify the count stays under 800 before merging; if further guidance is added later, the test will catch overruns before the document becomes unwieldy for agents to process.
- **Fix 4 — `cefd` value must include the full signature**: agents need to understand that the Definition component covers parameters and return type, not just the name. The examples in the skill and tool description show a complete signature to set this expectation.
- **Fix 4 — `yefd` yank-first pattern**: document the inspect-before-edit pattern (`yefd` to see the current declaration, then `cefd` to replace it) to lower the chance of an agent constructing a malformed replacement value.
- **Fix 5 — inline Append (mid-line)**: `point.start_col < line_len` — no `\n` is added. Existing append test `append_action_inserts_after_range_end` (which tests mid-word inline append) must still pass.
- **Fix 5 — value already starts with `\n`**: condition `!insertion.starts_with('\n')` prevents a double newline. If the caller explicitly wants two blank lines, they pass `\n\n…`.
- **Fix 5 — empty value with Append**: `insertion = ""`, auto-prefix produces `"\n"`, inserting a blank line. This is correct — "append an empty line after" is a valid operation.
- **Interaction of Fix 2 + Fix 5**: `aals(target:N, value:"new line")` after Fix 2 resolves the insertion point to end-of-line-N (col = line_len). Fix 5 then prepends `\n`, giving `\nnew line` — insertion on line N+1. This is the expected double-fix behaviour.


## Test Considerations:

- For each fix, add at least one unit test in the same `#[cfg(test)] mod tests` block as the function being changed — follow the existing pattern of inline tests in each source file.
- All new tests use the existing `buf(lines)` / `named_buf(path, lines)` / `query(…)` helpers already present in `resolver.rs` and `patcher.rs`.
- Regression: run the full test suite (`cargo test`) after each fix to confirm no pre-existing tests break. The following tests are most likely to be affected and should be audited:
  - `error_message_exec_requires_explicit_params` (Fix 1 — update to reflect new buffer-scope exemption)
  - `append_action_inserts_after_range_end` (Fix 5 — ensure mid-line inline append is unaffected)
  - `positional_entire_returns_component` (Fix 2 — ensure Entire positional is unchanged)
  - `cifc_resolves_to_brace_contents` and `cifc_multiline_resolves_contents` (Fix 3 — update expected ranges)
  - `cifn_with_selection_range_targets_identifier` (Fix 4 — no change needed; the resolver is unchanged)
- Add integration-level exec tests in `chord.rs` (where `execute_chord` is tested end-to-end with temp files) for Fix 1, Fix 2, and Fix 5, since those fixes touch the execution path rather than just the resolver.


## Codebase Integration:

- All fixes are in Layer 1 (`src/commands/`). No Layer 0 or Layer 2 changes are needed.
- Fix 1 touches `src/commands/chord.rs` (`execute_chord`). Import `Scope` from `crate::data::chord_types` — already imported in that file.
- Fix 2 touches `src/commands/chord_engine/resolver.rs` (`apply_positional`). No new imports; `TextRange::point` is already used in that function.
- Fix 3 touches `src/commands/chord_engine/resolver.rs` (`find_brace_range`). Uses existing `shrink_range`, `line_char_count`, and `Buffer` APIs.
- Fix 4 touches `src/data/ane-skill.md` (new Component guide section), `src/core.rs` `TOOL_DESCRIPTION` (updated Edit examples), and `src/data/skill.rs` (new `skill_content_under_800_words` test). No changes to the chord resolver or any Layer 1/Layer 2 code.
- Fix 5 touches `src/commands/chord_engine/patcher.rs` (`build_action`). Uses existing `line_char_count` from `super::text`. No new dependencies.
- Follow existing code style: no comments unless the why is non-obvious, `anyhow::Result` for errors, no public API changes (all affected functions are `pub(crate)` or private).
- Run `cargo clippy -- -D warnings` and `cargo fmt --check` before marking the work item complete.
