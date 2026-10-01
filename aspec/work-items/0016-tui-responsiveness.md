# Work Item: Bug + Task

Title: prevent blocking work from freezing the TUI
Issue: not yet filed

## Summary:
Users report delays of 10–20 seconds around first opening the file tree with `Ctrl-T` and moving with arrow keys. Audit and remove blocking paths that can affect the entire TUI: synchronous highlighting, unbounded LSP waits, filesystem-event starvation, recursive tree initialization, and shutdown waiting on the LSP engine lock.

This plan was approved and implemented. See [implementation and validation](0016-tui-responsiveness-guide.md) for the delivered behavior, measurements, and remaining platform checks. The exact cause of the reported stall has not been established by a trace of the user's session. Fix the confirmed defects while adding enough timing evidence to distinguish them in real workloads.

### Findings and evidence

- **Confirmed: synchronous highlighting with quadratic token conversion.** `SyntaxEngine::compute` parses on the calling thread. `tree_sitter_parse::byte_to_char_col` calls `content.lines().nth(line_num)` for each token boundary, repeatedly scanning the file prefix. Every text-changing key invokes `refresh_buffer_caches`, so this can freeze ordinary typing, opening, reloading, and chord execution. Synthetic Rust input in the existing debug build took 0.20 seconds at 1,000 lines, 5.10 seconds at 5,000 lines, and 19.96 seconds at 10,000 lines. Input was repeated `let value = 123;\n`; these are diagnostic measurements, not release-build latency guarantees.
- **Confirmed: unbounded LSP transport waits can reach the UI thread.** `LspTransport::send_request` synchronously reads until its response arrives, with no response deadline. The syntax worker holds `Mutex<LspEngine>` across semantic-token requests; `execute_chord_input` acquires the same lock on the UI thread for every chord, including non-LSP chords. LSP-dependent resolution can also perform its own blocking requests. The existing 30-second startup timeout does not bound requests after startup.
- **Confirmed: exit depends on that engine lock.** `run` acquires the lock and shuts down servers before disabling raw mode and leaving the alternate screen. A background request that never returns can prevent terminal restoration, despite the existing bounded shutdown handshake.
- **Confirmed: filesystem-event processing has no fairness limit.** `event_loop` drains the watcher queue completely before drawing or reading keyboard input. Access events undergo path canonicalization before being ignored. Each inserted path scans and sorts the complete backing tree. Watching remains active when the tree is hidden.
- **Confirmed: first tree initialization blocks.** `FileTree::from_dir` scans all descendants, including `.git`, `target`, and dependency directories, although the initial view only displays depth-zero entries. Recursive watcher registration also blocks. A synthetic tree with 50,102 entries, 10,101 directories, and two visible rows took 232–310 ms for scanning, watcher registration, and startup-event path processing combined in this Linux environment. Watcher registration generated 10,101 access/open events. This alone did not reproduce the reported 10–20 second stall.
- **Confirmed: token merging is quadratic.** `merge::merge` checks each Tree-sitter token against LSP tokens. It runs in both the background worker and synchronous cache refresh. A non-overlapping synthetic case with 10,000 tokens from each source took about 400 ms in the debug build.
- **Additional recurring costs to measure:** every frame clones the complete syntax-token vector, scans it for each displayed editor line, recomputes nonblank line count, and wraps complete logical lines before clipping the viewport. Whole-buffer token counting also runs after each edit. Token-count initialization measured about 520 ms once; subsequent counts for 10,000 lines were about 12 ms, so it was not the dominant bottleneck in that probe.

`Ctrl-T` and tree up/down do not directly invoke parsing or acquire the LSP engine lock. An unchanged file normally hits the Tree-sitter cache. A slow LSP semantic response alone therefore does not prove the tree stall; LSP indexing could instead amplify filesystem events or rendering work. Verify this distinction during profiling.

## User Stories

### User Story 1: Navigate and edit while background work runs
As a: user

I want to:
Type, move the cursor, switch tree focus, and scroll while highlighting, language-server requests, and filesystem updates are running

So I can:
Use the editor without multi-second pauses or queued keystrokes suddenly replaying after a stall

### User Story 2: Recover from a slow or stuck language server
As a: user

I want to:
Cancel an outstanding chord, continue using non-LSP commands, and quit even when a language server never responds

So I can:
Keep working and return to a usable terminal without killing the editor externally

### User Story 3: Open large project trees promptly
As a: user

I want to:
Open the tree and navigate available rows while additional directories and watches load in the background

So I can:
Browse a project without waiting for every generated or dependency file to be scanned

## Implementation Details:

### 1. Establish timing evidence and a responsiveness budget

