# ane Findings

## Finding 1: Buffer scope (`yebs`) requires a dummy parameter in exec mode

**Action attempted:** `ane exec --chord "yebs" <file>` and `ane exec --chord "yebs()" <file>`

**Expected outcome:** Read the entire file buffer without needing any target parameter, since buffer scope is whole-file and has no natural "target" symbol.

**What actually happened:** Both forms exit with `error: exec mode requires explicit parameters, e.g. yebs(fn_name, "body")`. The workaround is `yebs(target:1)`, which succeeds and returns the full file contents. The error message is misleading — it implies a function name is needed, but buffer scope doesn't conceptually take one. The `target:1` value appears to be ignored for buffer scope but satisfies the parameter presence check.
## Finding 2: `aals` appends to wrong location in large files

**Action attempted:** `ane exec app.rs --chord "aals(target:1014, value:\"}\")"`  — intended to append a line after line 1014.

**Expected outcome:** A new line containing `}` inserted immediately after line 1014.

**What actually happened:** The new line was appended at the very end of the file (line 3033), not after line 1014. The `aals` chord with a line target appears to ignore the target line number and always appends at the end of the buffer.

## Finding 3: `cifc` merges function boundary lines

**Action attempted:** `cifc(target:handle_tree_keys, value:-)` to replace the contents of a function.

**Expected outcome:** The function's opening `{` line and closing `}` line remain intact; only the body between them is replaced.

**What actually happened:** The opening brace was merged with the first line of the new content (`{    match code {`), and the closing brace was merged with the last line of the match (`    }}`). The chord does not preserve line boundaries around the function body delimiters.
## Finding 4: `cifn` (ChangeInFunctionName) prepends `fn` to the replacement

**Action attempted:** `cifn(target:centered_rect, value:"pub(super) fn centered_rect")` to change the function signature from `fn centered_rect` to `pub(super) fn centered_rect`.

**Expected outcome:** The function signature becomes `pub(super) fn centered_rect(...)`.

**What actually happened:** The result was `fn pub(super) fn centered_rect(...)` — the chord prepended `fn` to the value, producing a double `fn` keyword. The chord appears to only replace the function *name* itself (the identifier after `fn`), not the full declaration prefix.
## Finding 5: `aebs` does not insert a leading blank line before appended content

**Action attempted:** `aebs(target:1, value:-)` with stdin containing new-file-state content starting with `\n## Finding 2:...` to append a new section to `ane-findings.md`.

**Expected outcome:** A blank line separating the existing content from the newly appended content, preserving the visual separation between markdown sections.

**What actually happened:** The appended content was concatenated directly after the last line of the existing file with no blank line in between. The `\n` at the start of the stdin value was consumed or ignored, so `## Finding 2:` appeared on the very next line after `## Finding 1`'s last paragraph. The same issue occurred between Findings 3 and 4. Workaround: manually include an extra blank line at the start of the stdin value, or use a different chord.
## Finding 6: Used builtin Edit instead of `ane` for large multi-line test insertions

**Action attempted:** Inserting ~400-line test blocks at the end of `app.rs` and `exit_modal.rs` using `ane` Change/Append chords.

**Expected outcome:** Use `cels` (ChangeEntireLineSelf) on the last line or `aebs`/`aals` to append multi-line content without truncation or off-target placement.

**What actually happened:** Given existing findings (Finding 2: `aals` appends to wrong location; Finding 3: `cifc` merges boundary lines; Finding 5: `aebs` drops leading blank line), confidence in a correct multi-line insertion was low. The builtin Edit tool was used instead. A possible correct approach would be `cels(target:N, value:-)` on the last `}` line, piping a value that replaces just that line with the full new block — but this has not been verified for very large multi-line stdin values.

