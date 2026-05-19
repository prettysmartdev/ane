# Work Item 0015: Quick Reference

**Title**: Address Agent Usage Feedback  
**Status**: Ready for Implementation  
**Complexity**: Medium (5 independent fixes + integration testing)  
**Time Estimate**: 4-6 hours for experienced developer

---

## Fix Summary Table

| # | Title | File | Function | Impact | Tests |
|---|-------|------|----------|--------|-------|
| 1 | Buffer-scope empty args | `src/commands/chord.rs` | `execute_chord` | Allow `yebs` without `target` | 2 new |
| 2 | Append at correct line | `src/commands/chord_engine/resolver.rs` | `apply_positional` | Fix `aals` + `pbls` positioning | 2 new |
| 3 | Preserve brace lines | `src/commands/chord_engine/resolver.rs` | `find_brace_range` | Fix `cifc` delimiter merging | 3 new |
| 4 | Document Name vs Definition | `src/data/ane-skill.md`, `src/core.rs` | (doc updates) | Clarify component choice | 1 new (word count) |
| 5 | Auto-prefix newline | `src/commands/chord_engine/patcher.rs` | `build_action` | Fix `aebs` newline handling | 3 new |

---

## Implementation Checklist

### Phase 1: Fixes 1–3 (Independent, no dependencies)

- [ ] **Fix 1**: Add `Scope::Buffer` exemption to guard
  - [ ] Update guard condition
  - [ ] Update `error_message_exec_requires_explicit_params` test
  - [ ] Add `execute_yank_entire_buffer_no_args_succeeds` test
  - [ ] Run: `cargo test error_message_exec_requires_explicit_params execute_yank_entire_buffer_no_args_succeeds`

- [ ] **Fix 2**: Replace `Positional::After` and `Positional::Before` Self_ logic
  - [ ] Update `Positional::After` → use `TextRange::point(component_range.end_line, component_range.end_col)`
  - [ ] Update `Positional::Before` → use `TextRange::point(component_range.start_line, component_range.start_col)`
  - [ ] Add `append_after_line_self_inserts_at_line_end` test
  - [ ] Add `prepend_before_line_self_inserts_at_line_start` test
  - [ ] Run: `cargo test append_after_line_self prepend_before_line_self`

- [ ] **Fix 3**: Update `find_brace_range` to advance past newlines
  - [ ] After `shrink_range`, add start-advance logic (if at EOL after `{`, go to next line start)
  - [ ] Add end-retreat logic (if at BOL before `}`, go to end of previous line)
  - [ ] Add `cifc_multiline_preserves_brace_lines` test
  - [ ] Add `cifc_single_line_unchanged` test
  - [ ] Add `cifc_multiline_integration` test
  - [ ] Run: `cargo test cifc_multiline cifc_single_line`

### Phase 2: Fix 5 (Depends on understanding Fix 2)

- [ ] **Fix 5**: Update `build_action` Append branch to auto-prefix `\n`
  - [ ] Check if inserting at line-end: `point.start_col >= line_len`
  - [ ] If true and insertion doesn't start with `\n`, prefix: `format!("\n{insertion}")`
  - [ ] Add `append_at_line_end_auto_prefixes_newline` test
  - [ ] Add `append_mid_line_no_auto_prefix` test
  - [ ] Add `aebs_value_starts_on_new_line` test
  - [ ] Run: `cargo test append_at_line_end append_mid_line aebs_value`

### Phase 3: Fix 4 (Documentation only)

- [ ] **Fix 4a**: Update `src/data/ane-skill.md`
  - [ ] Add "Component guide: Name vs Definition" section after Editing
  - [ ] Include `yefn` / `yefd` inspection examples
  - [ ] Include `cifn` rename example
  - [ ] Include `cefd` definition example
  - [ ] Keep total word count ≤ 800

- [ ] **Fix 4b**: Update `src/core.rs` TOOL_DESCRIPTION
  - [ ] Find Edit examples block
  - [ ] Replace single `cifn` line with two lines: `cifn` (identifier) + `cefd` (definition)
  - [ ] Keep total word count ≤ 250

- [ ] **Fix 4c**: Add test to `src/data/skill.rs`
  - [ ] Add `skill_content_under_800_words` test
  - [ ] Run: `cargo test skill_content_under_800_words`

### Phase 4: Integration & Regression

- [ ] **Integration tests**: Create multi-fix scenario tests
  - [ ] Replace function body + append new function (Fixes 2, 3, 5)
  - [ ] Use buffer scope + append with auto-newline (Fixes 1, 5)

- [ ] **Full regression suite**:
  - [ ] `cargo test` (all tests pass)
  - [ ] `cargo clippy -- -D warnings` (no new warnings)
  - [ ] `cargo fmt --check` (formatting clean)