- Add opt-in diagnostic timings for event-loop iterations, filesystem batches, tree scan/watch setup, syntax parse/conversion/merge, token counting, rendering, chord resolution, and LSP queue/request duration. Include counts and operation names; avoid recording file contents or credentials. Write diagnostics outside watched project paths to avoid feedback loops.
- Record debug/release profile, OS, fixture size, active file size, and LSP state with benchmark results. Measure cold/warm runs and input-to-visible-update latency, not just worker execution time.
- Proposed review targets: on documented local release-build fixtures, input-to-visible-update p95 below 100 ms and no application-induced UI stall above 250 ms during background work. Give each filesystem batch a small elapsed-time budget (initial proposal: 5 ms) plus an event-count cap. These targets need a recorded baseline and reference hardware before becoming CI timing assertions.

### 2. Make LSP requests bounded and keep waits off the UI thread

- Give transport I/O a single owner with request IDs and response dispatch. Use a dedicated reader and deadline-aware request completion rather than checking a clock around a blocking `read_line`, which cannot interrupt a silent server. Handle server-initiated requests and notifications separately from responses.
- Introduce configurable request deadlines distinct from startup deadlines (initial proposal: 5 seconds per interactive request). Cancellation must release the request's waiters. Late responses must not satisfy another request; reconnect/restart must clear pending requests predictably.
- Do not hold a global engine mutex across network/process I/O. Route background semantic requests and interactive operations through an engine worker or per-server request queues. Give interactive work priority over replaceable highlighting requests.
- Resolve LSP-dependent chords asynchronously against a captured buffer revision. Apply the resulting action on the UI thread only if the buffer/revision and operation identity still match. Show pending/failure status and support `Esc` cancellation without blocking navigation.
- Remove the engine-lock dependency from non-LSP chord execution. Refactor resolver access to LSP behind a narrow provider interface or equivalent separation, preserving headless CLI behavior and existing error semantics.

### 3. Fix highlighting complexity and schedule expensive refreshes

- Build a line-offset/line-slice index once per source snapshot and pass it through token emission. Convert byte columns using the indexed line; retain UTF-8 character-column semantics. Reuse the index for multiline tokens instead of collecting the entire file's lines for each token.
- Replace the nested token-overlap scan with an indexed or sweep-based merge that exploits sorted line/column positions. Preserve the rule that any overlapping LSP token suppresses the entire Tree-sitter token, including nested/overlapping tokens within either source.
- Move parse, merge, and token counting off the event thread. Use bounded, latest-wins work keyed by buffer identity and revision; discard stale results after edits, reloads, path changes, and buffer switches. Keep the previous valid highlighting while a new revision computes.
- Share immutable token snapshots so drawing does not clone every token under a mutex. Index tokens by line for viewport rendering. Cache line count/nonblank line count by revision and avoid wrapping an entire very long logical line when only a small viewport is needed.
- Keep the existing debounced LSP behavior, but do not let a slow semantic request delay local highlighting or keyboard processing. Consider incremental Tree-sitter parsing after the indexed conversion and worker changes are correct; full reparsing must still satisfy responsiveness goals through asynchronous execution.

### 4. Bound filesystem work and make tree opening asynchronous

- Drop access events before canonicalization or tree/buffer work, preferably before enqueueing them. Normalize only paths needed for relevant create/remove/rename/write handling; retain atomic-save detection.
- Process a bounded batch per frame, prioritizing keyboard input and redraws. Coalesce redundant events by path while preserving rename pairs and remove/create ordering. Use a bounded queue or overflow flag with asynchronous reconciliation so an event storm cannot grow memory without limit or silently lose final state.
- Batch backing-tree updates instead of sorting the entire tree after each insertion. Preserve visible selection by path when updates insert/remove rows before it.
- Show a loading tree immediately and populate directory children lazily or scan into a background snapshot. Move recursive watch setup off the UI thread, too. Keep the UI navigable during startup and directory expansion; right-arrow expansion currently scans all backing entries and inserts children individually.
- Make exclusions an explicit policy rather than silently changing what users can browse. Review skipping generated/dependency/VCS internals for indexing/watching while retaining an explicit way to display them. Merely hiding rows does not eliminate scan/watch costs.
- Reconcile events and scan results across watcher setup so files created, removed, or renamed during initialization are not lost. Audit `FileTree::entries` and `files_only` consumers before switching to lazy loading; callers needing a complete file list must receive one through a separate background/indexing path.

### 5. Guarantee terminal restoration and bounded shutdown

- Restore raw-mode, mouse-capture, and alternate-screen state through a scope guard, including setup failures and error exits. Do not make restoration depend on obtaining the engine lock.
- Signal worker cancellation and stop pending request waits through an independently reachable control path. Bound total shutdown time (initial proposal: 2 seconds overall), including lock acquisition and all servers, then terminate remaining child processes. Preserve the existing graceful handshake where it fits within that overall deadline.
- Workers must not send late mutations into a closed session. Ensure cancellation and terminal restoration do not deadlock on request queues or callback locks.

### Suggested delivery order

1. Add reproducible fixtures/timing hooks and fix indexed token conversion and merging.
2. Bound LSP requests, decouple non-LSP chords, and make terminal restoration independent of engine locks.
3. Move syntax/chord work to cancellable workers with revision-safe results.
4. Bound/coalesce watcher events and move scan/watch setup off-thread.
5. Optimize viewport rendering and validate all responsiveness targets in release builds.

