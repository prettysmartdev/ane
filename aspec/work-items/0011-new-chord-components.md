# Work Item: Feature

Title: new chord components
Issue: issuelink

## Summary

Add five new chord grammar elements: `l/List` action, `w/Word` component, `d/Definition` component, `l/Last` positional, and `f/First` positional.

**`l/List` action** generates a list of matching items rather than modifying the buffer. In CLI, each item and its line number is printed to stdout. In TUI, a scrollable dialog appears; the user navigates with arrow keys and presses Enter to jump the cursor to the selected item. List is an exploratory action for discovering what exists in a file. When action is List, the positional changes meaning: instead of narrowing a range, it acts as a positional **filter** on the collected items — e.g. `lafn` lists only function names that appear *after* the cursor, and `lefn` lists all of them.

**`w/Word` component** targets a whitespace-delimited word in a line or buffer, analogous to vim's `w` target. It is text-based with no LSP requirement.

**`d/Definition` component** selects the entire definition signature of a scope, excluding its body. A definition is everything that declares the item's identity and type contract: for a variable, the keyword + name + type annotation (`let somename: int64`); for a function, the visibility + keyword + name + parameters + return type (`pub fn some_func(some_param: int) -> returnval`); for a struct/enum, the visibility + keyword + name + generic parameters (`pub struct Foo<T>`). The body (brace-delimited contents) is never included. Useful with `l/List` — e.g. `lefd` lists all function definitions in the buffer — giving a quick structural overview without reading implementation details.

**`l/Last` positional** selects the last occurrence of the component within the scope — e.g. `jllw` jumps to the last word in the current line, `jlfn` jumps to the last function's name in the buffer.

**`f/First` positional** is the inverse of Last, selecting the first occurrence — e.g. `jflw` jumps to the first word in the line.

Example chords:
- `lefn` / `ListEntireFunctionName` — list all function names in the buffer
- `lisn` / `ListInsideStructName` — list struct names within the scope at cursor
- `lemn` / `ListEntireMemberName` — list all member names of the enclosing struct or enum
- `lafn` / `ListAfterFunctionName` — list function names that appear after the cursor
- `lefd` / `ListEntireFunctionDefinition` — list all function definitions (signatures) in the buffer
- `cefd` / `ChangeEntireFunctionDefinition` — change a function's entire signature
- `yevd` / `YankEntireVariableDefinition` — yank a variable's declaration (keyword + name + type)
- `desd` / `DeleteEntireStructDefinition` — delete a struct's declaration line without its body
- `celw` / `ChangeEntireLineWord` — change the word the cursor is on
- `jnlw` / `JumpNextLineWord` — jump to the next word in the current line
- `jllw` / `JumpLastLineWord` — jump to the last word in the line
- `jlfn` / `JumpLastFunctionName` — jump to the last function's name in the buffer
- `jflw` / `JumpFirstLineWord` — jump to the first word in the line

---

## User Stories

### User Story 1
As a: developer

I want to: type a List chord such as `lefn` in chord mode and see a scrollable overlay of every function name in the current file with its line number, then press Enter to jump to the one I want

So I can: explore the structure of an unfamiliar file and navigate to any function in a single chord without reading through the buffer manually

### User Story 2
As a: developer

I want to: type `lefd` in chord mode and see a scrollable overlay showing the full signature of every function in the file — e.g. `pub fn process(input: &str) -> Result<Output>` — without their bodies

So I can: quickly scan a file's API surface and understand the type contracts at a glance, which is faster than scrolling through implementations

### User Story 3
As a: developer

I want to: use the `w/Word` component with Jump and Change actions — e.g. `jnlw` to jump to the next word, `celw` to change the word under the cursor — and use the `f/First` and `l/Last` positionals to reach the first or last word in the line instantly

So I can: perform word-level navigation and editing in the TUI without lifting my hands from the keyboard or pressing arrow keys repeatedly

### User Story 4
As a: code agent

I want to: run `ane exec file.rs lefn` and receive a line-separated list of function names with their line numbers on stdout

So I can: enumerate the symbols in a file and use the output as input for subsequent targeted chords, without parsing the source myself

---

## Implementation Details

### 1. Grammar additions — `src/data/chord_types.rs` (Layer 0)

**New `Action::List`** — short `"l"`, long `"List"`:

```rust
pub enum Action {
    // existing...
    List,
}
```

