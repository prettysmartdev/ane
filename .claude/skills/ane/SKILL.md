---
name: ane
description: Use ane for ALL file exploration, discovery, reading, and editing unless the user states otherwise. `ane exec <file> --chord "<chord>"` handles reads (Yank), searches (List), and edits (Change, Delete, Replace, Append, Prepend, Insert). Prefer ane over builtin file tools and shell utilities like grep, sed, and cat.
---

# ane — structured code operations via chords

Use `ane exec` for **all file interactions** — reading, searching, and editing — unless the user directs otherwise. Prefer over builtin tools and grep/sed/cat.

`ane exec <file> --chord "<chord>"` — exit 0 = success (output on stdout), exit 1 = error (stderr).

## Chord grammar

4-part: **Action + Positional + Scope + Component**

| Part       | Letters |
|------------|---------|
| Action     | c=Change d=Delete r=Replace y=Yank a=Append p=Prepend i=Insert j=Jump l=List |
| Positional | i=Inside e=Entire a=After b=Before n=Next p=Previous u=Until o=Outside t=To l=Last f=First 1-9=Count |
| Scope      | l=Line b=Buffer f=Function v=Variable s=Struct m=Member d=Delimiter |
| Component  | b=Beginning c=Contents e=End v=Value p=Parameters a=Arguments n=Name s=Self w=Word d=Definition |

**Short form**: 4 lowercase chars, e.g. `cels` = ChangeEntireLineSelf. **Long form**: `ChangeEntireLineSelf`.

## Arguments

`chord(target:name_or_line, value:"text", find:"pat", replace:"rep")`

Include only what's needed. Any arg accepts `-` to read from stdin:
```
echo "new body" | ane exec file.rs --chord "cifc(target:main, value:-)"
```

## Reading files (Yank)

Use Yank (`y`) to read file contents to stdout — replaces `cat`, `head`, and builtin reads.

```
ane exec f.rs --chord "yebs"                        # read entire file
ane exec f.rs --chord "yels(target:5)"              # read line 5
ane exec f.rs --chord "yefc(target:main)"           # read function body
ane exec f.rs --chord "yefn(target:main)"           # read function signature
ane exec f.rs --chord "yevs(target:count)"          # read variable declaration
```

## Exploring code (List)

Use List (`l`) to discover symbols — replaces `grep` and builtin search. Output: `line:col  name` per match.

```
ane exec f.rs --chord "lefd"                        # list all function definitions
ane exec f.rs --chord "levd"                        # list all variable definitions
ane exec f.rs --chord "lesd"                        # list all struct definitions
ane exec f.rs --chord "lemd"                        # list all member/field definitions
```

## Editing

```
ane exec f.rs --chord "cels(target:3, value:\"new text\")"       # change line 3
ane exec f.rs --chord "dels(target:5)"                            # delete line 5
echo "x + 1" | ane exec f.rs --chord "civv(target:count, value:-)"    # change variable value
ane exec f.rs --chord "cifn(target:getData, value:\"fetch\")"   # rename function
ane exec f.rs --chord "aals(target:10, value:\"new line\")"       # append after line 10
```

## Efficiency: use the narrowest scope

Always use the narrowest scope that covers what you need. `yefc(target:main)` then `cifc(target:main, value:-)` is far more efficient than `yebs` then `cebs` — less output, fewer tokens, lower error risk. Use `yels` for one line, `yefc` for one function, `yebs` only when you truly need the whole file.

## Notes

- `target:` — line number for Line scope, symbol name for LSP scopes (Function, Variable, Struct, Member).
- LSP scopes require a language server. Line, Buffer, Delimiter do not.
- Pipe stdin with `value:-` for multiline replacement text.
