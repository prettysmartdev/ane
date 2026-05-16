# Chord System

Chords are the primary editing language in ane. Every chord is a four-part
instruction that describes **what to do**, **where within** the target,
**which scope** to operate on, and **which sub-part** of that scope to target.

```
<action><positional><scope><component>
```

Chords accept two equivalent forms:

| Form | Example | Meaning |
|------|---------|---------|
| Short (4 chars) | `cifc` | Change Inside Function Contents |
| Long (PascalCase) | `ChangeInsideFunctionContents` | identical |

Arguments are passed in parentheses after the chord:

```
cifc(target:foo, value:"return 0;")
ChangeInsideFunctionContents(target:foo, value:"return 0;")
```

---

## Part 1 -- Action

The action determines what happens to the resolved text range.

| Short | Long | Behavior |
|:-----:|---------|----------|
| `c` | Change | Replace the target range with `value`. Without a value, enters Edit mode at the target. |
| `d` | Delete | Remove the target range (replace with empty string). |
| `r` | Replace | If `find` and `replace` args are given, performs find-and-replace within the target. Otherwise behaves like Change. |
| `y` | Yank | Copy the target range text. Produces no diff. |
| `a` | Append | Insert `value` immediately **after** the end of the target range. |
| `p` | Prepend | Insert `value` immediately **before** the start of the target range. |
| `i` | Insert | Insert `value` at the cursor position (falls back to target start if no cursor). |
| `j` | Jump | Move the cursor to the target position. TUI-only -- the CLI rejects Jump chords. Produces no diff. |
| `l` | List | Collect all matching items as a list. In the TUI, shows a scrollable overlay where you can navigate with arrow keys and press Enter to jump to the selected item. In the CLI, prints each item with its line and column number. Produces no diff. |

---

## Part 2 -- Positional

The positional narrows or shifts the component range relative to the scope.

| Short | Long | Selects |
|:-----:|----------|---------|
| `i` | Inside | The content **inside** delimiters. If the component range starts/ends with matching `()`, `{}`, or `[]`, the delimiters are stripped. Otherwise returns the range unchanged. |
| `e` | Entire | The full component range, unchanged. |
| `a` | After | From the **end** of the component to the end of the scope. When component is Self, extends to end of buffer. |
| `b` | Before | From the **start** of the scope to the start of the component. When component is Self, extends to start of buffer. |
| `u` | Until | From the **cursor position** to the start of the component. Requires `cursor` arg. |
| `t` | To | From the **cursor position** through the end of the component. Like Until but with inclusive endpoint. Requires `cursor` arg. |
| `o` | Outside | Everything in the scope **except** the component (may produce two disjoint ranges). |
| `n` | Next | Advances to the next symbol of the scope's kind after the cursor. Requires `cursor` arg for LSP scopes. |
| `p` | Previous | Moves to the previous symbol of the scope's kind before the cursor. Requires `cursor` arg for LSP scopes. |
| `f` | First | The **first** occurrence of the component within the scope. |
| `l` | Last | The **last** occurrence of the component within the scope. |

---

## Part 3 -- Scope

The scope identifies which syntactic region to operate on.

| Short | Long | Identifies | LSP required |
|:-----:|----------|------------|:------------:|
| `l` | Line | A single line, selected by `target:N` (zero-indexed line number) or cursor position. | No |
| `b` | Buffer | The entire file. | No |
| `f` | Function | A function or method, selected by `target:<name>` or cursor position. | Yes |
| `v` | Variable | A variable or constant, selected by `target:<name>` or cursor position. | Yes |
| `s` | Struct | A struct or enum, selected by `target:<name>` or cursor position. | Yes |
| `m` | Member | A struct field or enum variant, selected by `target:<name>` (with optional `parent:<name>` for disambiguation) or cursor position. | Yes |
| `d` | Delimiter | The innermost matching delimiter pair surrounding the cursor: `()`, `{}`, `[]`, `""`, `''`, `` `` ``. Purely text-based scanning. | No |

LSP-scoped chords require an active language server. If the server is not
ready, the chord fails with a diagnostic message.

---

## Part 4 -- Component

The component selects a sub-part of the scope.

