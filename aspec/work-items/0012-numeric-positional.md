# Work Item: Feature

Title: numeric positional
Issue: issuelink

## Summary:
Add a new kind of positional — a numeric one. The second part of a chord can now be a single digit (1–9). When present, the digit replaces the letter positional and means "next N of whatever the scope/component identifies, forward from the cursor."

Examples:
- `j5lw` — Jump, 5, Line, Word → jump forwards 5 words in the current line
- `j5ls` — Jump, 5, Line, Self → jump forwards 5 lines in the buffer
- `l5fd` — List, 5, Function, Definition → list the next 5 function definitions from the cursor
- `c5ls` — Change, 5, Line, Self → change within the next 5 lines

Constraints for this work item: no negative numbers, single digit only (1–9).

## User Stories

### User Story 1:
As a: user

I want to:
Type a count digit in the positional position of a chord (e.g., `j5lw`) to move or operate over N occurrences forward from my cursor

So I can:
Navigate and edit efficiently without repeating the same chord multiple times or reaching for arrow keys


### User Story 2:
As a: user

I want to:
Use `l5fd` to list the next 5 function definitions from my current cursor position

So I can:
Quickly preview upcoming symbols in a large file without scrolling or listing everything in the buffer


### User Story 3:
As a: code agent

I want to:
Pass a numeric positional in CLI exec mode (e.g., `ane exec file.rs c5ls(cursor:"10,0")`) and have it apply the action across N lines starting from the given cursor

So I can:
Make multi-line changes with a single chord instead of issuing N separate commands


## Implementation Details:

### Layer 0 — `src/data/chord_types.rs`

Add a `Count(u8)` variant to the `Positional` enum:

```rust
pub enum Positional {
    // existing variants...
    Count(u8),  // 1–9, single digit, forward direction
}
```

`u8` is `Copy`, so the enum can still derive `Copy`. Update every `match` on `Positional`:

- `short()` — return the static digit string. Since only 1–9 are valid, match each explicitly:
  `Count(1) => "1"`, …, `Count(9) => "9"`, `Count(_) => unreachable!()`.
- `from_short(s)` — add a branch: if `s` is a single ASCII digit 1–9, parse it and return `Some(Count(n))`. Zero is not matched (returns `None` → parse error).
- `Display` — `Count(n)` formats as the digit `n`.
- `long_form()` in `ChordQuery` calls `Display`, so `long_form()` will naturally emit e.g. `Jump5LineWord`.
- `short_form()` in `ChordQuery` calls `short()`, which works unchanged.

Add a new validation helper:

```rust
pub fn is_valid_count_scope(scope: Scope) -> bool {
    !matches!(scope, Scope::Buffer | Scope::Delimiter)
}
```

- `Buffer` scope: there is only one buffer, so "next 5 buffers" is meaningless.
- `Delimiter` scope: text-based delimiter scanning does not have natural N-repetition semantics (deferred to a future work item).

### Layer 1 — `src/commands/chord_engine/parser.rs`

**Short-form parsing** (`try_parse_short_form`): The chord is still exactly 4 characters. `chars[1]` may now be a digit. `Positional::from_short(chars[1])` already returns `Some(Count(n))` once Layer 0 is updated — no structural change needed here. Just add the scope validation after parsing:

```rust
if let Positional::Count(_) = positional {
    if !is_valid_count_scope(scope) {
        return Err(ChordError::parse(..., "numeric positional is not valid for Buffer or Delimiter scope").into());
    }
}
```

Also validate that the action is not `Replace` with a count (find-replace within N occurrences is out of scope for this item, so reject clearly).

**Long-form parsing** (`try_parse_long_form`): After stripping the action prefix in `parse_long_action`, the remainder may start with a digit before the scope word. In `parse_long_positional`, try to consume a leading ASCII digit 1–9 before attempting the existing word prefixes:

```rust
// in parse_long_positional, try digit first
if let Some(ch) = input.chars().next() {
    if ch.is_ascii_digit() && ch != '0' {
        let n = ch as u8 - b'0';
        return Some((Positional::Count(n), &input[1..]));
    }
}
// fall through to existing word-based matching
```

