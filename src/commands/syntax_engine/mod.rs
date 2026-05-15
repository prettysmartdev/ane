pub mod merge;
pub mod tree_sitter_parse;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::commands::lsp_engine::LspEngine;
use crate::data::lsp::types::{Language, SemanticToken};

/// Callback trait for delivering computed syntax tokens to the frontend.
/// Layer 1 defines this; Layer 2 implements it.
pub trait SyntaxFrontend: Send + Sync {
    fn set_semantic_tokens(&self, path: &Path, tokens: Vec<SemanticToken>);
}

struct LspRequest {
    path: PathBuf,
    content: String,
    content_hash: u64,
    ts_tokens: Vec<SemanticToken>,
}

/// Single-slot mailbox with latest-wins semantics. `compute()` overwrites
/// any previously-queued request; the worker reads whichever request was
/// last submitted. This avoids the wasted LSP roundtrip a bounded channel
/// would cause when a stale request sits in the queue while newer ones
/// are dropped on the floor.
struct LspRequestSlot {
    inner: Mutex<SlotState>,
    cv: Condvar,
}

struct SlotState {
    request: Option<LspRequest>,
    shutdown: bool,
}

impl LspRequestSlot {
    fn new() -> Self {
        Self {
            inner: Mutex::new(SlotState {
                request: None,
                shutdown: false,
            }),
            cv: Condvar::new(),
        }
    }

    fn submit(&self, req: LspRequest) {
        let mut s = self.inner.lock().unwrap();
        s.request = Some(req);
        self.cv.notify_all();
    }

    /// Block until a request is available, or return None on shutdown.
    fn take(&self) -> Option<LspRequest> {
        let mut s = self.inner.lock().unwrap();
        loop {
            if s.shutdown {
                return None;
            }
            if let Some(req) = s.request.take() {
                return Some(req);
            }
            s = self.cv.wait(s).unwrap();
        }
    }

    /// Wait up to `dur` for a newer request to arrive. Returns the new
    /// request if one shows up (consuming it from the slot), or None if
    /// the window elapsed without any arrival (or on shutdown).
    fn wait_for_newer(&self, dur: Duration) -> Option<LspRequest> {
        let deadline = Instant::now() + dur;
        let mut s = self.inner.lock().unwrap();
        loop {
            if s.shutdown {
                return None;
            }
            if let Some(req) = s.request.take() {
                return Some(req);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (next, _) = self.cv.wait_timeout(s, deadline - now).unwrap();
            s = next;
        }
    }

    fn is_shutdown(&self) -> bool {
        self.inner.lock().unwrap().shutdown
    }

    fn signal_shutdown(&self) {
        let mut s = self.inner.lock().unwrap();
        s.shutdown = true;
        self.cv.notify_all();
    }
}

pub struct SyntaxEngine {
    ts_cache: HashMap<PathBuf, (u64, Vec<SemanticToken>)>,
    lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
    content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    frontend: Arc<dyn SyntaxFrontend>,
    request_slot: Arc<LspRequestSlot>,
}

fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

impl SyntaxEngine {
    pub fn new(lsp_engine: Arc<Mutex<LspEngine>>, frontend: Arc<dyn SyntaxFrontend>) -> Self {
        let request_slot = Arc::new(LspRequestSlot::new());
        let lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let w_frontend = Arc::clone(&frontend);
        let w_lsp_cache = Arc::clone(&lsp_cache);
        let w_hashes = Arc::clone(&content_hashes);
        let w_slot = Arc::clone(&request_slot);

        std::thread::spawn(move || {
            Self::lsp_worker(lsp_engine, w_slot, w_frontend, w_lsp_cache, w_hashes);
        });

        Self {
            ts_cache: HashMap::new(),
            lsp_cache,
            content_hashes,
            frontend,
            request_slot,
        }
    }