Each step must include its regression coverage; performance optimization alone does not replace the scheduling and timeout fixes.

## Edge Case Considerations:

- Hung server, closed stdout, malformed/partial response, late response after timeout, server-initiated request, concurrent requests, and restart during an outstanding chord. A startup watchdog is insufficient once the server is running.
- User edits/switches/reloads/renames a buffer while a chord or syntax result is pending. Never apply an old action to newer text or another buffer; cancellation must not overwrite unrelated current status.
- Unicode, CRLF, empty/final lines, multiline tokens, nested highlights, and very long/minified lines. Keep byte/character/display-column semantics consistent.
- Filesystem storms from builds, dependency installs, Git operations, and LSP indexing. Linux access-event behavior is verified; macOS and Windows watcher semantics need independent checks.
- Atomic saves, rename pairs, queue overflow, permission errors, symlinks, directories disappearing mid-scan, and slow/network-mounted paths. Background operations still need bounded queues and cancellation even when underlying filesystem calls are slow.
- Hiding the tree must not disable active-file change detection. Retained tree watches must not starve the editor while hidden.
- Lazy loading must preserve empty-directory expansion state, selection, sorting, rename/delete/new-file behavior, and complete-file-list consumers. Review exclusion policy before changing default visibility.
- Quit during startup, parse, scan, semantic request, or chord resolution; failures entering alternate-screen/raw mode; multiple LSP servers. Terminal cleanup must be independent of worker success.

## Test Considerations:

- **Highlight correctness and scaling:** compare tokens before/after indexing for supported languages and Unicode/multiline cases. Benchmark repeated source at 1k/5k/10k lines and very long single-line input in release builds. Verify near-linear conversion scaling instead of the observed quadratic curve. Test overlap-merge equivalence for nested, same-line, and non-overlapping intervals.
- **Stale results:** controllable workers finish requests out of order; assert only the current buffer revision/operation can update tokens, counts, or chord actions. Verify bounded latest-wins queues during rapid typing.
- **LSP nonresponse:** extend `mock_lsp_server` to ignore a request, send a partial response, delay a response beyond its deadline, and issue a server request. Verify timeout/cancellation, correct response routing, and recovery without an uninterruptible read.
- **Whole-TUI integration:** while the mock semantic request is stalled, navigate both panes, type in Edit mode, execute a non-LSP chord, cancel an LSP chord, and quit. Assert the loop continues processing input and the terminal is restored within the shutdown budget. Use a PTY or an injectable terminal/input harness as appropriate.
- **Watcher fairness:** continuously produce events while injecting keyboard input; verify batch budgets, redraw progress, bounded backlog, early access-event filtering, and reconciliation after overflow. Check the final tree/disk-change state after create/remove/rename storms and atomic saves.
- **Large tree startup:** reuse a documented fixture with approximately 50k entries/10k directories and measure first `Ctrl-T`, warm reopen, expansion, and navigation during scan/watch setup. Verify creation/removal during initialization and explicit excluded-directory visibility behavior.
- **Rendering:** benchmark large token snapshots, many lines, and minified/very long lines with a fixed viewport. Verify work tracks visible content rather than repeatedly scanning all tokens or formatting offscreen wrapped rows.
- Prefer deterministic scheduling/deadline assertions in unit tests; keep machine-dependent latency thresholds in documented performance/integration runs. Record release-build measurements against the agreed reference workload before accepting the work item.
- Run the applicable existing tests, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`. Preserve headless chord behavior and file-tree/atomic-save regressions from WI-13/WI-14.

## Codebase Integration:

- Follow the strict Layer 0/1/2 dependency rules in `aspec/architecture/design.md`. Plain revision/result/tree state belongs in `src/data`; LSP transport, syntax computation, and chord resolution belong in `src/commands`; terminal scheduling, rendering, and application of completed actions belong in `src/frontend/tui`.
- Primary files: `src/frontend/tui/app.rs`, `fs_watcher.rs`, `editor_pane.rs`, `tree_pane.rs`, `title_bar.rs`; `src/commands/syntax_engine/{mod.rs,tree_sitter_parse.rs,merge.rs}`; `src/commands/lsp_engine/{engine.rs,transport.rs}`; `src/commands/chord_engine/resolver.rs`; `src/data/{state.rs,file_tree.rs,buffer.rs}`.
- Extend the existing syntax latest-wins mailbox and mock-LSP facilities where suitable; avoid introducing another unbounded queue or frontend dependency into shared command/data code.
- Preserve shared CLI/TUI resolver semantics through a narrow LSP access abstraction. The architectural promise that non-LSP chords run without waiting for LSP must hold in implementation.
- Update `aspec/architecture/lsp-engine.md` and relevant TUI documentation for deadlines, cancellation, loading behavior, diagnostics, and any approved exclusion policy. Coordinate tree changes with `0013-on-disk-updates.md` and `0014-filetree-enhancements.md`.
