# Work Item: Feature

Title: top bar additions
Issue: issuelink

## Summary:
- In the top bar, pinned to the righthand side, show buffer information:

  `{cursor x:y} | {x loc} | {x lines} | {~x tokens}`

- **loc** = non-empty lines in the buffer
- **lines** = total line count
- **tokens** = approximate BPE token count (tiktoken or equivalent)
- Update in real-time if performant; debounced otherwise
- Text should be bright white or black on the existing DarkGray top bar background


## User Stories

### User Story 1:
As a: user

I want to: see my cursor position, line counts, and approximate token count in the top bar at all times

So I can: understand the size and cost of my buffer at a glance without leaving the editor, especially when passing context to an LLM

### User Story 2:
As a: user

I want to: see LOC separate from total lines

So I can: quickly distinguish how much of the file is substantive code versus blank lines or whitespace

### User Story 3:
As a: user

I want to: see an approximate token count prefixed with `~`

So I can: estimate whether the current buffer will fit within an LLM's context window before sending it


## Implementation Details:
- **Location:** `src/frontend/tui/title_bar.rs` — replace the existing right-side cursor span (`{line}:{col}` in DarkGray) with the full buffer info string
- **Format:** ` {cursor_line}:{cursor_col} | {loc} loc | {lines} lines | ~{tokens} tokens ` — render in `Color::White` (bright white) to contrast against the DarkGray bar background
- **LOC computation:** count lines in `buf.lines` where `!line.trim().is_empty()`; this is O(n) and cheap enough to compute inline on every draw
- **Total lines computation:** `buf.lines.len()` — trivially O(1)
- **Token approximation:** use a whitespace-split word count multiplied by a fixed coefficient (e.g. `words * 4 / 3`) as a lightweight approximation, or integrate `tiktoken-rs` for BPE accuracy. Given the 50 ms render loop, prefer the cheap approximation first; only reach for tiktoken-rs if more accuracy is required. If tiktoken-rs is used, cache the count in `EditorState` and recompute on buffer-modified events (already tracked in the event loop via `buffer_modified`) rather than on every draw
- **Debounce strategy:** LOC and line counts can be computed on every draw with no debounce (negligible cost). Token counting with tiktoken-rs should be debounced: store `cached_token_count: usize` in `EditorState` and update it in the `buffer_modified` branch of `event_loop` in `src/frontend/tui/app.rs`, mirroring how `syntax_engine.compute` is triggered
- **Layout:** the right info string replaces the current `right_text` span in `title_bar::render`. Reuse the existing `right_start` padding logic — compute `right_len` from the new string's character count, keep `mid_pad` filling the gap between the centered filename and the right-anchored info block
- **Color:** use `Style::default().fg(Color::White)` for bright white text (the existing DarkGray bar provides the contrast)


## Edge Case Considerations:
- **Empty buffer / no open file:** `state.current_buffer()` returns `None` — render `0:0 | 0 loc | 0 lines | ~0 tokens` or omit the right block entirely; do not panic
- **Very wide info string on a narrow terminal:** the right info can overflow into the centered filename area; guard with `right_start.saturating_sub(right_len)` and truncate or hide the info block if `right_len >= width`
- **Files with only blank lines:** LOC will be 0 while lines > 0 — both values should be shown correctly; `0 loc | N lines` is a valid and informative display
- **Large files (10k+ lines):** O(n) LOC scan on every draw will remain fast (sub-millisecond for typical files), but token counting via tiktoken-rs on every draw is unacceptable; the debounce/cache approach in `EditorState` is required in that case
- **Unicode / multi-byte content:** cursor position is already tracked as byte offsets in `EditorState`; display as the existing `cursor_line:cursor_col` values (1-indexed is conventional — decide and document the convention; the existing bar shows 0-indexed, so match it for consistency)
- **Token count staleness:** if cached, the token count briefly lags after a keystroke until the next `buffer_modified` event fires the recompute; prefix `~` already signals approximation, so this is acceptable


## Test Considerations:
- Unit-test `compute_loc(lines: &[String]) -> usize` extracting the LOC logic into a standalone function; assert it returns 0 for all-blank buffers, correct counts for mixed content, and handles lines with only whitespace as empty
- Unit-test `compute_token_approx(content: &str) -> usize` for the approximation function; assert it returns 0 for empty content and a plausible range for known inputs
- Test the title bar render output does not panic when `current_buffer()` is `None`
- Test that the right info block is fully within the bar width for a range of terminal widths (e.g. 40, 80, 120 columns) — the existing `title_bar::render` is a pure function that can be called in tests with a mock frame and `Rect`
- If the token count is cached in `EditorState`, add a test verifying it updates after `buffer_modified` is set to `true` in the event loop (integration test in `app.rs` tests module, similar to the existing scroll tests)


## Codebase Integration:
- Follow the three-layer architecture: all computation lives in Layer 2 (`src/frontend/`) since it is purely display logic with no I/O. If a token-count utility function is extracted, place it in `src/frontend/tui/title_bar.rs` or a new `src/frontend/tui/buffer_info.rs` — never in `src/data/` or `src/commands/`
- If token count caching requires a new field on `EditorState` (Layer 0), the field must be a plain data type with no frontend imports — a `usize` is fine; trigger updates from Layer 2 (`app.rs`) in the same `buffer_modified` branch that already calls `syntax_engine.compute`
- Match existing `title_bar.rs` style: use `Span::styled`, compute lengths in characters not bytes, build a single `Line::from(spans)` and a `Paragraph`
- Any new dependency (e.g. `tiktoken-rs`) must be added as optional under `[features] frontends` in `Cargo.toml` to preserve the library-only build path; keep the default to a pure-Rust approximation with no additional dependency
- Run `cargo clippy -- -D warnings` and `cargo fmt --check` before marking complete