    /// Returns immediately. Runs tree-sitter synchronously (<2ms), then
    /// queues a debounced LSP token request on the background worker.
    pub fn compute(&mut self, path: &Path, content: &str) {
        let lang = match Language::from_path(path) {
            Some(l) => l,
            None => {
                // Unknown extension: clear any previous tokens so the
                // frontend renders plain text. Matches spec edge case
                // "Language with no tree-sitter and no LSP".
                self.frontend.set_semantic_tokens(path, Vec::new());
                return;
            }
        };
        let caps = lang.capabilities();
        let content_hash = hash_content(content);

        // Phase 1: tree-sitter (synchronous, cached by content hash)
        let ts_tokens = if caps.has_tree_sitter {
            if self.ts_cache.get(path).map(|(h, _)| *h) != Some(content_hash) {
                let tokens = tree_sitter_parse::parse(lang, content);
                self.ts_cache
                    .insert(path.to_path_buf(), (content_hash, tokens));
            }
            self.ts_cache.get(path).unwrap().1.clone()
        } else {
            vec![]
        };

        // Merge with any previously cached LSP tokens for this path
        let cached_lsp = self
            .lsp_cache
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default();
        let merged = if caps.has_lsp && !cached_lsp.is_empty() {
            merge::merge(&ts_tokens, &cached_lsp)
        } else {
            ts_tokens.clone()
        };

        // Deliver best-effort tokens to frontend immediately
        self.frontend.set_semantic_tokens(path, merged);

        // Update content hash for staleness detection by the worker
        self.content_hashes
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content_hash);

        // Phase 2: submit LSP request to the latest-wins slot. Any prior
        // unprocessed request is silently overwritten — the worker reads
        // whichever request was most recently submitted.
        if caps.has_lsp {
            self.request_slot.submit(LspRequest {
                path: path.to_path_buf(),
                content: content.to_string(),
                content_hash,
                ts_tokens,
            });
        }
    }

    fn lsp_worker(
        engine: Arc<Mutex<LspEngine>>,
        slot: Arc<LspRequestSlot>,
        frontend: Arc<dyn SyntaxFrontend>,
        lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
        content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    ) {
        let debounce = Duration::from_millis(300);

        while let Some(mut req) = slot.take() {
            // Debounce: keep taking newer requests for `debounce` since the
            // last arrival. Each newer request resets the window.
            while let Some(newer) = slot.wait_for_newer(debounce) {
                req = newer;
            }
            if slot.is_shutdown() {
                return;
            }

            // Fetch LSP semantic tokens
            let lsp_tokens = engine
                .lock()
                .unwrap()
                .semantic_tokens(&req.path, &req.content)
                .unwrap_or_default();

            // Staleness check: discard if content changed since request was queued
            let current = content_hashes.lock().unwrap().get(&req.path).copied();
            if current != Some(req.content_hash) {
                continue;
            }

            // Cache LSP tokens for use by future compute() calls
            lsp_cache
                .lock()
                .unwrap()
                .insert(req.path.clone(), lsp_tokens.clone());

            // Merge with tree-sitter tokens and deliver
            let merged = if !lsp_tokens.is_empty() {
                merge::merge(&req.ts_tokens, &lsp_tokens)
            } else {
                req.ts_tokens
            };
            frontend.set_semantic_tokens(&req.path, merged);
        }
    }
}

impl Drop for SyntaxEngine {
    fn drop(&mut self) {
        // Wake the worker thread so it can exit instead of blocking forever
        // on the slot's condvar.
        self.request_slot.signal_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
    use crate::data::lsp::types::SemanticToken;

    use super::tree_sitter_parse::PARSE_COUNT;
    use super::{SyntaxEngine, SyntaxFrontend};

    type RecordedCalls = Arc<Mutex<Vec<(PathBuf, Vec<SemanticToken>)>>>;

    struct RecordingFrontend {
        calls: RecordedCalls,
    }

    impl SyntaxFrontend for RecordingFrontend {
        fn set_semantic_tokens(&self, path: &Path, tokens: Vec<SemanticToken>) {
            self.calls
                .lock()
                .unwrap()
                .push((path.to_path_buf(), tokens));
        }
    }

    fn make_engine() -> (SyntaxEngine, RecordedCalls) {
        let calls: RecordedCalls = Arc::new(Mutex::new(Vec::new()));
        let frontend = Arc::new(RecordingFrontend {
            calls: Arc::clone(&calls),
        });
        let lsp = Arc::new(Mutex::new(LspEngine::new(LspEngineConfig::default())));
        let engine = SyntaxEngine::new(lsp, frontend as Arc<dyn SyntaxFrontend>);
        (engine, calls)
    }

    fn call_count(calls: &RecordedCalls) -> usize {
        calls.lock().unwrap().len()
    }

    #[test]
    fn compute_no_lsp_language_no_lsp_queued() {
        let (mut engine, calls) = make_engine();
        let path = Path::new("README.md");
        engine.compute(path, "# Hello\n\nSome text.");
        assert_eq!(call_count(&calls), 1, "one synchronous delivery");
        // Wait beyond the debounce window — no LSP request was queued for Markdown
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            call_count(&calls),
            1,
            "no worker delivery for has_lsp: false language"
        );
    }

