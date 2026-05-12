# Work Item: Feature

Title: jump action, to positional, delimiter scope
Issue: issuelink

## Summary

Add `j/Jump` as a new action, `t/To` as a new positional, and `d/Delimiter` as a new scope to the chord grammar. Jump moves the cursor to the resolved chord location without modifying buffer content — it is only valid in interactive (TUI) frontends. To satisfy this constraint, both frontends acquire an `is_interactive` method. The CLI returns an error for any Jump chord. The `To` positional is also valid with all existing actions, behaving like `Until` with inclusive-endpoint semantics. The `Delimiter` scope resolves to the innermost matching delimiter pair surrounding the cursor — brackets, braces, square brackets, quotes, and backticks — using purely text-based scanning with no LSP requirement.

Valid example chords:
- `jtfc` / JumpToFunctionContents — jump to start of current function's body
- `jnfn` / JumpNextFunctionName — jump to the name of the next function after cursor
- `jpfn` / JumpPreviousFunctionName — jump to the name of the previous function
- `jtbe` / JumpToBufferEnd — jump to end of buffer
- `jofb` / JumpOutsideFunctionBeginning — jump to the line just before the function
- `jofe` / JumpOutsideFunctionEnd — jump to the line just after the function
- `ctfe` / ChangeToFunctionEnd — change from cursor position through function end
- `cids` / ChangeInsideDelimiterSelf — change the text between the innermost delimiter pair (exclusive of delimiters)
- `ceds` / ChangeEntireDelimiterSelf — change everything including the delimiters themselves
- `didc` / DeleteInsideDelimiterContents — delete text inside a brace-delimited block
- `jtdb` / JumpToDelimiterBeginning — jump to the opening delimiter

---

## User Stories

### User Story 1
As a: developer

I want to: type a Jump chord in chord mode (e.g. `jnfc`) and have the cursor land at the contents of the next function in the buffer

So I can: navigate code structure by symbol without lifting my hands from the keyboard or pressing arrow keys repeatedly

### User Story 2
As a: developer

I want to: use the `t` (To) positional with edit actions like `ctfe` (ChangeToFunctionEnd) to target the range from the cursor to a specific code landmark

So I can: express directional edits ("change from here to the end of this function") that aren't covered by Inside, Before, or Until

### User Story 3
As a: code agent or script

I want to: receive a clear error when I attempt to use a Jump chord via `ane exec`

So I can: understand that Jump is a cursor-navigation primitive and is not meaningful in a headless pipeline

### User Story 4
As a: developer

I want to: type a Delimiter chord like `cids` and have it find the innermost delimiter pair (braces, brackets, parens, quotes, backticks) around my cursor and operate on the text inside

So I can: quickly change, delete, or yank the contents of a delimited region without manually selecting it or knowing which specific delimiter type surrounds the cursor

---

## Implementation Details

### 1. Grammar additions — `src/data/chord_types.rs` (Layer 0)

**New `Action::Jump`** — short `"j"`, long `"Jump"`:

```rust
pub enum Action {
    // existing...
    Jump,
}
```

`"j"` is currently unused in the action set. Update `short()`, `from_short()`, and `Display` for the new variant. Update the `each_position_has_unique_short_letters` test in the same file to include `"j"` in the actions array.

**New `Positional::To`** — short `"t"`, long `"To"`:

```rust
pub enum Positional {
    // existing...
    To,
}
```

`"t"` is currently unused in the positional set. Update `short()`, `from_short()`, and `Display`. Update the uniqueness test to include `"t"` in the positionals array.

**New `Scope::Delimiter`** — short `"d"`, long `"Delimiter"`:

```rust
pub enum Scope {
    // existing...
    Delimiter,
}
```

`"d"` is currently unused in the scope set. Update `short()`, `from_short()`, `Display`, and the uniqueness test to include `"d"`. `Delimiter` does not require LSP — add it alongside `Line` and `Buffer` in the `requires_lsp` method:

```rust
pub fn requires_lsp(&self) -> bool {
    !matches!(self, Self::Line | Self::Buffer | Self::Delimiter)
}
```

**New method `Action::requires_interactive`** — pure data-layer fact, no imports from higher layers:

```rust
pub fn requires_interactive(&self) -> bool {
    matches!(self, Self::Jump)
}
```

