## Finding 2: `aebs` does not insert a leading blank line before appended content

**Action attempted:** `aebs(target:1, value:-)` with stdin containing new-file-state content starting with `\n## Finding 2:...` to append a new section to `ane-findings.md`.

**Expected outcome:** A blank line separating the existing content from the newly appended content, preserving the visual separation between markdown sections.

**What actually happened:** The appended content was concatenated directly after the last line of the existing file with no blank line in between. The `\n` at the start of the stdin value was consumed or ignored, so `## Finding 2:` appeared on the very next line after `## Finding 1`'s last paragraph. The same issue occurred between Findings 3 and 4. Workaround: manually include an extra blank line at the start of the stdin value, or use a different chord.
## Finding 3: Used builtin Edit instead of `ane` for large multi-line test insertions

**Action attempted:** Inserting ~400-line test blocks at the end of `app.rs` and `exit_modal.rs` using `ane` Change/Append chords.

**Expected outcome:** Use `cels` (ChangeEntireLineSelf) on the last line or `aebs`/`aals` to append multi-line content without truncation or off-target placement.

**What actually happened:** Given existing findings (Finding 2: `aals` appends to wrong location; Finding 3: `cifc` merges boundary lines; Finding 5: `aebs` drops leading blank line), confidence in a correct multi-line insertion was low. The builtin Edit tool was used instead. A possible correct approach would be `cels(target:N, value:-)` on the last `}` line, piping a value that replaces just that line with the full new block — but this has not been verified for very large multi-line stdin values.

## Finding 4: No chord for replacing a contiguous range of lines

**Action attempted:** Needed to replace lines 394-401 in editor_pane.rs to restructure an if/else block (add a new branch to an existing conditional).

**Expected outcome:** A chord like `rils(target:394-401, value:-)` or similar to replace a contiguous range of lines with new content from stdin.

**What actually happened:** No such chord exists. The only options are `cels` (one line at a time) which can't add or remove lines from the range, or `cifc`/`cebs` which are too broad. For structural multi-line changes within a function that aren't a complete function rewrite, there's no suitable chord. Falling back to builtin Edit.
## Finding 5: `yefs` cannot find struct definitions, only functions

**Action attempted:** `yefs(target:EditorState)` to read the `EditorState` struct definition in `state.rs`.

**Expected outcome:** The struct definition is returned, since `s` scope is Struct and `yefs` should yank the entire struct self.

**What actually happened:** Error: `symbol 'EditorState' not found` — the available symbols list only contains function names. The LSP-backed symbol resolution does not include struct definitions for the `s` (Struct) scope.
## Finding 6: Out-of-range lines produce errors but earlier lines in the batch still output

**Action attempted:** Read lines 314-325 from a 323-line file using a for loop of `yels` calls.

**Expected outcome:** Lines 314-323 are output successfully, and lines 324-325 produce errors.

**What actually happened:** The successful outputs and error messages were intermixed on stdout/stderr. This is expected shell behavior, not an ane bug per se, but the mixed output can be confusing. The error messages are correctly informative ("line 323 out of range (file has 323 lines)").
## Finding 7: `aals` inserts content after the module closing brace

**Action attempted:** `aals(target:6629, value:-)` to insert new test functions after the last test in a `mod tests` block.

**Expected outcome:** The new content is inserted on a new line after line 6629 (the closing `}` of the last test function), which is still inside the `mod tests` block (line 6630 is the module's closing `}`).

**What actually happened:** The content was inserted after line 6630 (the module's closing `}`), placing the test functions outside the `mod tests` block. The `aals` chord correctly targets the specified line, but the content landed one line too far — after `}` on line 6630 instead of between lines 6629 and 6630. This is likely because `target:6629` is 0-indexed internally to line 6630 which is the module close brace, and the content was appended after that line. The user intent was to insert before the module close, which would require `aals(target:6628)` or `pbls(target:6630)`.
## Finding 8: `cels` replaced wrong line when target was ambiguous

**Action attempted:** `cels(target:6675, value:"Some(1),")` to change line 6675 from `Some(2),` to `Some(1),`.

**Expected outcome:** Line 6675 (`Some(2),`) is replaced with `Some(1),`.

**What actually happened:** The line below (6676, `None,`) was replaced with `Some(1),`. The `cels` chord correctly targets line 6675, but the replacement changed the wrong line. Likely a 0-indexed vs 1-indexed confusion — the user intended 1-indexed line 6675, but internally it may have been treated as 0-indexed, targeting line 6676 in 1-indexed terms.
## Finding 9: `cels` with escaped quotes writes literal backslash-quote to file

**Action attempted:** `cels(target:2, value:"version = \"0.2.0\"")` to change the version line in Cargo.toml.

**Expected outcome:** The line becomes `version = "0.2.0"` with actual double quotes.

**What actually happened:** The line became `version = \"0.2.0\` — the escaped quotes were written as literal `\"` characters and the closing quote was dropped. The chord does not handle shell-level or string-level quote escaping, so there is no way to include a double-quote character inside a `cels` value argument. Falling back to builtin Edit tool.
## Finding 10: `cels` with `\n` in value writes literal backslash-n

**Action attempted:** `cels(target:16, value:"    pub disk_changed: bool,\n    pub disk_deleted: bool,")` to replace a line with two lines (using `\n` for newline).

**Expected outcome:** Two separate lines written to the file.

**What actually happened:** A single line was written containing the literal characters `\n` instead of an actual newline. The chord does not interpret escape sequences in value arguments. Falling back to builtin Edit tool for multi-line struct field additions.