`"l"` is unused in the current action set (`c r d y a p i j`). Update `short()`, `from_short()`, and `Display`. `List` does not require an interactive frontend — it is valid in both CLI and TUI. Do not add it to `requires_interactive`. Update `each_position_has_unique_short_letters` to include `"l"` in the actions array.

**New `Component::Word`** — short `"w"`, long `"Word"`:

```rust
pub enum Component {
    // existing...
    Word,
}
```

`"w"` is unused in the current component set (`b c e v p a n s`). Update `short()`, `from_short()`, and `Display`. Update `each_position_has_unique_short_letters` to include `"w"` in the components array.

**New `Component::Definition`** — short `"d"`, long `"Definition"`:

```rust
pub enum Component {
    // existing...
    Definition,
}
```

`"d"` is unused in the current component set (`b c e v p a n s w`). Update `short()`, `from_short()`, and `Display`. Update `each_position_has_unique_short_letters` to include `"d"` in the components array.

Update `is_valid_combination` to include Definition rules. Definition requires LSP and is meaningful for scopes that have declarations:

```rust
(Scope::Function | Scope::Variable | Scope::Struct, Component::Definition) => true,
(_, Component::Definition) => false,
```

Definition is not meaningful for Line, Buffer, Delimiter, or Member scopes. A function's definition is its signature; a variable's definition is its declaration; a struct's definition is its head line (the Struct scope covers both structs and enums).

Update `is_valid_combination` to include Word rules. Word is text-based and only meaningful for Line and Buffer scopes:

```rust
(Scope::Line | Scope::Buffer, Component::Word) => true,
(_, Component::Word) => false,
```

Add this before the catch-all arms. Word does not require LSP; the scope-level `requires_lsp()` already handles this correctly since Line and Buffer return `false`.

**New `Positional::Last`** — short `"l"`, long `"Last"`:

```rust
pub enum Positional {
    // existing...
    Last,
}
```

`"l"` is unused in the current positional set (`i u a b n p e o t`). Update `short()`, `from_short()`, and `Display`. Update the uniqueness test to include `"l"` in the positionals array.

**New `Positional::First`** — short `"f"`, long `"First"`:

```rust
pub enum Positional {
    // existing...
    First,
}
```

`"f"` is unused in the current positional set. Update `short()`, `from_short()`, `Display`, and the uniqueness test to include `"f"`.

Note: the short letter `l` now appears in the action set (List), the positional set (Last), and the scope set (Line). The short letter `f` appears in the positional set (First) and the scope set (Function). This is consistent with the existing design where `p` appears as action (Prepend), positional (Previous), and component (Parameters), and `a` appears as action (Append), positional (After), and component (Arguments). Uniqueness is enforced per-position only.

**New function `is_valid_list_positional`**: List reinterprets positional as a filter. All positionals are structurally valid with List except `Outside`, which has no clear list-filtering semantics. Add:

```rust
pub fn is_valid_list_positional(positional: Positional) -> bool {
    !matches!(positional, Positional::Outside)
}
```