**Updated `is_valid_combination`** — Jump does not modify text, so components that describe text content (`Value`, `Parameters`, `Arguments`) are invalid with Jump. Add a parallel function:

```rust
pub fn is_valid_jump_combination(positional: Positional, component: Component) -> bool {
    match positional {
        Positional::Outside => matches!(component, Component::Beginning | Component::End),
        _ => !matches!(component, Component::Value | Component::Parameters | Component::Arguments),
    }
}
```

The `Outside` constraint: "Jump outside a function" is spatially ambiguous (before or after?) unless `Beginning` or `End` pins the side. Any other component with Outside and Jump is rejected at parse time.

`To` positional: no new `is_valid_combination` constraints beyond those already enforced for the scope/component pair.

**Delimiter scope valid combinations** — update `is_valid_combination` to add Delimiter rules:

```rust
// Delimiter supports: Beginning, Contents, End, Self_, Name
// Delimiter does NOT support: Value, Parameters, Arguments
(Scope::Delimiter, Component::Beginning) => true,
(Scope::Delimiter, Component::Contents) => true,
(Scope::Delimiter, Component::End) => true,
(Scope::Delimiter, Component::Self_) => true,
(Scope::Delimiter, Component::Name) => true,
(Scope::Delimiter, Component::Value) => false,
(Scope::Delimiter, Component::Parameters) => false,
(Scope::Delimiter, Component::Arguments) => false,
```

The `Name` component for Delimiter resolves to the opening delimiter character itself (e.g., `{`, `(`, `[`, `"`), which can be useful for identifying which delimiter type is in play.

### 2. `FrontendCapabilities` trait — `src/commands/chord.rs` (Layer 1)

Define a trait in Layer 1 that Layer 2 implements. This allows `execute_chord` to query frontend properties without importing any Layer 2 type:

```rust
pub trait FrontendCapabilities {
    fn is_interactive(&self) -> bool;
}
```

Placing this trait in Layer 1 follows the dependency-inversion rule: lower layers define the interface they need; higher layers supply the implementation. Layer 1 never imports from Layer 2.

Update `execute_chord` to accept a `frontend: &dyn FrontendCapabilities` parameter and validate Jump before any buffer I/O, LSP startup, or resolve work. The current signature is `execute_chord(path: &Path, chord: &ChordQuery, lsp: &mut LspEngine) -> Result<ChordResult>`:

```rust
pub fn execute_chord(
    frontend: &dyn FrontendCapabilities,
    path: &Path,
    chord: &ChordQuery,
    lsp: &mut LspEngine,
) -> Result<ChordResult> {
    if chord.action.requires_interactive() && !frontend.is_interactive() {
        bail!("Jump action requires an interactive frontend; use ane in TUI mode");
    }
    // ... existing body unchanged
}
```

Add a private `HeadlessContext` struct in `chord.rs` for use by CLI callers that don't have a frontend:

```rust
struct HeadlessContext;
impl FrontendCapabilities for HeadlessContext {
    fn is_interactive(&self) -> bool { false }
}
```

All existing call sites (CLI `main.rs` exec path, tests) pass `&HeadlessContext` as the first argument. No convenience wrapper is needed — the callers already have an `LspEngine` in scope.

`CliFrontend` (`src/frontend/cli_frontend.rs`, Layer 2) implements `FrontendCapabilities`:

```rust
impl FrontendCapabilities for CliFrontend {
    fn is_interactive(&self) -> bool { false }
}
```

`TuiFrontend` (`src/frontend/tui/tui_frontend.rs`, Layer 2) implements `FrontendCapabilities`:

```rust
impl FrontendCapabilities for TuiFrontend {
    fn is_interactive(&self) -> bool { true }
}
```

`ApplyChordAction` in `src/frontend/traits.rs` (Layer 2) is unchanged — it handles chord result application, not capability reporting. Jump validation is complete before `apply` is ever called.

### 3. TUI cursor scroll — `src/frontend/tui/tui_frontend.rs` (Layer 2)

In `TuiFrontend::apply`, handle a Jump `ChordAction` (which carries `diff: None` and `cursor_destination: Some(...)`):