---

## Code Snippets for Each Fix

### Fix 1 Guard
```rust
if args_are_empty(&chord.args)
    && chord.action != Action::List
    && chord.scope != Scope::Buffer
{
    bail!("exec mode requires explicit parameters …");
}
```

### Fix 2 After/Before
```rust
Positional::After => {
    if query.component == Component::Self_ {
        return Ok(vec![TextRange::point(component_range.end_line, component_range.end_col)]);
    }
    // ... existing non-Self_ logic
}

Positional::Before => {
    if query.component == Component::Self_ {
        return Ok(vec![TextRange::point(component_range.start_line, component_range.start_col)]);
    }
    // ... existing non-Self_ logic
}
```

### Fix 3 Brace Range
```rust
// After shrink_range call:

// Advance start past opening brace's newline
if range.start_col >= buffer.lines.get(range.start_line)
    .map(|l| line_char_count(l))
    .unwrap_or(0)
{
    range.start_line += 1;
    range.start_col = 0;
}

// Retreat end before closing brace's newline
if range.end_col == 0 && range.end_line > range.start_line {
    range.end_line -= 1;
    range.end_col = buffer.lines.get(range.end_line)
        .map(|l| line_char_count(l))
        .unwrap_or(0);
}
```

### Fix 5 Append Auto-Prefix
```rust
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
```

---

## Key Insights

1. **Buffer scope is whole-file**: No target argument is meaningful or required (Fix 1)
2. **Positional is directional**: `After` points to end-of-component; `Before` points to component-start (Fix 2)
3. **Delimiter preservation requires line-level awareness**: Advance/retreat past newlines to keep braces on separate lines (Fix 3)
4. **Component definitions matter for agents**: Name = identifier only; Definition = full declaration (Fix 4)
5. **End-of-line append needs auto-newline**: Agents expect replacement to start on fresh line, not concatenate (Fix 5)

---

## Testing Strategy

- **Unit tests**: Each fix has 2–3 unit tests in the same file as the change
- **Integration tests**: Multi-fix workflows validated after all fixes are complete
- **Regression tests**: Full suite ensures no pre-existing tests break
- **Word count test**: Fix 4 includes a test to enforce documentation budget

**Run tests in phases**:
1. After each fix: `cargo test` for that module
2. After all fixes: `cargo test` for full codebase
3. Before merge: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`

---

## Dependencies & Interactions

```
Fix 1 (buffer guard)     [independent]
Fix 2 (append position)  [independent]
Fix 3 (brace lines)      [independent]
         ↓
Fix 5 (newline prefix)   [depends on understanding Fix 2]
         ↓
Fix 4 (documentation)    [independent, documents all fixes]
         ↓
Integration tests        [validate 1+2+3+5 working together]
```

---

## Common Issues & Solutions

| Issue | Solution |
|-------|----------|
| Fix 2: Tests fail for Before/After | Verify **both** After and Before are updated (symmetric bugs) |
| Fix 3: Merging delimiters still occur | Check that `shrink_range` is called **before** newline logic |
| Fix 5: Double newlines appear | Verify `!insertion.starts_with('\n')` guard is in place |
| Fix 4: Word count test fails | Trim examples or abbreviations in skill content; test budget is 800 words |
| Regression: Existing tests fail | Check that mid-line append (Fix 5) preserves inline behavior (`start_col < line_len`) |

---

## Helpful ane Commands

```bash
# Explore the file structure
ane exec src/commands/chord.rs --chord "lefd"                  # list functions
ane exec src/commands/chord_engine/resolver.rs --chord "lefd"  # list functions

# Read a specific function before editing
ane exec src/commands/chord.rs --chord "yefs(target:execute_chord)"
ane exec src/commands/chord_engine/resolver.rs --chord "yefs(target:apply_positional)"
ane exec src/commands/chord_engine/resolver.rs --chord "yefs(target:find_brace_range)"
ane exec src/commands/chord_engine/patcher.rs --chord "yefs(target:build_action)"

# Search for test examples
ane exec src/commands/chord_engine/resolver.rs --chord "lefd"  # find test modules

# Read skill content to check word count
ane exec src/data/ane-skill.md --chord "yebs"
```

---

## References

- **Full specification**: [0015-address-agent-usage-feedback.md](0015-address-agent-usage-feedback.md)
- **Implementation guide**: [0015-IMPLEMENTATION.md](0015-IMPLEMENTATION.md)
- **Project CLAUDE.md**: [/workspace/CLAUDE.md](/workspace/CLAUDE.md)
- **Architecture**: [/workspace/docs/07-architecture-overview.md](/workspace/docs/07-architecture-overview.md)
- **Chord system**: [/workspace/docs/01-chord-system.md](/workspace/docs/01-chord-system.md)