| Short | Long | What it targets | Valid scopes |
|:-----:|------------|-----------------|--------------|
| `b` | Beginning | Zero-width point at the **start** of the scope. | Line, Buffer, Delimiter |
| `c` | Contents | The brace-delimited block `{ ... }` of the scope. | Function, Struct, Delimiter |
| `e` | End | Zero-width point at the **end** of the scope. | all |
| `v` | Value | The assignment RHS for Variable, type/variant payload for Member. | Variable, Member |
| `p` | Parameters | The parenthesized parameter list `( ... )`. | Function |
| `a` | Arguments | The parenthesized argument list at a **call site** of the named function (searched outside the function's own scope). | Function |
| `n` | Name | The identifier (name) of the symbol. Uses the LSP `selectionRange` when available. For Line/Buffer scopes, returns a point at scope start. For Delimiter, returns the opening delimiter character. | all |
| `s` | Self | The entire scope range. | all |
| `w` | Word | A whitespace-delimited word. Text-based, no LSP required. | Line, Buffer |
| `d` | Definition | The entire definition signature of a scope, excluding its body. For functions: visibility + keyword + name + parameters + return type. For variables: keyword + name + type annotation (excluding assignment). For structs/enums: visibility + keyword + name + generics. | Function, Variable, Struct |

### Scope-component validity matrix

A chord with an invalid combination is rejected at parse time.

|              | Beginning | Contents | End | Value | Parameters | Arguments | Name | Self | Word | Definition |
|--------------|:---------:|:--------:|:---:|:-----:|:----------:|:---------:|:----:|:----:|:----:|:----------:|
| **Line**       | Y         | --       | Y   | --    | --         | --        | Y    | Y    | Y    | --         |
| **Buffer**     | Y         | --       | Y   | --    | --         | --        | Y    | Y    | Y    | --         |
| **Function**   | --        | Y        | Y   | --    | Y          | Y         | Y    | Y    | --   | Y          |
| **Variable**   | --        | --       | Y   | Y     | --         | --        | Y    | Y    | --   | Y          |
| **Struct**     | --        | Y        | Y   | --    | --         | --        | Y    | Y    | --   | Y          |
| **Member**     | --        | --       | Y   | Y     | --         | --        | Y    | Y    | --   | --         |
| **Delimiter**  | Y         | Y        | Y   | --    | --         | --        | Y    | Y    | --   | --         |

---

## Arguments

Arguments are key-value pairs inside parentheses, separated by commas.
Values containing spaces, commas, or parentheses must be quoted with `"`.

### All argument keys

| Key | Purpose | Accepted by |
|-----|---------|-------------|
| `target` | Identify what to operate on: a symbol name for LSP scopes, or a zero-indexed line number for Line scope. | all scopes that need a target |
| `parent` | Disambiguate a member when multiple parents define the same name. | Member scope |
| `cursor` | Cursor position as `"line,col"` (zero-indexed). | any (required by Until, To, Next, Previous) |
| `value` | Replacement text for Change/Append/Prepend/Insert. | Change, Replace, Append, Prepend, Insert |
| `find` | Substring to locate (used with Replace action). | Replace |
| `replace` | Replacement for each `find` match. | Replace |

### Examples by scope

```
# Line — target is a zero-indexed line number
cels(target:5, value:"new text")

# Buffer — no target needed (operates on whole file)
yebs

# Function — use 'target' to name the function
cifc(target:init, value:"\n    todo!()\n")
cifn(target:get_data, value:"fetch_data")

# Variable — use 'target' to name the variable
cevv(target:config, value:"Config::default()")

# Struct — use 'target' to name the struct
cesn(target:OldName, value:"NewName")

# Member — use 'target' + optional 'parent' for disambiguation
cemn(target:x, parent:Point, value:"horizontal")
cemv(target:count, parent:Stats, value:" usize")

# Delimiter — uses 'cursor' to locate the delimiter pair
cidc(cursor:"3,7", value:"x, y, z")

# Find-replace within a scope
rels(target:0, find:"foo", replace:"bar")
```

---

## List Action and Positional Filtering

The `List` action has special behavior compared to modification actions. Instead of generating a diff, it collects all matching items matching the scope and component, then applies the positional as a **filter** on those results.

### List in the TUI

When you execute a List chord in the TUI, a scrollable overlay appears showing all matching items:

```
┌──────────────────────────┐
│ List Results             │
├──────────────────────────┤
│ setup          (line 5)  │
│ process        (line 12) │
│ cleanup        (line 18) │
└──────────────────────────┘
```

Use **Up/Down arrow keys** to navigate, then press **Enter** to jump the cursor to the selected item's position. Press **Escape** to close the dialog without navigating.

### List in the CLI

When you run a List chord via `ane exec`, each result is printed on a separate line with the format `line:col  name`:

```
$ ane exec file.rs lefn
5:1  setup
12:1  process
18:1  cleanup
```

This is useful for scripting: pipe the output to other tools or parse it programmatically.

### Positional filtering for List

With List, the positional narrows the results:

| Positional | Behavior |
|:----------:|----------|
| `Entire` (`e`) | Return all items (no filtering). |
| `First` (`f`) | Return only the first item. |
| `Last` (`l`) | Return only the last item. |
| `Next` (`n`) | Return only the first item after the cursor. Requires `cursor` arg. |
| `Previous` (`p`) | Return only the first item before the cursor. Requires `cursor` arg. |
| `After` (`a`) | Return all items after the cursor. Requires `cursor` arg. |
| `Before` (`b`) | Return all items before the cursor. Requires `cursor` arg. |
| `Inside` (`i`) | Return only items within the innermost scope at the cursor. Requires `cursor` arg. |
| `Until` (`u`) | Return items between start of scope and cursor. Requires `cursor` arg. |
| `To` (`t`) | Return items between cursor and end of scope. Requires `cursor` arg. |

For example:
- `lefn` — list **all** function names in the buffer
- `llfn` — list **last** function name in the buffer (single result)
- `lafn` — list all function names **after** the cursor
- `lisn` — list struct names **inside** the current scope

---

## Resolution pipeline

Every chord passes through three stages before the action is applied:

```
 input string
      |
      v
  1. PARSE      chord text + args  -->  ChordQuery
      |
      v
  2. RESOLVE    ChordQuery + buffer + LSP  -->  ResolvedChord (text ranges)
      |            scope_range   = resolve_scope()
      |            component_range = resolve_component()
      |            target_ranges = apply_positional()
      v
  3. PATCH      ResolvedChord + buffer  -->  diff / yanked text
```

The resolver works from the outside in:

1. **Scope** -- locates the broad region (a line, the buffer, a function, ...).
2. **Component** -- narrows to a sub-part within the scope (body, name, parameters, ...).
3. **Positional** -- further adjusts the component range (strip delimiters, take before/after, ...).

The patcher then applies the action to the resulting target ranges.

---

## Worked examples

### Rename a function

```
cifn(target:get_data, value:"fetch_data")
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Change | replace range with value |
| Positional | Inside | strip delimiters (name has none, no-op) |
| Scope | Function `get_data` | full range of the function definition |
| Component | Name | the identifier `get_data` (via LSP selectionRange) |

Result: `get_data` is replaced with `fetch_data` everywhere in the definition.

### Replace a function's contents

```
cifc(target:process, value:"\n    todo!()\n")
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Change | replace range with value |
| Positional | Inside | strip the `{` `}` delimiters from the contents |
| Scope | Function `process` | full range of the function |
| Component | Contents | the `{ ... }` brace block |

Result: everything between `{` and `}` is replaced while the braces themselves are preserved.

### Delete everything before a function

```
dbfs(target:main)
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Delete | remove the range |
| Positional | Before | from buffer start to start of component |
| Scope | Function `main` | full range of `fn main` |
| Component | Self | the entire function |

Result: all text from the start of the file up to (but not including) `fn main` is deleted.

### Yank a struct's brace block

```
yefc(target:Config)
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Yank | copy text, no modification |
| Positional | Entire | the full component range including delimiters |
| Scope | Struct `Config` | full range of the struct definition |
| Component | Contents | the `{ ... }` brace block |

Result: the `{ field: Type, ... }` block (including braces) is copied to the yank register.

### Append to a variable's value

```
aevv(target:COUNT, value:" + 1")
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Append | insert text after the end of the range |
| Positional | Entire | full value range |
| Scope | Variable `COUNT` | full range of the variable declaration |
| Component | Value | the RHS of the `=` assignment |

Result: ` + 1` is inserted after the current value expression.

---

For exhaustive before/after examples of every scope/component combination, see [Chord Examples](02-chord-examples.md).

[<- Getting Started](00-getting-started.md) | [Next: Chord Examples ->](02-chord-examples.md)