    #[test]
    fn compute_ts_cache_hit() {
        let (mut engine, calls) = make_engine();
        let path = Path::new("main.rs");
        let content = "fn main() {}";

        let before = PARSE_COUNT.with(|c| c.get());
        engine.compute(path, content);
        let after_first = PARSE_COUNT.with(|c| c.get());
        engine.compute(path, content);
        let after_second = PARSE_COUNT.with(|c| c.get());

        assert_eq!(
            after_first - before,
            1,
            "first compute parses via tree-sitter"
        );
        assert_eq!(
            after_second - after_first,
            0,
            "second compute with same content hits cache"
        );
        assert_eq!(call_count(&calls), 2, "both computes deliver tokens");
    }

    #[test]
    fn compute_cache_miss_on_content_change() {
        let (mut engine, calls) = make_engine();
        let path = Path::new("main.rs");

        let before = PARSE_COUNT.with(|c| c.get());
        engine.compute(path, "fn main() {}");
        engine.compute(path, "fn other() {}");
        let after = PARSE_COUNT.with(|c| c.get());

        assert_eq!(
            after - before,
            2,
            "content change triggers new tree-sitter parse"
        );
        assert_eq!(call_count(&calls), 2);

        let guard = calls.lock().unwrap();
        // The two synchronous deliveries should carry different tokens
        assert_ne!(
            guard[0].1.len(),
            0,
            "first content should produce ts tokens"
        );
    }

    #[test]
    fn compute_returns_immediately() {
        let (mut engine, calls) = make_engine();
        let path = Path::new("main.rs");

        let start = std::time::Instant::now();
        engine.compute(path, "fn main() {}");
        let elapsed = start.elapsed();

        // set_semantic_tokens called synchronously within compute()
        assert_eq!(call_count(&calls), 1);
        assert!(
            elapsed < Duration::from_millis(10),
            "compute took {:?}, expected < 10ms",
            elapsed
        );
    }

    #[test]
    fn debounce_coalesces_rapid_calls() {
        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let counter_frontend = Arc::new({
            struct Counter(Arc<Mutex<usize>>);
            impl SyntaxFrontend for Counter {
                fn set_semantic_tokens(&self, _: &Path, _: Vec<SemanticToken>) {
                    *self.0.lock().unwrap() += 1;
                }
            }
            Counter(cc)
        });

        let lsp = Arc::new(Mutex::new(LspEngine::new(LspEngineConfig::default())));
        let mut engine = SyntaxEngine::new(lsp, counter_frontend as Arc<dyn SyntaxFrontend>);

        let path = Path::new("main.rs");
        let content = "fn main() {}";
        for _ in 0..10 {
            engine.compute(path, content);
        }

        let sync_deliveries = *call_count.lock().unwrap();
        assert_eq!(sync_deliveries, 10, "each compute fires one sync delivery");

        // Wait for debounce + LSP (fails gracefully) → worker fires once
        std::thread::sleep(Duration::from_millis(600));

        let total = *call_count.lock().unwrap();
        assert_eq!(total, 11, "worker should fire exactly once after debounce");
    }