### 2. New `ListItem` type and `ChordAction` extension — `src/commands/chord_engine/types.rs` (Layer 1)

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub val: String, // the full value that was selected (function/var name, full definition, etc.)
    pub line: usize,
    pub col: usize,
}
```

Add a field to `ChordAction`:

```rust
pub listed_items: Vec<ListItem>,
```

For non-List actions, `listed_items` is always empty. For `Action::List`, `diff` is always `None` and `yanked_content` is always `None`.

### 3. Parser additions — `src/commands/chord_engine/parser.rs` (Layer 1)

- Map `"l"` / `"List"` → `Action::List` in short- and long-form parsers.
- Map `"w"` / `"Word"` → `Component::Word`.
- Map `"d"` / `"Definition"` → `Component::Definition`.
- Map `"l"` / `"Last"` → `Positional::Last`.
- Map `"f"` / `"First"` → `Positional::First`.

After parsing all four parts, if `action == List`:
- Call `is_valid_list_positional(positional)`. On failure, emit: `"List action does not support the Outside positional"`.
- If `args.value.is_some()`, emit: `"List action does not accept a value argument"`.
- If `args.find.is_some() || args.replace.is_some()`, emit: `"List action does not accept find/replace arguments"`.

List chords do not require a value argument and parse cleanly from bare 4-char short form (`lefn`) as well as long form (`ListEntireFunctionName`). The `try_auto_submit_short` function already handles all 4-char lowercase chords without modification.

### 4. Resolver — `src/commands/chord_engine/resolver.rs` (Layer 1)

#### `Action::List` resolution path

Add a branch in `resolve_buffer` (or the equivalent dispatch point) that detects `action == List` and calls a new `resolve_list` function instead of the standard range-resolution path.

**New function `resolve_list`**: collects all matching symbols, applies positional as a filter, returns `Vec<ListItem>`:

```rust
fn resolve_list(
    query: &ChordQuery,
    buffer: &Buffer,
    lsp: &mut LspEngine,
    buffer_name: &str,
) -> Result<Vec<ListItem>>
```

Resolution steps:

1. **Collect candidates**: gather all LSP symbols matching the scope kind (for LSP scopes), or all words in the buffer (for Buffer scope + Word component), or all words in the target line (for Line scope + Word component). For LSP scopes the full symbol tree is requested via `document_symbols()`. Filter by component to determine what property to surface:
   - `Name`: use each symbol's `selectionRange` start position and name string.
   - `Definition`: use each symbol's range start position and the full definition text (from scope start to body opener, as resolved by `resolve_definition_component`).
   - `End`: use each symbol's range end position and name string.
   - Other components with LSP scopes: unsupported for List — emit a parse error during parsing if component is not Name, Definition, End, or Self_ when action is List with an LSP scope.

2. **Apply positional as filter**: given the collected `Vec<ListItem>` and the cursor position (from `args.cursor_pos` when required):
   - `Entire` → return all items (no filtering)
   - `After` → keep only items where `item.line > cursor.line || (item.line == cursor.line && item.col > cursor.col)`. Requires cursor.
   - `Before` → keep only items before cursor. Requires cursor.
   - `Inside` → keep only items whose position falls within the innermost LSP symbol at cursor (find the innermost containing symbol, then filter). Requires cursor.
   - `Next` → keep only the single item immediately after cursor (smallest line/col greater than cursor). Requires cursor.
   - `Previous` → keep only the single item immediately before cursor. Requires cursor.
   - `Last` → keep only the last item (greatest line/col).
   - `First` → keep only the first item (smallest line/col).
   - `Until` / `To` → keep items between start-of-scope and cursor (`Until`) or cursor and end-of-scope (`To`). Requires cursor.

3. Return the filtered `Vec<ListItem>`, which the patcher stores in `ChordAction.listed_items`.

#### `Component::Word` resolution

Add a new `resolve_word_component` function for text-based word resolution:

```rust
fn resolve_word_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange>
```

A word is a maximal run of non-whitespace characters (`!char.is_ascii_whitespace()`). The algorithm:

1. For `Positional::Entire`: find the word that contains or is nearest to the cursor column. Scan left from the cursor to find the word start, scan right to find the word end.
2. For `Positional::Next`: starting from one past the current word's end, scan forward to find the start of the next run of non-whitespace characters.
3. For `Positional::Previous`: scan backward from one before the current word's start to find the end of the previous word, then scan left to find its start.
4. For `Positional::First`: find the first non-whitespace run in the scope range.
5. For `Positional::Last`: find the last non-whitespace run in the scope range.
6. For `Positional::Inside` / `Positional::Entire`: equivalent for Word (a word has no inner delimiters).

If the cursor is on whitespace and no word is adjacent in the requested direction, return an error: `"no word found at cursor position"`.

#### `Component::Definition` resolution

Add a new `resolve_definition_component` function:

```rust
fn resolve_definition_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    lsp: &mut LspEngine,
    buffer_name: &str,
) -> Result<TextRange>
```

The Definition component returns the range from the start of the scope to the start of the body (exclusive), trimming trailing whitespace. The algorithm:

1. **Find the scope start**: use `scope_range.start` — this is the beginning of the full declaration (includes visibility modifiers, keywords, etc.).

2. **Find the body start**: locate the opening brace `{` of the scope's body using `find_brace_range()`. The definition ends immediately before the opening brace (after trimming trailing whitespace before the brace).

3. **Handle scopes without brace bodies**:
   - **Variable**: the definition is from scope start up to (but not including) the `=` sign. For variables without assignment (e.g. `let x: i32;`), the definition is the entire scope.
   - **Function**: from scope start to the opening `{` of the function body. For function declarations without bodies (trait methods, extern fns), the definition is the entire scope up to the `;`.
   - **Struct** (covers both structs and enums): from scope start to the opening `{`. For unit structs (`struct Foo;`), the definition is the entire scope.

4. **Trim trailing whitespace**: the returned range should not include trailing whitespace or newlines between the last meaningful token and the body opener.

Examples of what Definition selects:

```rust
// Variable: "let count: i32" (excludes "= 0;")
let count: i32 = 0;

// Variable without assignment: "let count: i32" (entire declaration minus semicolon)
let count: i32;

// Function: "pub fn process(input: &str) -> Result<Output>" (excludes "{ ... }")
pub fn process(input: &str) -> Result<Output> {
    // body
}

// Struct: "pub struct Config<T>" (excludes "{ ... }")
pub struct Config<T> {
    field: T,
}

// Enum (via Struct scope): "pub enum Status" (excludes "{ ... }")
pub enum Status {
    Active,
    Inactive,
}
```

For `Action::List` with `Component::Definition`, each `ListItem.val` contains the full definition text (trimmed). This enables `lefd` to show all function signatures in the buffer as a readable summary.

#### `Positional::Last` and `Positional::First` in standard resolution

For non-List, non-Word uses of Last and First (e.g. `jlfn` / `jlfn`):

Add arms in `apply_positional` that delegate to a scan over the scope's symbol list:

- `Positional::First`: resolve all symbols matching the scope kind, sort by position, return the range of the first one's component. For Line/Buffer scopes with non-Word components, return the beginning of the scope.
- `Positional::Last`: same but return the last one.

For LSP scopes, this requires calling `document_symbols()` and finding the first/last matching symbol. This is structurally similar to the `Next`/`Previous` positional arms already present.

#### `Action::List` cursor and mode

In `resolve_cursor_and_mode`, add:

```rust
Action::List => (None, None),
```

List does not move the cursor or change mode at the resolver stage; the TUI handles cursor navigation when the user presses Enter in the dialog.

### 5. Patcher — `src/commands/chord_engine/patcher.rs` (Layer 1)

Add a `List` arm:

```rust
Action::List => ChordAction {
    buffer_name: buffer_name.to_string(),
    diff: None,
    yanked_content: None,
    cursor_destination: None,
    mode_after: None,
    highlight_ranges: vec![],
    warnings: vec![],
    listed_items: resolution.listed_items,
},
```

The patcher is otherwise scope- and component-agnostic; no changes are needed for Word, Last, or First.

### 6. `ListFrontend` trait — `src/commands/chord_engine/types.rs` (Layer 1)

Define the trait in Layer 1 alongside `ListItem`, following the same dependency-inversion pattern used for `FrontendCapabilities` in `src/commands/chord.rs`. Layer 1 defines the interface it needs; Layer 2 supplies the implementations.

```rust
pub trait ListFrontend {
    fn show_list(&mut self, state: &mut EditorState, items: &[ListItem]) -> Result<()>;
}
```

`EditorState` is Layer 0, so this import does not violate the dependency rules. Both `CliFrontend` and `TuiFrontend` in Layer 2 implement this trait with different behavior.

### 7. CLI frontend — `src/frontend/cli_frontend.rs` (Layer 2)

Implement `ListFrontend`:

```rust
impl ListFrontend for CliFrontend {
    fn show_list(&mut self, _state: &mut EditorState, items: &[ListItem]) -> Result<()> {
        for item in items {
            println!("{}:{}  {}", item.line + 1, item.col + 1, item.name);
        }
        Ok(())
    }
}
```

Line and column numbers are printed 1-indexed to match conventional tool output. If `items` is empty, print nothing and exit with code 0.

Extend the CLI exec path to detect a `ChordAction` with non-empty `listed_items` and dispatch to `show_list` instead of applying a diff.

### 8. TUI list dialog — `src/frontend/tui/` (Layer 2)

Add a new module `src/frontend/tui/list_dialog.rs` containing a `ListDialog` widget:

```rust
pub struct ListDialog {
    pub items: Vec<ListItem>,
    pub selected: usize,
    pub scroll_offset: usize,
}
```

Rendering: centered overlay with a title bar ("List Results"), a scrollable list of entries formatted as `{val}  (line {line+1})`, and a highlight on the selected row. The visible window adjusts `scroll_offset` to keep `selected` in view.

Key handling (in `EditorMode::ListDialog` or similar new mode):
- `Up` / `Down` arrows: move `selected`, adjust `scroll_offset` if needed
- `Enter`: jump the viewable buffer area if needed, set `state.cursor_line` and `state.cursor_col` from the selected item, close dialog, return to Edit mode
- `Escape`: close dialog without navigating, return to previous mode

Implement `ListFrontend` for `TuiFrontend`:

```rust
impl ListFrontend for TuiFrontend {
    fn show_list(&mut self, state: &mut EditorState, items: &[ListItem]) -> Result<()> {
        state.list_dialog = Some(ListDialog { items: items.to_vec(), selected: 0, scroll_offset: 0 });
        state.mode = EditorMode::ListDialog;
        Ok(())
    }
}
```

Add `list_dialog: Option<ListDialog>` to `EditorState`. Render the dialog overlay in the main draw function when `state.list_dialog.is_some()`.

---

## Edge Case Considerations

### List action

- **Empty results**: if no items match the scope/component/positional combination, CLI prints nothing and exits 0; TUI shows the dialog with the message "No results" and a single Escape-to-close affordance.
- **List with LSP not ready**: for LSP-scoped chords, the resolver returns a `ChordError::resolve` describing the LSP-unavailable state, same as existing behavior for other LSP chords.
- **List + Inside positional with no cursor**: emit `"Inside positional requires a cursor position (cursor arg)"` before any LSP calls.
- **List + After/Before/Next/Previous with no cursor**: same error pattern as the existing `Until` and `Next` positionals.
- **List + non-Name component on LSP scope**: restrict to Name, Definition, End, and Self_ for the initial implementation. Reject at parse time with `"List action only supports Name, Definition, End, and Self components for LSP scopes"`. This avoids the complexity of rendering parameter lists or values as list items.
- **List on CLI is not interactive but is valid**: unlike Jump, List does not require an interactive frontend. `requires_interactive` remains `false` for `Action::List`.

### Word component

- **Cursor on whitespace**: if the cursor is not within a word, use the next word to the right for `Entire`. If no word exists to the right, use the previous word to the left. If the line is entirely whitespace, emit `"no word found on current line"`.
- **Word at line start**: `Previous` word has no result; emit `"no previous word on this line"`.
- **Word at line end**: `Next` word has no result; emit `"no next word on this line"`.
- **Empty line**: all word resolutions emit `"no word found on current line"`.
- **Tab characters**: treat as whitespace (word boundary). The col offset must account for tab width in terms of buffer column, not visual column.
- **Unicode**: word boundary detection uses `char::is_whitespace()` (not ASCII-only) to correctly handle Unicode whitespace. Positions are tracked as character offsets, matching LSP convention.
- **Word spanning a component boundary**: if the cursor is on a word that extends beyond the resolved scope, clip to the scope boundary.

### Definition component

- **No body (trait method, extern fn)**: if the function has no brace body (ends with `;`), the definition is the entire scope excluding the trailing semicolon.
- **Variable without assignment**: for `let x: i32;`, the definition is `let x: i32` (excludes the semicolon). For `let x = 5;`, the definition is `let x` (up to but not including the `=`). For `let x: i32 = 5;`, the definition is `let x: i32`.
- **Unit struct**: `struct Foo;` — the definition is `struct Foo` (excludes semicolon).
- **Tuple struct**: `struct Foo(i32, i32);` — the definition is `struct Foo` (excludes the parenthesized fields, which are its "body").
- **Multi-line signatures**: for functions with parameters spanning multiple lines, the definition includes all lines from the start through the return type, stopping before the `{`. The range is contiguous.
- **Attributes and doc comments**: attributes (`#[...]`) and doc comments (`///`) that precede the definition are NOT included. The definition starts at the first keyword token (visibility modifier or item keyword). This matches LSP symbol range start behavior.
- **Generic where clauses**: included in the definition. E.g. `fn foo<T>(x: T) -> T where T: Clone` is the full definition for a function whose body follows.
- **Definition with List action**: when used with `lefd`, each list item's `name` field contains the full definition text. Long signatures may be truncated in the TUI dialog display (with `...`) but the full text is available for cursor navigation.
- **Definition on invalid scopes**: `(Line, Definition)`, `(Buffer, Definition)`, `(Delimiter, Definition)`, `(Member, Definition)` are all invalid combinations, rejected at parse time via `is_valid_combination`.

### Last and First positionals

- **Last/First on empty scope**: if no matching symbol or word exists in the scope, emit `"no {component} found in {scope}"` (e.g. `"no function found in buffer"`).
- **Last/First with List action**: the filter reduces the list to a single item. Empty result follows the empty-results handling above.
- **Last/First with Next/Previous**: not combinable — Last and First are positionals, so they occupy the same slot; the user cannot specify both. This is structurally enforced by the grammar.
- **Last on a single-item scope**: returns that item, same as First.
- **`jlfn` with no functions in buffer**: resolver emits `"no function found in buffer"`.

### Grammar disambiguation

- The short letter `l` now means List (action), Last (positional), and Line (scope). Position in the chord is unambiguous, but the parser must map `l` in position 1 to List, `l` in position 2 to Last, and `l` in position 3 to Line. The existing position-indexed parsing already handles this correctly.
- Similarly, `f` means First (positional) and Function (scope). No structural change required.
- `w` and `d` are new to the component set and have no conflicts.

---

## Test Considerations

### Grammar unit tests — `src/data/chord_types.rs`

- **Round-trip**: `Action::from_short("l") == Some(Action::List)` and back; `Component::from_short("w") == Some(Component::Word)` and back; `Positional::from_short("l") == Some(Positional::Last)` and back; `Positional::from_short("f") == Some(Positional::First)` and back.
- **Uniqueness**: update `each_position_has_unique_short_letters` to include `"l"` in actions, `"w"` and `"d"` in components, and `"l"`, `"f"` in positionals.
- **`is_valid_combination` with Word**: assert `(Line, Word)` and `(Buffer, Word)` are valid; assert `(Function, Word)`, `(Struct, Word)`, `(Variable, Word)`, `(Member, Word)`, `(Delimiter, Word)` are all invalid.
- **`is_valid_combination` with Definition**: assert `(Function, Definition)`, `(Variable, Definition)`, `(Struct, Definition)` are valid; assert `(Line, Definition)`, `(Buffer, Definition)`, `(Delimiter, Definition)`, `(Member, Definition)` are all invalid.
- **`is_valid_list_positional`**: assert Outside returns `false`; assert all other positionals return `true`.
- **`Action::List.requires_interactive()`**: assert returns `false`.

### Parser unit tests — `src/commands/chord_engine/parser.rs`

- `parse("lefn")` returns `Ok` with `Action::List`, `Positional::Entire`, `Scope::Function`, `Component::Name`.
- `parse("lisn")` returns `Ok` with `Action::List`, `Positional::Inside`, `Scope::Struct`, `Component::Name`.
- `parse("lafn")` returns `Ok` with `Action::List`, `Positional::After`, `Scope::Function`, `Component::Name`.
- `parse("celw")` returns `Ok` with `Action::Change`, `Positional::Entire`, `Scope::Line`, `Component::Word`.
- `parse("jnlw")` returns `Ok` with `Action::Jump`, `Positional::Next`, `Scope::Line`, `Component::Word`.
- `parse("jllw")` returns `Ok` with `Action::Jump`, `Positional::Last`, `Scope::Line`, `Component::Word`.
- `parse("jflw")` returns `Ok` with `Action::Jump`, `Positional::First`, `Scope::Line`, `Component::Word`.
- `parse("jlfn")` returns `Ok` with `Action::Jump`, `Positional::Last`, `Scope::Function`, `Component::Name`.
- `parse("lefd")` returns `Ok` with `Action::List`, `Positional::Entire`, `Scope::Function`, `Component::Definition`.
- `parse("cefd")` returns `Ok` with `Action::Change`, `Positional::Entire`, `Scope::Function`, `Component::Definition`.
- `parse("yevd")` returns `Ok` with `Action::Yank`, `Positional::Entire`, `Scope::Variable`, `Component::Definition`.
- `parse("celd")` returns `Err` because `(Line, Definition)` is an invalid combination.
- `parse("lefn(value:\"x\")")` returns `Err` containing `"List action does not accept a value argument"`.
- `parse("lofn")` returns `Err` containing `"List action does not support the Outside positional"`.
- Long form `parse("ListEntireFunctionName")` returns the same result as `parse("lefn")`.
- Long form `parse("ListEntireFunctionDefinition")` returns the same result as `parse("lefd")`.
- Long form `parse("JumpLastLineWord")` returns the same result as `parse("jllw")`.

### Resolver unit tests — `src/commands/chord_engine/resolver.rs`

- **`resolve_word_component` — Entire**: buffer line `"  hello world"`, cursor col 3 (on `h`) → word range covers `hello`.
- **`resolve_word_component` — Entire on whitespace**: cursor on the space between words → range covers the next word to the right.
- **`resolve_word_component` — Next**: cursor on `hello` → range covers `world`.
- **`resolve_word_component` — Previous**: cursor on `world` → range covers `hello`.
- **`resolve_word_component` — First**: returns range of first word on line regardless of cursor.
- **`resolve_word_component` — Last**: returns range of last word on line.
- **`resolve_word_component` — empty line**: returns error.
- **`resolve_definition_component` — Function**: buffer `"pub fn foo(x: i32) -> bool {\n    true\n}"` → definition range covers `"pub fn foo(x: i32) -> bool"`.
- **`resolve_definition_component` — Variable with type and assignment**: buffer `"let count: i32 = 0;"` → definition range covers `"let count: i32"`.
- **`resolve_definition_component` — Variable without assignment**: buffer `"let count: i32;"` → definition range covers `"let count: i32"`.
- **`resolve_definition_component` — Variable without type**: buffer `"let count = 0;"` → definition range covers `"let count"`.
- **`resolve_definition_component` — Struct**: buffer `"pub struct Config<T> {\n    field: T,\n}"` → definition range covers `"pub struct Config<T>"`.
- **`resolve_definition_component` — Enum (Struct scope)**: buffer `"enum Status {\n    Active,\n}"` → definition range covers `"enum Status"`.
- **`resolve_definition_component` — Trait method (no body)**: buffer `"fn process(&self) -> Result<()>;"` → definition range covers `"fn process(&self) -> Result<()>"`.
- **`resolve_definition_component` — Multi-line signature**: buffer with params on multiple lines → definition range spans all lines up to `{`.
- **`resolve_list` — Definition component**: buffer with two functions; `lefd` returns `ListItem`s whose names are full signatures.
- **`resolve_list` — Entire with mock LSP**: buffer with three functions; `lefn` returns three `ListItem`s in source order.
- **`resolve_list` — After filter**: cursor on line 5; functions on lines 2, 5, 8 → After filter returns only the function on line 8.
- **`resolve_list` — Before filter**: same buffer → Before returns only lines 2 and 5.
- **`resolve_list` — Last filter**: returns only the function on line 8.
- **`resolve_list` — First filter**: returns only the function on line 2.
- **`resolve_list` — empty results**: buffer with no functions; `lefn` returns empty `Vec<ListItem>`.
- **`Positional::Last` in standard resolution (non-List)**: buffer with two functions; `jlfn` resolves `cursor_destination` to the second function's name start.
- **`Positional::First` in standard resolution**: `jffn` resolves to the first function's name start.

### Patcher unit tests

- `Action::List` → `ChordAction.diff == None`, `listed_items` populated from resolver.
- Non-List actions → `listed_items` is empty.

### Frontend integration tests

- **CLI List output format**: resolve `lefn` against a buffer with two functions `foo` (line 1) and `bar` (line 5); assert stdout is `"1:1  foo\n5:1  bar\n"` (1-indexed).
- **CLI empty List**: assert empty stdout and exit code 0.
- **TUI ListDialog state**: call `show_list` on `TuiFrontend` with two items; assert `state.list_dialog.is_some()` and `state.mode == EditorMode::ListDialog`.
- **TUI Enter navigation**: simulate Enter keypress in ListDialog with `selected = 1`; assert `state.cursor_line` and `state.cursor_col` match `items[1].line` and `items[1].col` and `state.list_dialog.is_none()`.
- **TUI Escape dismissal**: simulate Escape; assert dialog is closed, cursor position unchanged.

### Manual test checklist

- Open a multi-function Rust file in TUI; in chord mode type `lefn` — a dialog appears listing all function names with line numbers
- Use Up/Down arrows to scroll; press Enter — cursor jumps to the selected function's name
- Type `lisn` with cursor inside a struct body — dialog lists only struct names within that scope
- Type `lafn` — dialog lists only functions after the cursor
- Type `lofn` — chord box turns red with the Outside error message
- Type `lefd` — dialog lists full function signatures (e.g. `pub fn process(input: &str) -> Result<Output>`) with line numbers
- Type `cefd` with cursor inside a function — the entire function signature is selected for editing; body remains intact
- Type `yevd` with cursor on a variable — yanks the variable declaration (keyword + name + type) without the assignment
- Run `ane exec file.rs lefd` — stdout shows one definition signature per line with line:col prefix
- Type `celw` with cursor on a word — word is cleared and Edit mode entered; surrounding words unaffected
- Type `jnlw` — cursor jumps to the next whitespace-delimited word on the same line
- Type `jllw` — cursor jumps to the last word on the current line
- Type `jflw` — cursor jumps to the first word on the current line
- Type `jlfn` — cursor jumps to the name of the last function in the buffer
- Run `ane exec file.rs lefn` — stdout shows one `line:col  name` entry per function, no diff output
- Run `ane exec file.rs lefn` on a file with no functions — empty stdout, exit code 0

---

## Codebase Integration

- **Layer 0** (`src/data/chord_types.rs`): add `Action::List`, `Component::Word`, `Component::Definition`, `Positional::Last`, `Positional::First`. Update `is_valid_combination` for Word and Definition. Add `is_valid_list_positional`. No imports from higher layers.

- **Layer 1** (`src/commands/chord_engine/`):
  - `types.rs`: add `ListItem` struct; add `listed_items: Vec<ListItem>` field to `ChordAction`; define `ListFrontend` trait here so Layer 2 implements it without Layer 1 importing from Layer 2.
  - `parser.rs`: add parsing for new variants; add List-specific post-parse validation.
  - `resolver.rs`: add `resolve_list` function for List action dispatch; add `resolve_word_component` for Word component; add `resolve_definition_component` for Definition component; add Last and First arms in `apply_positional`. Compiler non-exhaustive match errors after adding the new enum variants serve as the integration checklist.
  - `patcher.rs`: add `Action::List` arm emitting no diff and populating `listed_items`.

- **Layer 2** (`src/frontend/`):
  - `cli_frontend.rs`: implement `commands::chord_engine::types::ListFrontend`; extend exec path to dispatch List results to `show_list`.
  - `tui/list_dialog.rs`: new module with `ListDialog` struct and rendering logic.
  - `tui/tui_frontend.rs`: implement `ListFrontend`; add `EditorMode::ListDialog` variant; add dialog key handling; render dialog overlay in draw function.
  - `data/state.rs` (Layer 0): add `list_dialog: Option<ListDialog>` to `EditorState`. Since `ListDialog` contains `ListItem` which is defined in `commands`, this creates a Layer 1 → Layer 0 dependency if placed in `EditorState`. Instead, keep `list_dialog` on a TUI-specific state struct in Layer 2, or define a thin `ListDialogState` in Layer 0 that holds only the raw data (`Vec<(String, usize, usize)>`). The latter keeps Layer 0 clean.

- **Compiler-driven integration**: adding `Action::List`, `Component::Word`, `Component::Definition`, `Positional::Last`, and `Positional::First` to their enums will produce non-exhaustive match errors throughout the codebase. Use these as the integration checklist: `resolve_cursor_and_mode`, `apply_positional`, `resolve_scope`, `resolve_component`, the patcher's action dispatch, all `Display` impls, and all tests that enumerate variants.

- **`is_valid_combination` interaction**: the existing `is_valid_combination(scope, component)` governs scope/component pairs and is extended with Word and Definition rules. `is_valid_list_positional(positional)` is an additive validation called only when `action == List`. The parser calls both checks independently.

- **Word component independence**: Word resolution is purely text-based and never calls LSP. It is handled entirely within `resolve_word_component` using buffer text scanning, analogous to how `find_innermost_delimiter` works for the Delimiter scope. Word resolution has no dependency on the LSP engine being ready.

- **Definition component and LSP**: Definition resolution requires LSP to identify the scope range (like Name, Parameters, and other LSP-scoped components). The actual definition text extraction is text-based once the scope range is known — it scans for the body opener (`{` or `=`) from the scope start. This is similar to how Contents resolution works: LSP provides the scope, then text scanning finds the delimiters.