**Cursor requirement**: A count positional implies movement from the current cursor position, so add a validation step (parallel to the existing `Next`/`Previous`/`Until`/`To` checks in `execute_chord`) that warns in CLI mode when no `cursor` arg is provided. Do not reject at parse time — the TUI always provides cursor context implicitly.

**`suggest_chord`**: Update the digit characters to not appear in the Levenshtein candidate set (candidates stay letter-only), to avoid confusing suggestions when a numeric chord is mistyped.

### Layer 1 — `src/commands/chord_engine/resolver.rs`

When `ChordQuery::positional == Positional::Count(n)`, the resolver treats it as "next N from cursor." Specifically:

- **Line scope + Self component** (`c5ls`, `d5ls`, `y5ls`): select N consecutive lines starting at the cursor line (lines `cursor.line` through `cursor.line + n - 1`). Clamp to buffer end; do not error on overflow.
- **Line scope + Word component** (`j5lw`): scan forward through the current line, collecting the next N whitespace-delimited word boundaries. Target the position just after the Nth word. If fewer than N words remain on the line, advance to end of line.
- **LSP-backed scopes (Function, Variable, Struct, Member)**: call `document_symbols()`, filter to the matching kind, sort by position, find the first symbol that starts after the cursor, then take the next `n` symbols. For List action, this produces a list of at most N items. For other actions, the target range spans from the first to the last of the N symbols.
- **Delimiter scope**: blocked at parse time (see Layer 0), so no resolver handling needed.

When `n` is larger than the number of remaining occurrences, clamp gracefully — do not error. Emit a warning in the `ChordResult::warnings` field: `"only M of N requested occurrences found"`.

### Layer 1 — `src/commands/chord_engine/patcher.rs`

No structural changes needed. The patcher operates on the `target_ranges` produced by the resolver. A count positional may produce a single merged range (for contiguous line operations) or a list of disjoint ranges (for non-contiguous symbols). The patcher already handles `Vec<TextRange>`, so both cases are covered.

### Layer 2 — `src/frontend/tui/app.rs` and chord display

If any TUI code renders the positional name as a label (e.g., in a status bar or chord preview), ensure it handles `Positional::Count(n)` by displaying the digit. Check any match-exhaustiveness errors after the enum change.

The TUI's implicit cursor context means count chords work without an explicit `cursor` arg — the resolver receives the current cursor position from editor state, same as other cursor-dependent positionals.


## Edge Case Considerations:

- **Count 0**: `from_short("0")` returns `None`, causing a parse error. Error message should say "count must be 1–9; 0 is not a valid positional."
- **Count ≥ 10 in short form**: The chord is exactly 4 characters, so `j15lw` is 5 characters and will not match the short-form parser (length check fails). It will then fail long-form parsing too. Error message should say "only single-digit counts (1–9) are supported."
- **Long form digit collisions**: Existing long-form positionals (`Inside`, `Until`, etc.) all start with a letter, so a leading digit unambiguously signals a count. No collision risk.
- **Count with `Outside` positional**: `Outside` is a letter positional, not a count, so there is no interaction. `Count(n)` does not combine with letter positionals; they occupy the same slot in the chord.
- **Buffer scope**: Rejected at parse time with a clear error. "Numeric positional is not valid for Buffer scope: there is only one buffer."
- **Delimiter scope**: Rejected at parse time. "Numeric positional is not valid for Delimiter scope."
- **Count with `Replace` action**: Reject at parse time. `Replace` with find/replace args over N occurrences is ambiguous (does N mean N lines or N replacements?); defer to a future work item.
- **Count exceeds available targets**: Clamp and emit a warning, do not error. The user asked for "up to N"; fewer is acceptable.
- **Cursor not provided in CLI mode**: Emit a clear error from `execute_chord`: "numeric positional requires a cursor position; pass `cursor:\"line,col\"`."
- **`short_form()` and `long_form()` round-trip for Count**: `j5lw` → `ChordQuery` → `short_form()` must produce `j5lw`; `long_form()` must produce `Jump5LineWord`. Test both.
- **Unicode chord input**: The existing short-form parser iterates over UTF-8 char boundaries; a digit is ASCII and safe. No change needed.
- **`suggest_chord` stability**: The candidate generator iterates over letter-positional chars only. A mistyped digit chord (e.g., `j0lw`) should not produce a confusing suggestion; the error message should instead explain the 1–9 constraint.


