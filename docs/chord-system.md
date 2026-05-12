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
cifc(function:foo, value:"return 0;")
ChangeInsideFunctionContents(function:foo, value:"return 0;")
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
| `o` | Outside | Everything in the scope **except** the component (may produce two disjoint ranges). |
| `n` | Next | Advances to the next symbol of the scope's kind after the cursor. Requires `cursor` arg for LSP scopes. |
| `p` | Previous | Moves to the previous symbol of the scope's kind before the cursor. Requires `cursor` arg for LSP scopes. |

---

## Part 3 -- Scope

The scope identifies which syntactic region to operate on.

| Short | Long | Identifies | LSP required |
|:-----:|----------|------------|:------------:|
| `l` | Line | A single line, selected by `line:N` or cursor position. | No |
| `b` | Buffer | The entire file. | No |
| `f` | Function | A function or method, selected by `function:<name>` or cursor position. | Yes |
| `v` | Variable | A variable or constant, selected by `variable:<name>` or cursor position. | Yes |
| `s` | Struct | A struct or enum, selected by `struct:<name>` or cursor position. | Yes |
| `m` | Member | A struct field or enum variant, selected by `member:<name>` (with optional `parent:<name>` for disambiguation) or cursor position. | Yes |

LSP-scoped chords require an active language server. If the server is not
ready, the chord fails with a diagnostic message.

---

## Part 4 -- Component

The component selects a sub-part of the scope.

| Short | Long | What it targets | Valid scopes |
|:-----:|------------|-----------------|--------------|
| `b` | Beginning | Zero-width point at the **start** of the scope. | Line, Buffer |
| `c` | Contents | The brace-delimited block `{ ... }` of the scope. | Function, Struct |
| `e` | End | Zero-width point at the **end** of the scope. | all |
| `v` | Value | The assignment RHS for Variable, type/variant payload for Member. | Variable, Member |
| `p` | Parameters | The parenthesized parameter list `( ... )`. | Function |
| `a` | Arguments | The parenthesized argument list at a **call site** of the named function (searched outside the function's own scope). | Function |
| `n` | Name | The identifier (name) of the symbol. Uses the LSP `selectionRange` when available. For Line/Buffer scopes, returns a point at scope start. | all |
| `s` | Self | The entire scope range. | all |

### Scope-Component validity matrix

A chord with an invalid combination is rejected at parse time.

|            | Beginning | Contents | End | Value | Parameters | Arguments | Name | Self |
|------------|:---------:|:--------:|:---:|:-----:|:----------:|:---------:|:----:|:----:|
| **Line**     | Y         | --       | Y   | --    | --         | --        | Y    | Y    |
| **Buffer**   | Y         | --       | Y   | --    | --         | --        | Y    | Y    |
| **Function** | --        | Y        | Y   | --    | Y          | Y         | Y    | Y    |
| **Variable** | --        | --       | Y   | Y     | --         | --        | Y    | Y    |
| **Struct**   | --        | Y        | Y   | --    | --         | --        | Y    | Y    |
| **Member**   | --        | --       | Y   | Y     | --         | --        | Y    | Y    |

---

## Arguments

Arguments are key-value pairs inside parentheses, separated by commas.
Values containing spaces, commas, or parentheses must be quoted with `"`.

| Key | Purpose | Accepted by |
|-----|---------|-------------|
| `function` | Target function by name. | Function scope |
| `variable` | Target variable by name. | Variable scope |
| `struct` | Target struct/enum by name. | Struct scope |
| `member` | Target field/variant by name. | Member scope |
| `name` | Generic alias for any of the above. | any LSP scope |
| `parent` | Disambiguate a member when multiple parents define the same name. | Member scope |
| `line` | Zero-indexed line number. | Line scope |
| `cursor` | Cursor position as `"line,col"` (zero-indexed). | any (required by Until, Next, Previous) |
| `value` | Replacement text for Change/Append/Prepend/Insert. | Change, Replace, Append, Prepend, Insert |
| `find` | Substring to locate (used with Replace action). | Replace |
| `replace` | Replacement for each `find` match. | Replace |

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
cifn(function:get_data, value:"fetch_data")
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
cifc(function:process, value:"\n    todo!()\n")
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
dbfs(function:main)
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
yefc(struct:Config)
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
aevv(variable:COUNT, value:" + 1")
```

| Part | Value | Resolves to |
|------|-------|-------------|
| Action | Append | insert text after the end of the range |
| Positional | Entire | full value range |
| Scope | Variable `COUNT` | full range of the variable declaration |
| Component | Value | the RHS of the `=` assignment |

Result: ` + 1` is inserted after the current value expression.