```rust
let line_count = state.current_buffer().map(|b| b.line_count()).unwrap_or(1);
state.cursor_line = dest.line.min(line_count.saturating_sub(1));
let line_len = state.current_buffer()
    .and_then(|b| b.lines.get(state.cursor_line))
    .map(|l| l.chars().count())
    .unwrap_or(0);
state.cursor_col = dest.col.min(line_len);
// bring cursor into viewport
if state.cursor_line < state.scroll_offset {
    state.scroll_offset = state.cursor_line;
} else if state.cursor_line >= state.scroll_offset + visible_height {
    state.scroll_offset = state.cursor_line.saturating_sub(visible_height.saturating_sub(1));
}
```

`visible_height` is derived from the last-known frame area stored in `EditorState` after each draw call. Note: `EditorState` does not have `buffer_line_count()` or `line_char_count()` methods — access the buffer via `state.current_buffer()` and use `Buffer::line_count()` and `String::chars().count()` on the line. The TUI path never calls `execute_chord` — it calls `ChordEngine::resolve` and `ChordEngine::patch` directly. Because TUI is always interactive, no Jump validation is needed in the TUI dispatch path.

### 4. Parser additions — `src/commands/chord_engine/parser.rs` (Layer 1)

- Map `"j"` / `"Jump"` → `Action::Jump` in the short- and long-form parsers.
- Map `"t"` / `"To"` → `Positional::To`.
- Map `"d"` / `"Delimiter"` → `Scope::Delimiter` (already handled by `Scope::from_short` and `Scope::from` updates in Layer 0; parser uses those methods).
- After parsing all four parts, if `action == Jump`:
  - Call `is_valid_jump_combination(positional, component)`. On failure, emit a parse error:
    `"Jump with Outside positional requires Beginning or End component to specify direction"` (Outside case) or `"Jump does not operate on Value, Parameters, or Arguments components"` (other cases).
  - If `args.value.is_some()`, emit a parse error: `"Jump does not accept a value argument"`.
- `Jump` chords do not require the `(...)` argument list and should parse cleanly from bare 4-char short form (e.g. `jtfc`) as well as long form (e.g. `JumpToFunctionContents`).

The `try_auto_submit_short` function in `mod.rs` already handles 4-char lowercase inputs — Jump and Delimiter chords auto-submit exactly like other actions without any changes to that function.

### 5. Resolver — `src/commands/chord_engine/resolver.rs` (Layer 1)

**`Positional::To` in `apply_positional`**: add a new arm that mirrors `Until` but uses the _end_ of the component range as the endpoint rather than the start, making To inclusive:

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

`Until` ends at `component_range.start` (stops before the target); `To` ends at `component_range.end` (runs through the target). For single-character or point-range components (e.g. line beginning) the two produce identical results.

**`Action::Jump` in `resolve_cursor_and_mode`**: add:

```rust
Action::Jump => {
    let cursor = CursorPosition {
        line: target_range.start_line,
        col: target_range.start_col,
    };
    (Some(cursor), Some(EditorMode::Edit))
}
```

Jump always transitions to Edit mode after landing so the user can immediately start editing. `target_range` is whatever `apply_positional` returns for the chord's positional+scope+component combination — the same resolution path used by all other actions.

**`Positional::Outside` + `Action::Jump`**: `outside_ranges` returns two ranges (before and after the scope). For Jump, only one is needed: the Beginning component maps to the head range (before the scope start) and End maps to the tail range (after the scope end). Add a post-`apply_positional` step in `resolve_buffer` that, when `action == Jump && positional == Outside`, selects the appropriate single range from the two returned by `outside_ranges` based on `component`.

**`Scope::Delimiter` in `resolve_scope`**: add a new arm that calls `resolve_delimiter_scope`:

```rust
Scope::Delimiter => resolve_delimiter_scope(query, buffer, buffer_name),
```

**New function `resolve_delimiter_scope`**: purely text-based, no LSP. Takes the cursor position and scans the buffer to find the innermost matching delimiter pair:

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

**New function `find_innermost_delimiter`**: the core scanning algorithm. Supported delimiter pairs:

| Open  | Close | Type |
|-------|-------|------|
| `(`   | `)`   | Symmetric paired |
| `{`   | `}`   | Symmetric paired |
| `[`   | `]`   | Symmetric paired |
| `"`   | `"`   | Self-paired |
| `'`   | `'`   | Self-paired |
| `` ` `` | `` ` `` | Self-paired |