    #[test]
    fn staleness_check_discards_outdated_lsp_tokens() {
        use crate::data::lsp::types::SemanticToken as ST;

        // Strategy: inject slow LSP tokens (400ms delay). After the debounce
        // window (300ms), the worker calls semantic_tokens and SLEEPS for 400ms.
        // During that sleep the test thread calls compute(B), updating
        // content_hashes to hash(B). When the worker wakes and checks staleness,
        // hash(A) ≠ hash(B) → SKIP. Only the two sync deliveries occur.
        let path = std::path::PathBuf::from("staleness_test.rs");

        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_semantic_tokens(
            path.clone(),
            vec![ST {
                line: 0,
                start_col: 0,
                length: 2,
                token_type: "keyword".to_string(),
            }],
        );
        lsp.test_semantic_tokens_delay = Some(Duration::from_millis(400));

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let counter_frontend = Arc::new({
            struct Counter(Arc<Mutex<usize>>);
            impl SyntaxFrontend for Counter {
                fn set_semantic_tokens(&self, _: &Path, _: Vec<SemanticToken>) {
                    *self.0.lock().unwrap() += 1;
                }
            }
            Counter(cc)
        });

        let lsp_arc = Arc::new(Mutex::new(lsp));
        let mut engine = SyntaxEngine::new(
            Arc::clone(&lsp_arc),
            counter_frontend as Arc<dyn SyntaxFrontend>,
        );

        // compute(A): queued, worker starts 300ms debounce
        engine.compute(path.as_path(), "fn foo() {}");
        assert_eq!(
            *call_count.lock().unwrap(),
            1,
            "sync delivery for content A"
        );

        // Wait for debounce to expire so the worker starts the slow LSP call
        std::thread::sleep(Duration::from_millis(350));

        // compute(B): worker is now sleeping inside semantic_tokens (400ms delay).
        // This updates content_hashes to hash(B), and req(B) is queued.
        engine.compute(path.as_path(), "fn bar() {}");
        assert_eq!(
            *call_count.lock().unwrap(),
            2,
            "sync delivery for content B"
        );

        // Timeline after compute(B) at t=350ms:
        //   t≈700ms  – slow LSP for req(A) returns; staleness check → SKIP
        //   t≈700ms  – worker picks up req(B), 300ms debounce fires at t≈1000ms
        //   t≈1400ms – slow LSP for req(B) returns; staleness OK → delivery #3
        // Sleep 2100ms after compute(B) (total ~2450ms) to ensure delivery #3 has fired.
        std::thread::sleep(Duration::from_millis(2100));

        // delivery #3 comes from req(B) processed correctly; req(A) was discarded
        let final_count = *call_count.lock().unwrap();
        assert_eq!(
            final_count, 3,
            "req(A) stale → skip; req(B) not stale → deliver; total = 3"
        );
    }

    #[test]
    fn latest_wins_during_slow_lsp_call() {
        // True latest-wins: while the worker is inside a slow semantic_tokens
        // call for content A, two more computes happen (B then C). Both B and C
        // are submitted to the slot; with latest-wins semantics, C overwrites B
        // before the worker takes them out. After A's call returns (stale →
        // skip), the worker picks up C directly — B is never processed.
        use crate::data::lsp::types::SemanticToken as ST;
        let path = std::path::PathBuf::from("latest_wins.rs");

        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_semantic_tokens(
            path.clone(),
            vec![ST {
                line: 0,
                start_col: 0,
                length: 2,
                token_type: "keyword".to_string(),
            }],
        );
        lsp.test_semantic_tokens_delay = Some(Duration::from_millis(500));

        let call_count = Arc::new(Mutex::new(0usize));
        let cc = Arc::clone(&call_count);
        let counter_frontend = Arc::new({
            struct Counter(Arc<Mutex<usize>>);
            impl SyntaxFrontend for Counter {
                fn set_semantic_tokens(&self, _: &Path, _: Vec<SemanticToken>) {
                    *self.0.lock().unwrap() += 1;
                }
            }
            Counter(cc)
        });

        let lsp_arc = Arc::new(Mutex::new(lsp));
        let mut engine = SyntaxEngine::new(
            Arc::clone(&lsp_arc),
            counter_frontend as Arc<dyn SyntaxFrontend>,
        );

        // compute(A): sync delivery #1, worker debounces 300ms, then starts
        // a 500ms LSP call at t≈300ms (finishes at t≈800ms).
        engine.compute(path.as_path(), "fn a() {}");
        assert_eq!(*call_count.lock().unwrap(), 1);

        // Wait past the debounce so the worker is mid-LSP-call.
        std::thread::sleep(Duration::from_millis(400));

        // compute(B) then compute(C) — both arrive while worker is in LSP call.
        // With latest-wins, C overwrites B in the slot before the worker
        // takes them. The worker should pick up C (not B) next.
        engine.compute(path.as_path(), "fn b() {}");
        engine.compute(path.as_path(), "fn c() {}");
        assert_eq!(*call_count.lock().unwrap(), 3, "three sync deliveries");

        // Timeline:
        //   t≈800ms  – LSP(A) returns; content_hash is now hash(C) → SKIP
        //   t≈800ms  – worker takes C, debounces 300ms (t≈1100ms)
        //   t≈1600ms – LSP(C) returns; staleness OK → delivery #4
        // Sleep 1500ms after compute(C) (total ~1900ms after start).
        std::thread::sleep(Duration::from_millis(1500));

        let total = *call_count.lock().unwrap();
        assert_eq!(
            total, 4,
            "exactly one worker delivery (for C); B was overwritten before worker took it"
        );
    }
}
