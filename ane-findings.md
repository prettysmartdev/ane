# ane Findings

## Finding 1: Buffer scope (`yebs`) requires a dummy parameter in exec mode

**Action attempted:** `ane exec --chord "yebs" <file>` and `ane exec --chord "yebs()" <file>`

**Expected outcome:** Read the entire file buffer without needing any target parameter, since buffer scope is whole-file and has no natural "target" symbol.

**What actually happened:** Both forms exit with `error: exec mode requires explicit parameters, e.g. yebs(fn_name, "body")`. The workaround is `yebs(target:1)`, which succeeds and returns the full file contents. The error message is misleading — it implies a function name is needed, but buffer scope doesn't conceptually take one. The `target:1` value appears to be ignored for buffer scope but satisfies the parameter presence check.