Algorithm for paired delimiters (`()`, `{}`, `[]`):
1. From the cursor position, scan backward through the buffer text, tracking a nesting depth counter for each delimiter type.
2. When a closing delimiter is encountered (scanning backward), increment its depth. When an opening delimiter is encountered, decrement. When depth reaches -1, that opening delimiter is a candidate.
3. Record the position of each candidate opening delimiter found.
4. For each candidate opening delimiter, scan forward from after it to find the matching closing delimiter at depth 0.
5. The innermost pair is the candidate whose opening delimiter is closest to the cursor (i.e., found first in the backward scan) and whose closing delimiter is at or after the cursor.

Algorithm for self-paired delimiters (`"`, `'`, `` ` ``):
1. Walk forward from the beginning of the buffer to the cursor, counting occurrences of each self-paired delimiter (ignoring escaped ones like `\"`, `\'`, `` \` ``).
2. If the count is odd, the cursor is inside that delimiter pair. Scan backward from the cursor to find the opening instance, and forward from the cursor to find the closing instance.
3. If the count is even, the cursor is outside any pair of that delimiter type — skip it.

After collecting all candidates (paired and self-paired), select the one with the tightest span (smallest `end - start` range) that encloses the cursor. The resulting `TextRange` covers the full delimiter pair inclusive of the delimiters themselves.

If no enclosing delimiter is found, return:
```rust
Err(ChordError::resolve(buffer_name, "no enclosing delimiter found at cursor position").into())
```

**Delimiter component resolution**: the existing `resolve_component` function handles Delimiter scope naturally through existing component logic:
- `Component::Self_`: returns the full scope range (delimiters included).
- `Component::Beginning`: returns a point range at the opening delimiter position.
- `Component::End`: returns a point range at the closing delimiter position.
- `Component::Contents`: uses the existing `resolve_contents_component` logic. For Delimiter scope, this needs a new branch that returns the range between the delimiters (exclusive). The opening delimiter is at `scope_range.start_line, scope_range.start_col` and the closing delimiter is at `scope_range.end_line, scope_range.end_col`. Contents is everything between:
  ```rust
  Scope::Delimiter => {
      Ok(TextRange {
          start_line: scope_range.start_line,
          start_col: scope_range.start_col + 1,
          end_line: scope_range.end_line,
          end_col: scope_range.end_col,
      })
  }
  ```
  Note: `start_col + 1` works because the opening delimiter is always a single character. The end_col already points to the closing delimiter character, so `end_col` (exclusive end) gives us everything before it.
- `Component::Name`: returns a single-character range covering the opening delimiter character. This lets a user identify or change which delimiter type is in use.

**Positional behavior with Delimiter scope**: all existing positionals work with the resolved Delimiter scope range:
- `Inside` + `Contents`: the text strictly between the delimiters.
- `Entire` + `Self_`: the full span including delimiters.
- `Inside` + `Self_`: shrinks inward by stripping the delimiter characters — equivalent to `Contents`.
- `Before`/`After`/`Until`/`To`: work relative to the delimiter scope boundaries, same as any other scope.
- `Next`/`Previous`: not meaningful for Delimiter scope (there is no LSP symbol list to navigate). Return an error: `"Next/Previous positional is not valid for Delimiter scope"`.

### 6. Patcher — `src/commands/chord_engine/patcher.rs` (Layer 1)

Add a Jump arm that emits no diff:

```rust
Action::Jump => ChordAction {
    buffer_name: buffer_name.to_string(),
    diff: None,
    yanked_content: None,
    cursor_destination: resolution.cursor_destination,
    mode_after: resolution.mode_after,
    highlight_ranges: vec![resolution.component_range],
    warnings: vec![],
},
```

`highlight_ranges` is set to the component range so the TUI can briefly flash the landing zone.

No patcher changes are needed for Delimiter scope — the patcher operates on resolved ranges and is scope-agnostic.


---

## Edge Case Considerations

### Jump edge cases

- **Jump on CLI**: `execute_chord` checks `chord.action.requires_interactive() && !frontend.is_interactive()` before any buffer I/O or LSP startup. It bails immediately with `"Jump action requires an interactive frontend; use ane in TUI mode"`. The error reaches stderr via `main.rs`'s error handler; stdout stays clean.

- **Jump+Outside with invalid component**: `jofv` (JumpOutsideFunctionValue) is rejected at parse time with a clear message. Only `jofb` and `jofe` (and equivalents for other scopes) pass validation.

- **Jump with value argument**: `jtfc("my_fn")` is a parse error — Jump takes no arguments. This is caught in the parser before the resolver runs.

- **`jnfn` at last function**: No next function exists. The resolver's `find_neighbor_symbol` returns `None`; emit `ChordError::resolve` with `"no next function found from cursor position (line, col)"`. Same pattern mirrors the existing error for `Next`/`Previous` on LSP scopes.

- **`jpfn` at first function**: Symmetric error: `"no previous function found from cursor position"`.

- **`jtbe` / JumpToBufferEnd**: Buffer scope has no LSP requirement. The chord parses successfully. When invoked via `ane exec`, `execute_chord` rejects it immediately through the `FrontendCapabilities` check — before any file I/O occurs — because `Action::Jump.requires_interactive()` is `true` and `HeadlessContext::is_interactive()` is `false`.

### To positional edge cases

- **To vs Until for point-range components**: `Component::Beginning` and `Component::Name` resolve to point ranges (`start == end`). In this case `To` and `Until` produce identical `target_ranges` (cursor to that point). The distinction only manifests for ranged components like `Contents` or `End`.

- **`ChangeToFunctionEnd` vs `ChangeUntilFunctionEnd`**: `Until` ends at `component_range.start_col` (just before the `}`); `To` ends at `component_range.end_col` (after the `}`). For `End` component, which is itself a point range, start == end, so the two are identical — the `}` is included in both. The meaningful difference emerges for `Contents`: `ChangeUntilFunctionContents` stops at the opening `{`, while `ChangeToFunctionContents` runs through the closing `}`.

### Delimiter scope edge cases

- **Cursor on a delimiter character**: if the cursor is sitting directly on an opening delimiter like `{`, the scope resolves to the pair that starts at that character. If on a closing delimiter like `}`, the scope resolves to the pair that ends at that character. This is determined naturally by the scanning algorithm: the backward scan from `}` finds its matching `{`.

- **Nested delimiters**: `fn foo() { if bar { x } }` with cursor on `x` — the innermost pair is the inner `{ x }`, not the outer `{ if bar { x } }`. The backward-scan algorithm finds the closest opening delimiter first, which is the inner `{`.

- **Mixed delimiter types**: `foo({ "bar" })` with cursor on `bar` — the innermost delimiter is `"bar"`, not `{ "bar" }` or `({ "bar" })`. The algorithm collects all candidate pairs and selects the tightest span.

- **Unmatched delimiters**: `let s = "hello;` with cursor on `hello` — if the closing `"` is missing, the forward scan reaches end-of-buffer without finding a match. This candidate is discarded. If no other delimiter pair encloses the cursor, the error `"no enclosing delimiter found at cursor position"` is returned.

- **Escaped delimiters**: `"he said \"hi\""` — escaped delimiters (`\"`) are skipped during self-paired delimiter counting. The backslash-preceded delimiter does not count as an opening or closing instance. This applies to `\"`, `\'`, and `` \` ``.

- **String contents containing braces**: `let s = "{ not a block }";` — if the cursor is inside the string, the `"` pair is the innermost delimiter. The braces inside the string are also valid candidates, but the `"` pair has a tighter span and wins. If the cursor is on the `{` inside the string, the `{`...`}` pair is tighter than the `"` pair and is selected — this is intentional, as the user explicitly positioned their cursor there.

- **Empty delimiter pair**: `()` with cursor between the parens — the scope resolves to the `()` pair. `Contents` produces a zero-width range. A `Change` on a zero-width range inserts text at that position (existing patcher behavior for zero-width ranges).

- **Next/Previous with Delimiter scope**: `cndX` and `cpdX` are parse errors — `Next` and `Previous` positionals are not valid for Delimiter scope because there is no symbol list to navigate. The error message: `"Next/Previous positional is not valid for Delimiter scope"`.

- **Delimiter scope does not require LSP**: `Scope::Delimiter.requires_lsp()` returns `false`. Delimiter chords work immediately on any file without waiting for a language server, same as Line and Buffer scopes.

- **Multi-line delimiters**: the algorithm works across line boundaries. For a multi-line block `{ \n  code \n }`, the scope range spans from the `{` on line N to the `}` on line M. The Contents range spans from after `{` to before `}`, potentially covering multiple lines.

### General edge cases

- **Cursor scroll when destination is far off-screen**: `scroll_offset` must be clamped after update. If `cursor_line` is 0, `scroll_offset` becomes 0. If the buffer is shorter than the viewport, `scroll_offset` stays 0 regardless.

- **Jump in Edit mode**: Jump chords are only entered in Chord mode; the key handler already gates chord dispatch to `Mode::Chord`. No additional guard is needed, but the error path should be clear if the chord is somehow dispatched outside Chord mode.

- **`jtfn` notation clarification**: In the 4-part grammar `jtfn` = Jump + To + Function + Name (jump cursor to the identifier of the current function at the cursor). The "next function" use-case is expressed as `jnfn` (Jump + Next + Function + Name). The summary's label "JumpToFunctionNext" is informal — the formal chord for next-function navigation is `jnfn`.

- **Exhaustive match coverage**: Adding `Action::Jump`, `Positional::To`, and `Scope::Delimiter` will produce compiler errors in every exhaustive `match` on these enums. The resolver's `resolve_cursor_and_mode`, the patcher's action dispatch, `apply_positional`, `resolve_scope`, `resolve_component`, and any display/test code all need new arms. Treat compiler errors as the integration checklist.

---

## Test Considerations

### Jump tests

- **Unit: round-trip for new variants** — `Action::from_short("j") == Some(Action::Jump)` and back; `Positional::from_short("t") == Some(Positional::To)` and back; `Scope::from_short("d") == Some(Scope::Delimiter)` and back. The existing `each_position_has_unique_short_letters` test must include `"j"`, `"t"`, and `"d"`.

- **Unit: `requires_interactive`** — `Action::Jump.requires_interactive() == true`; all other existing actions return `false`.

- **Unit: `is_valid_jump_combination`** — assert `jofb` and `jofe` pass; assert `jofv`, `jofc`, `jofa`, `jofp`, `jofn` fail. Assert valid non-Outside combinations (e.g. `jtfc`, `jnfn`, `jifc`) pass. Assert Jump+Value/Parameters/Arguments fail regardless of positional.

- **Unit: parser rejects Jump+Outside+invalid component** — `parse("jofv")` returns `Err` containing the direction-hint message.

- **Unit: parser rejects Jump with value** — `parse("jtfc(my_fn, \"text\")")` returns `Err`.

- **Unit: parser accepts bare Jump short form** — `parse("jtfc")` returns `Ok` (no args required).

- **Unit: `FrontendCapabilities` implementations** — assert `CliFrontend::is_interactive()` returns `false`; assert `TuiFrontend::is_interactive()` returns `true`. These tests live in the respective Layer 2 frontend files.

- **Unit: `execute_chord` rejects Jump before file I/O** — call `execute_chord(&HeadlessContext, path_that_does_not_exist, jump_chord, &mut lsp)` and assert the error message is the interactive-frontend message, not "file not found". This confirms the check fires before any I/O, validating the early-exit ordering within `execute_chord`.

### To positional tests

- **Unit: `Positional::To` in resolver produces inclusive range** — given a buffer with a known function, a `Change+To+Function+Contents` chord resolves to a range whose endpoint is `component_range.end` rather than `component_range.start`.

- **Unit: `Positional::Until` vs `Positional::To` for Contents** — verify the two positionals produce `end_col` values differing by the width of the function body, confirming the inclusive/exclusive distinction.

### Delimiter scope tests

- **Unit: `Scope::Delimiter.requires_lsp()` returns `false`**.

- **Unit: `is_valid_combination` for Delimiter** — assert `(Delimiter, Beginning)`, `(Delimiter, Contents)`, `(Delimiter, End)`, `(Delimiter, Self_)`, `(Delimiter, Name)` are valid. Assert `(Delimiter, Value)`, `(Delimiter, Parameters)`, `(Delimiter, Arguments)` are invalid.

- **Unit: `find_innermost_delimiter` with parentheses** — buffer `"foo(bar, baz)"`, cursor on `bar` → scope range covers `(bar, baz)` from the `(` to the `)`.

- **Unit: `find_innermost_delimiter` with braces** — buffer `"if true { x + 1 }"`, cursor on `x` → scope range covers `{ x + 1 }`.

- **Unit: `find_innermost_delimiter` with quotes** — buffer `'let s = "hello";'`, cursor on `hello` → scope range covers `"hello"`.

- **Unit: `find_innermost_delimiter` nested** — buffer `"foo({ bar })"`, cursor on `bar` → scope range covers `{ bar }` (innermost), not `({ bar })`.

- **Unit: `find_innermost_delimiter` empty pair** — buffer `"f()"`, cursor between parens → scope range covers `()`.

- **Unit: `find_innermost_delimiter` no delimiter** — buffer `"just text"`, cursor on `text` → returns error.

- **Unit: `find_innermost_delimiter` escaped quote** — buffer `'"he said \\"hi\\""'`, cursor between escaped quotes → correctly identifies the outer `"` pair.

- **Unit: `find_innermost_delimiter` multi-line braces** — buffer `["fn f() {", "    x", "}"]`, cursor on `x` → scope range from `{` on line 0 to `}` on line 2.

- **Unit: Delimiter Contents component** — resolve `cids` (ChangeInsideDelimiterSelf) with cursor inside `{content}` → component range excludes the `{` and `}`.

- **Unit: Delimiter Self component** — resolve `ceds` (ChangeEntireDelimiterSelf) → component range includes the delimiters.

- **Unit: Next/Previous with Delimiter errors** — `parse("cnds")` returns error about Next/Previous not valid for Delimiter scope.

### Jump + Delimiter combined tests

- **Unit: Jump resolver produces no diff, only cursor_destination** — resolve and patch a `jtfc` chord against a buffer with a known function; assert `ChordAction.diff == None` and `cursor_destination` points to the first line of the function body.

- **Unit: Jump+Next resolver** — buffer with two functions, cursor in first; `jnfn` resolves `cursor_destination` to the second function's name start position.

- **Unit: Jump+Previous resolver** — cursor in second function; `jpfn` resolves to first function's name position.

- **Unit: Jump+Outside+Beginning** — `jofb` resolves `cursor_destination` to the line immediately before the function start (the head range from `outside_ranges`).

- **Unit: Jump+Outside+End** — `jofe` resolves to the line immediately after the function end (the tail range from `outside_ranges`).

- **Unit: `jtdb` (JumpToDelimiterBeginning)** — cursor inside `{ block }`, chord resolves cursor_destination to the position of `{`.

- **Integration: TUI cursor update from Jump** — simulate a Jump chord dispatch against `EditorState` with a multi-function buffer; assert `state.cursor_line`, `state.cursor_col`, and `state.scroll_offset` are updated correctly. Specifically: if the destination is outside the initial viewport, `scroll_offset` adjusts so the cursor is visible.

- **Manual test checklist**:
  - Open a multi-function Rust file in TUI; in chord mode type `jnfc` — cursor jumps to the opening of the next function's body
  - Type `jpfn` — cursor jumps back to the previous function's name
  - Type `jtfe` — cursor jumps to the closing `}` of the current function
  - Type `jofb` — cursor jumps to the line just before the current function
  - Type `jofe` — cursor jumps to the line just after the current function
  - Type `jofv` — chord box turns red with the direction-hint parse error
  - Type `ctfe` — text from cursor through function end is deleted and edit mode entered
  - Type `ctlb` — text from cursor back to line beginning is deleted (ChangeToLineBeginning)
  - Type `cids` inside `{ block }` — text between braces is cleared, delimiters remain
  - Type `ceds` inside `{ block }` — entire `{ block }` including braces is cleared
  - Type `cids` inside `"string"` — text between quotes is cleared, quotes remain
  - Type `didc` inside nested `foo({ bar })` — only inner `{ bar }` contents are deleted
  - Type `jtdb` inside a block — cursor jumps to the `{`
  - Run `ane exec file.rs jtfc` — stderr shows "Jump action is only valid in interactive (TUI) mode", exit code 1
  - Jump to a function 200 lines away — `scroll_offset` updates so the cursor is visible, editor does not leave cursor off-screen

---

## Codebase Integration

- **Layer 0 changes** (`src/data/chord_types.rs`): Add `Action::Jump`, `Positional::To`, and `Scope::Delimiter`. Add `Action::requires_interactive()`. Add `is_valid_jump_combination()`. Update `is_valid_combination()` with Delimiter rules. Update `requires_lsp()` to return `false` for Delimiter. These are pure enum/function additions with no imports from higher layers.

- **Layer 1 changes**:
  - `src/commands/chord.rs`: Add `pub trait FrontendCapabilities { fn is_interactive(&self) -> bool; }`. Add private `HeadlessContext` implementing the trait with `is_interactive() -> false`. Update `execute_chord` signature to take `frontend: &dyn FrontendCapabilities` as the new first parameter (before `path`), keeping the existing `lsp: &mut LspEngine` parameter. Add the early Jump validation check. Update all call sites (CLI exec path, tests) to pass `&HeadlessContext`.
  - `src/commands/chord_engine/parser.rs`: Add `"j"/"Jump"`, `"t"/"To"`, and `"d"/"Delimiter"` parsing. Add Jump-specific post-parse validation. Delimiter parsing is handled by the existing `Scope::from_short`/`from` plumbing updated in Layer 0.
  - `src/commands/chord_engine/resolver.rs`: Add `Positional::To` arm in `apply_positional` (mirrors `Until` with inclusive endpoint). Add `Action::Jump` arm in `resolve_cursor_and_mode`. Add Outside+Jump range-selection logic in `resolve_buffer`. Add `Scope::Delimiter` arm in `resolve_scope` calling new `resolve_delimiter_scope`. Add `find_innermost_delimiter` function implementing the delimiter scanning algorithm. Add Delimiter branch in `resolve_contents_component` for Contents component. Reject `Next`/`Previous` positional for Delimiter scope in `apply_positional`.
  - `src/commands/chord_engine/patcher.rs`: Add `Action::Jump` arm that emits no diff. No `requires_interactive` field on `ChordAction` — the check is complete before the patcher runs. No patcher changes for Delimiter scope — the patcher is scope-agnostic.

- **Layer 2 changes**:
  - `src/frontend/cli_frontend.rs`: Implement `commands::chord::FrontendCapabilities` (returns `false`). No change to `apply` — Jump is rejected before `apply` is ever called.
  - `src/frontend/tui/tui_frontend.rs`: Implement `commands::chord::FrontendCapabilities` (returns `true`). Add Jump handling in `apply` — update `cursor_line`, `cursor_col`, and `scroll_offset` from `cursor_destination`.
  - `src/frontend/traits.rs`: No change — `ApplyChordAction` does not gain `is_interactive`.

- **Compiler-driven integration**: Adding `Action::Jump`, `Positional::To`, and `Scope::Delimiter` to the enums will produce non-exhaustive `match` errors throughout the codebase. Use these as the integration checklist: `resolve_cursor_and_mode`, `apply_positional`, `resolve_scope`, `resolve_component`, `resolve_contents_component`, the patcher's action dispatch, `Display` impls, and all tests that enumerate variants. Resolve each error site before moving to the next.

- **`is_valid_combination` interaction**: The existing `is_valid_combination(scope, component)` function governs scope/component pairs and is updated with Delimiter rules. `is_valid_jump_combination(positional, component)` is a separate, additive validation called only when `action == Jump`. The parser calls both checks independently.

- **Auto-submit compatibility**: `try_auto_submit_short` already accepts any 4-char lowercase chord — Jump and Delimiter chords auto-submit without modification to that function. A 4-char chord like `jtfc`, `cids`, or `ceds` will trigger auto-submit on the 4th keypress exactly like `cifn`.

- **Struct scope covers enums**: `Scope::Struct` maps to `[SymbolKind::Struct, SymbolKind::Enum]` in `resolve_lsp_scope`, so Jump chords targeting Struct scope (e.g. `jtsb`) resolve against both struct and enum symbols. Delimiter scope does not use LSP and has its own resolution path.

- **Delimiter scope independence**: unlike Function, Variable, Struct, and Member scopes, Delimiter never calls into `resolve_lsp_scope` or `resolve_variable_scope_via_selection_range`. It is fully self-contained in `resolve_delimiter_scope` and `find_innermost_delimiter`, operating only on the buffer text. This makes it available immediately on file open, before any LSP server starts.
