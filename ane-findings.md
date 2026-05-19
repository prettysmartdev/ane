## Finding 3: `cifc` merges function boundary lines [RESOLVED]

**Action attempted:** `cifc(target:handle_tree_keys, value:-)` to replace the contents of a function.

**Expected outcome:** The function's opening `{` line and closing `}` line remain intact; only the body between them is replaced.

**What actually happened:** The opening brace was merged with the first line of the new content (`{    match code {`), and the closing brace was merged with the last line of the match (`    }}`). The chord does not preserve line boundaries around the function body delimiters.

**Resolution:** Fixed in `find_brace_range` — the WI-15 fix only excluded brace lines when `{` was the last char on the line and `}` was at column 0. Now checks whether the remainder after `{` or the prefix before `}` is whitespace-only, so indented closing braces (e.g. inside `impl` blocks) are properly excluded.
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

## Finding 7: `cifc` merges closing brace with last content line [RESOLVED]

**Action attempted:** `cifc(target:open_file, value:-)` to replace the function body of `open_file` in `state.rs`.

**Expected outcome:** The function body is replaced with the new content, preserving the closing `}` on its own line.

**What actually happened:** The closing brace of the function was merged with the last line of the replacement content, producing `Ok(())}` instead of `Ok(())` followed by `}` on the next line. This is a variant of Finding 3 — `cifc` does not preserve line boundaries for the closing delimiter. Workaround: use builtin Edit to fix the formatting after `cifc`.

**Resolution:** Same fix as Finding 3 — `find_brace_range` now uses whitespace-aware checks instead of exact column comparisons.
## Finding 8: No chord for replacing a contiguous range of lines

**Action attempted:** Needed to replace lines 394-401 in editor_pane.rs to restructure an if/else block (add a new branch to an existing conditional).

**Expected outcome:** A chord like `rils(target:394-401, value:-)` or similar to replace a contiguous range of lines with new content from stdin.

**What actually happened:** No such chord exists. The only options are `cels` (one line at a time) which can't add or remove lines from the range, or `cifc`/`cebs` which are too broad. For structural multi-line changes within a function that aren't a complete function rewrite, there's no suitable chord. Falling back to builtin Edit.
## Finding 9: `yefs` cannot find struct definitions, only functions

**Action attempted:** `yefs(target:EditorState)` to read the `EditorState` struct definition in `state.rs`.

**Expected outcome:** The struct definition is returned, since `s` scope is Struct and `yefs` should yank the entire struct self.

**What actually happened:** Error: `symbol 'EditorState' not found` — the available symbols list only contains function names. The LSP-backed symbol resolution does not include struct definitions for the `s` (Struct) scope.
## Finding 10: Out-of-range lines produce errors but earlier lines in the batch still output

**Action attempted:** Read lines 314-325 from a 323-line file using a for loop of `yels` calls.

**Expected outcome:** Lines 314-323 are output successfully, and lines 324-325 produce errors.

**What actually happened:** The successful outputs and error messages were intermixed on stdout/stderr. This is expected shell behavior, not an ane bug per se, but the mixed output can be confusing. The error messages are correctly informative ("line 323 out of range (file has 323 lines)").