## Test Considerations:

- **Parser unit tests** (`src/commands/chord_engine/parser.rs`):
  - `j5lw` parses to `(Jump, Count(5), Line, Word)` — spot-check
  - `j1ls` parses to `(Jump, Count(1), Line, Self_)`
  - `l9fd` parses to `(List, Count(9), Function, Definition)`
  - `c3ls` parses to `(Change, Count(3), Line, Self_)`
  - Digit `0` in positional position errors with a message explaining the 1–9 range
  - `j5bs` errors: Buffer scope with count rejected
  - `j5ds` errors: Delimiter scope with count rejected
  - Long form `Jump5LineWord` parses correctly and matches `j5lw`
  - Long form `List9FunctionDefinition` parses correctly and matches `l9fd`
  - Short-form round-trip: `short_form()` on a Count query re-emits the digit
  - Long-form round-trip: `long_form()` on a Count query emits `Jump5LineWord` style
  - `r5ls` (Replace with count) errors clearly
  - All existing short/long form tests remain unaffected (digit `1`–`9` cannot appear in existing valid 4-char chords because existing positional letters are not digits)

- **Resolver unit tests** (`src/commands/chord_engine/resolver.rs`):
  - `Count(3)` on Line + Self selects exactly 3 lines (mock buffer with ≥ 3 lines)
  - `Count(3)` on Line + Self with only 2 lines remaining: selects 2 lines, emits "only 2 of 3 requested occurrences found" warning
  - `Count(5)` on Line + Word: resolves to position of 5th word boundary from cursor
  - `Count(5)` on Line + Word with fewer than 5 words: clamps to end of line, emits warning
  - `Count(3)` on Function + Name with mock LSP data: returns 3 function symbols after cursor
  - `Count(3)` on Function + Definition: returns 3 function definition ranges after cursor
  - Cursor arg required for count in non-TUI context: resolver errors clearly when no cursor provided

- **Integration tests**:
  - Full pipeline: `c3ls(cursor:"2,0")` applied to a 10-line file changes lines 2–4
  - Full pipeline: `j5lw(cursor:"0,0")` on a known-word-count line resolves to correct column
  - CLI exec: `ane exec file.rs l5fd` prints at most 5 results with line/col, or all if fewer than 5 exist

- **Regression**: The exhaustive `all_valid_short_forms_parse_and_invalid_fail` test in `parser.rs` must still pass. Count positionals are not in the letter-based candidate matrix, so no existing test assertions change; the digits simply do not appear in that test's iteration.


## Codebase Integration:

- Follow established conventions, best practices, testing, and architecture patterns from the project's aspec.
- The `Positional::Count(u8)` variant lives in Layer 0 (`src/data/chord_types.rs`) alongside the other positional variants. Layer 0 also gains `is_valid_count_scope`.
- Parser changes are in Layer 1 (`src/commands/chord_engine/parser.rs`). The `from_short` and long-form parsing paths both need to handle the digit.
- Resolver changes are in Layer 1 (`src/commands/chord_engine/resolver.rs`). Treat `Count(n)` similarly to how `Next`/`Previous` are handled but loop N times or select an N-element span.
- Layer 2 (`src/frontend/tui/app.rs`) only needs exhaustiveness fixes on any `match positional` arms; no structural changes.
- `ChordQuery::short_form()` and `long_form()` require no changes as they delegate to `short()` / `Display` which are updated in Layer 0.
- `suggest_chord` in the parser should not include digit characters in its candidate alphabet; keep it letter-only to avoid misleading suggestions.
- All new validation functions (`is_valid_count_scope`) follow the same naming and placement pattern as `is_valid_combination`, `is_valid_jump_combination`, and `is_valid_list_positional`.
