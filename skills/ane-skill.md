---
name: ane
description: Execute structured code edits via the ane chord grammar. Use `ane exec <file> --chord "<chord>"` for precise, non-interactive edits — change lines, rename functions, replace function bodies, yank symbols, append or prepend text, and more. Use when you need a deterministic, diff-producing edit on a file.
---

# ane — structured code editing via chords

Use `ane exec <file> --chord "<chord>"` for precise, non-interactive edits. Exit 0 = success (diff on stdout). Exit 1 = error (stderr).

## Chord grammar

4-part: **Action + Positional + Scope + Component**

| Part       | Letters                                                              |
|------------|----------------------------------------------------------------------|
| Action     | c=Change d=Delete r=Replace y=Yank a=Append p=Prepend i=Insert      |
| Positional | i=Inside e=Entire a=After b=Before n=Next p=Previous u=Until o=Outside t=To |
| Scope      | l=Line b=Buffer f=Function v=Variable s=Struct m=Member d=Delimiter  |
| Component  | b=Beginning c=Contents e=End v=Value p=Parameters a=Arguments n=Name s=Self |

**Short form**: exactly 4 lowercase chars mapping to action+positional+scope+component, e.g. `cels` = ChangeEntireLineSelf.
**Long form**: `ChangeEntireLineSelf`.

## Arguments

`chord(target:name_or_line, value:"text", cursor:L:C, find:"pat", replace:"rep")`

Include only what's needed. Use `-` as value to read from stdin:
```
echo "new body" | ane exec file.rs --chord "cifc(target:main, value:-)"
```

## LSP scopes

Function, Variable, Struct, Member require LSP. Line, Buffer, Delimiter do not.

## Examples

```
ane exec f.rs --chord "cels(target:3, value:\"new text\")"       # change line 3
ane exec f.rs --chord "dels(target:5)"                            # delete line 5
echo "x + 1" | ane exec f.rs --chord "civv(target:count, value:-)"    # change variable value
ane exec f.rs --chord "cifn(target:getData, value:\"fetch\")"   # rename function
ane exec f.rs --chord "aals(target:10, value:\"new line\")"       # append after line 10
ane exec f.rs --chord "yefc(target:main)"                       # yank function body (stdout)
```

## Notes

- Use `target:` for all scopes — a line number for Line scope, a symbol name for LSP scopes.
- Pipe stdin with `value:-` for multiline replacement text.
