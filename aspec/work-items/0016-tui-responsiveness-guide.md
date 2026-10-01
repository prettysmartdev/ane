# WI-16 implementation and validation

The approved [work item](0016-tui-responsiveness.md) is implemented. The original user-session stall has no trace, so these measurements demonstrate fixes for confirmed blocking paths rather than attribution of that particular stall.

## Delivered behavior

- Ctrl-T immediately shows a loading tree. Recursive scanning, watcher setup, and teardown run on a worker. Existing visibility is preserved, including generated directories; this is a background full scan rather than lazy directory loading.
- Watcher events use a bounded queue, early access-event filtering, per-path modification coalescing, and asynchronous reconciliation after overflow. UI batches stop after 64 events or approximately 5 ms; selection follows its path across updates.
- Local parsing, token merging, token counting, and nonblank line counting run on a separate latest-wins worker. Revision checks reject obsolete local and semantic results. Immutable token snapshots, visible-line token lookup, cached counts, and viewport-limited wrapping reduce rendering work.
- Tree-sitter token conversion indexes source lines once, with indexed Unicode character boundaries. Token merging uses a sorted interval sweep while preserving whole-token overlap suppression.
- LSP pipe readers and writers run independently. Requests have IDs, bounded outbound queues, cancellation, and deadlines (5 seconds normally; 30 seconds during initialization). Server requests cannot be confused with client responses, and late responses cannot satisfy another request. Global engine locks are released before waiting.
- Chords resolve on a worker using captured unsaved document contents. Results apply only to the matching buffer, generation, cursor, mode, focus, and operation. Esc cancels pending resolution after any active modal handles it. Non-LSP chords do not require the engine lock. Interactive requests defer new replaceable semantic requests.
- Terminal restoration precedes LSP shutdown. An independent child-process control path bounds the final shutdown wait to 2 seconds even if an engine lock is unavailable.

## Reference measurements

Linux x86_64, AMD EPYC 9V74, five visible logical CPUs with a four-CPU container quota, local filesystem, Rust 1.95. These are one-machine diagnostic results, not portable latency guarantees.

The release PTY run used 40,001 files and 10,101 directories, a 10,000-line Rust buffer, a mock LSP that never answers semantic/symbol requests, and continuous filesystem creates/removes. It checks actual selection/cursor changes, cancellation status, terminal attributes, and alternate-screen restoration.

| Measurement | Result |
| --- | ---: |
| First populated tree visible | 202.95 ms |
| Tree arrow-key updates, p95 | 18.91 ms |
| Typing updates, p95 | 16.65 ms |
| Maximum sampled navigation/typing update | 21.00 ms |
| Pending chord cancellation | 8.84 ms |
| Terminal restoration after final quit input | 0.26 ms |

The input measurements pass the proposed reference targets of p95 below 100 ms and sampled maximum below 250 ms. First-tree completion includes the full asynchronous scan; input remains available while it loads. A smaller fixture also passed (navigation p95 6.14 ms, typing p95 5.30 ms).

For the repeated Rust source fixture, the original debug parser took 0.20/5.10/19.96 seconds at 1k/5k/10k lines. The new debug parser took roughly 13–16/70–73/139–163 ms, providing a same-profile comparison. The new release parser took 2.4–3.1/12.6–13.7/25.9–29.2 ms. Release merging took 0.37 ms for 10k tokens per source and 4.61 ms for 50k tokens per source. Parsing remains a full snapshot parse; the improvement comes from indexing and asynchronous scheduling rather than incremental Tree-sitter parsing.

## Reproduce and review

```sh
cargo test --offline --locked
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
cargo check --offline --no-default-features
cargo build --offline --release --features test-support --bins --examples
cargo run --offline --release --example syntax_performance
python3 scripts/tui_responsiveness.py
```

The full suite passed: 579 unit tests and 74 integration tests. Clippy, formatting, no-default-features compilation, and the release build passed. New coverage exercises Unicode/CRLF token conversion, interval-merge equivalence, stale worker results, non-LSP execution while the engine lock is held, silent/partial/late LSP responses, server-request ID collisions, unsaved documents, cancellation, queue overflow, desired-watch replacement, viewport wrapping, and distant scroll jumps.

For real-project traces, start ANE with `ANE_TIMINGS=1`. The bounded background logger writes `/tmp/ane-timings-<pid>.jsonl` on this platform (the platform temporary directory elsewhere), recording operation durations, counts, profile, OS, and architecture without source text or project paths. The watcher ignores its own timing file. Samples can be dropped when the queue fills. Event-loop timing includes idle terminal polling; use operation-specific timings to identify expensive work.

## Remaining checks and limits

- Runtime validation here is Linux only. macOS/Windows watcher behavior, permission failures, and network filesystem responsiveness still need platform-specific review.
- Opening, saving, renaming, deleting, and individual external-buffer reloads still use synchronous filesystem operations. The filesystem batch budget is cooperative between events, so one slow file operation can exceed it. This change removes the identified recursive/tree, highlighting, LSP-wait, and queue-starvation paths; it does not make every filesystem operation asynchronous.
- Full tree snapshots still require memory proportional to project size and UI application of a completed snapshot. Visibility/exclusion policy is unchanged. Tree navigation may wait for initial population, though the editor loop remains responsive.
- LSP cancellation releases editor waiters and sends the protocol cancellation notification; the server controls whether its own computation stops. A blocked pipe worker may remain until its process is terminated. Shutdown restores the terminal independently.
- Existing source-size caps and shared synchronous syntax APIs remain. TUI metrics/highlighting may briefly show the previous completed revision while new results arrive.
