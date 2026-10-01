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
    fn begin_revision(&self, _path: &Path, _hash: u64) {}
    fn set_buffer_metrics_versioned(&self, _path: &Path, _hash: u64, _loc: usize, _tokens: usize) {}
    fn set_semantic_tokens_versioned(&self, path: &Path, _hash: u64, tokens: Vec<SemanticToken>) {
        self.set_semantic_tokens(path, tokens);
    }
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

    fn defer(&self, request: LspRequest) {
        let mut state = self.inner.lock().unwrap();
        if state.request.is_none() && !state.shutdown {
            state.request = Some(request);
        }
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
    local_slot: Option<Arc<LspRequestSlot>>,
    worker_mode: bool,
    lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
    content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    frontend: Arc<dyn SyntaxFrontend>,
    request_slot: Arc<LspRequestSlot>,
}

pub fn hash_content(content: &str) -> u64 {
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
            local_slot: None,
            worker_mode: false,
            lsp_cache,
            content_hashes,
            frontend,
            request_slot,
        }
    }

    /// Local parsing, merging and counting use their own latest-wins worker,
    /// independent of the LSP worker and its response latency.
    pub fn new_background(
        lsp_engine: Arc<Mutex<LspEngine>>,
        frontend: Arc<dyn SyntaxFrontend>,
    ) -> Self {
        let mut worker = Self::new(lsp_engine, Arc::clone(&frontend));
        worker.worker_mode = true;
        let slot = Arc::new(LspRequestSlot::new());
        let shell = Self {
            ts_cache: HashMap::new(),
            local_slot: Some(Arc::clone(&slot)),
            worker_mode: false,
            lsp_cache: Arc::clone(&worker.lsp_cache),
            content_hashes: Arc::clone(&worker.content_hashes),
            frontend,
            request_slot: Arc::clone(&worker.request_slot),
        };
        std::thread::spawn(move || {
            while let Some(mut req) = slot.take() {
                while let Some(newer) = slot.wait_for_newer(Duration::from_millis(10)) {
                    req = newer;
                }
                if slot.is_shutdown() {
                    break;
                }
                if worker.content_hashes.lock().unwrap().get(&req.path) != Some(&req.content_hash) {
                    continue;
                }
                worker.compute(&req.path, &req.content);
                let loc = req
                    .content
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count();
                #[cfg(feature = "frontends")]
                let count = {
                    let _timing = crate::commands::diagnostics::Timing::new("token_count");
                    tiktoken::get_encoding("o200k_base")
                        .expect("built-in encoding")
                        .count(&req.content)
                };
                #[cfg(not(feature = "frontends"))]
                let count = 0;
                worker.frontend.set_buffer_metrics_versioned(
                    &req.path,
                    req.content_hash,
                    loc,
                    count,
                );
            }
        });
        shell
    }

    pub fn is_background(&self) -> bool {
        self.local_slot.is_some()
    }

    /// Runs cached local highlighting synchronously, then
    /// queues a debounced LSP token request on the background worker.
    pub fn compute(&mut self, path: &Path, content: &str) {
        let content_hash = hash_content(content);
        if !self.worker_mode {
            self.frontend.begin_revision(path, content_hash);
            self.content_hashes
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), content_hash);
        }
        if let Some(slot) = &self.local_slot {
            slot.submit(LspRequest {
                path: path.to_path_buf(),
                content: content.to_owned(),
                content_hash,
                ts_tokens: Vec::new(),
            });
            return;
        }
        let _timing = crate::commands::diagnostics::Timing::new("syntax_compute");
        let lang = match Language::from_path(path) {
            Some(l) => l,
            None => {
                // Unknown extension: clear any previous tokens so the
                // frontend renders plain text. Matches spec edge case
                // "Language with no tree-sitter and no LSP".
                self.frontend.set_semantic_tokens_versioned(
                    path,
                    hash_content(content),
                    Vec::new(),
                );
                return;
            }
        };
        let caps = lang.capabilities();
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
        self.frontend
            .set_semantic_tokens_versioned(path, content_hash, merged);

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
            let mut client = engine.lock().unwrap().request_client();
            if client.interactive_pending() {
                slot.defer(req);
                continue;
            }
            if content_hashes.lock().unwrap().get(&req.path) != Some(&req.content_hash) {
                continue;
            }
            let lsp_tokens = client
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
            frontend.set_semantic_tokens_versioned(&req.path, req.content_hash, merged);
        }
    }
}

impl Drop for SyntaxEngine {
    fn drop(&mut self) {
        // Wake the worker thread so it can exit instead of blocking forever
        // on the slot's condvar.
        self.request_slot.signal_shutdown();
        if let Some(slot) = &self.local_slot {
            slot.signal_shutdown();
        }
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

    fn assert_latest_semantic_delivery(queued: &[&str]) {
        use crate::commands::lsp_engine::SemanticTestGate;
        use std::sync::mpsc;

        let path = PathBuf::from("latest_wins.rs");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (delivery_tx, delivery_rx) = mpsc::channel();
        struct Counter(mpsc::Sender<()>);
        impl SyntaxFrontend for Counter {
            fn set_semantic_tokens(&self, _: &Path, _: Vec<SemanticToken>) {
                self.0.send(()).unwrap();
            }
        }
        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_semantic_tokens(
            path.clone(),
            vec![SemanticToken {
                line: 0,
                start_col: 0,
                length: 2,
                token_type: "keyword".into(),
            }],
        );
        lsp.test_semantic_gate = Some(Arc::new(SemanticTestGate {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }));
        let mut engine =
            SyntaxEngine::new(Arc::new(Mutex::new(lsp)), Arc::new(Counter(delivery_tx)));
        let timeout = Duration::from_secs(5);
        engine.compute(&path, "fn a() {}");
        delivery_rx.recv_timeout(timeout).unwrap(); // synchronous A
        entered_rx.recv_timeout(timeout).unwrap(); // semantic A is blocked
        for content in queued {
            engine.compute(&path, content);
            delivery_rx.recv_timeout(timeout).unwrap(); // synchronous edit
        }
        release_tx.send(()).unwrap();
        entered_rx.recv_timeout(timeout).unwrap(); // newest snapshot only
        assert!(
            delivery_rx.try_recv().is_err(),
            "stale A must not be delivered"
        );
        release_tx.send(()).unwrap();
        delivery_rx.recv_timeout(timeout).unwrap(); // newest semantic result
        assert!(delivery_rx.try_recv().is_err());
    }

    #[test]
    fn staleness_check_discards_outdated_lsp_tokens() {
        assert_latest_semantic_delivery(&["fn b() {}"]);
    }

    #[test]
    fn latest_wins_during_slow_lsp_call() {
        assert_latest_semantic_delivery(&["fn b() {}", "fn c() {}"]);
    }

    #[test]
    fn compute_no_lsp_for_config_languages() {
        let (mut engine, calls) = make_engine();

        let cases: &[(&str, &str)] = &[
            ("config.json", r#"{"x": 1}"#),
            ("config.yaml", "x: 1\n"),
            ("config.toml", "x = 1\n"),
            ("Dockerfile", "FROM ubuntu:22.04\n"),
            ("schema.xml", "<r/>"),
        ];

        for (filename, content) in cases {
            engine.compute(Path::new(filename), content);
        }

        assert_eq!(
            call_count(&calls),
            5,
            "one synchronous delivery per config file"
        );

        {
            let guard = calls.lock().unwrap();
            for (i, (_path, tokens)) in guard.iter().enumerate() {
                assert!(
                    !tokens.is_empty(),
                    "case {i}: expected non-empty tree-sitter tokens"
                );
            }
        }

        // Wait past the debounce window — no LSP worker delivery for has_lsp: false languages
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            call_count(&calls),
            5,
            "no additional worker delivery for config languages (has_lsp: false)"
        );
    }
    #[test]
    fn background_local_worker_keeps_latest_revision_without_blocking_submission() {
        use std::sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc,
        };
        struct Gated {
            current: AtomicU64,
            first: AtomicBool,
            entered: mpsc::SyncSender<()>,
            release: Mutex<mpsc::Receiver<()>>,
            metrics: mpsc::SyncSender<u64>,
        }
        impl SyntaxFrontend for Gated {
            fn set_semantic_tokens(&self, _: &Path, _: Vec<SemanticToken>) {}
            fn begin_revision(&self, _: &Path, hash: u64) {
                self.current.store(hash, Ordering::Release);
            }
            fn set_semantic_tokens_versioned(&self, _: &Path, _: u64, _: Vec<SemanticToken>) {
                if self.first.swap(false, Ordering::AcqRel) {
                    self.entered.send(()).unwrap();
                    self.release
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
            }
            fn set_buffer_metrics_versioned(&self, _: &Path, hash: u64, _: usize, _: usize) {
                if self.current.load(Ordering::Acquire) == hash {
                    self.metrics.send(hash).unwrap();
                }
            }
        }
        let (entered, started) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let (metrics, delivered) = mpsc::sync_channel(1);
        let frontend = Arc::new(Gated {
            current: AtomicU64::new(0),
            first: AtomicBool::new(true),
            entered,
            release: Mutex::new(gate),
            metrics,
        });
        let engine = Arc::new(Mutex::new(LspEngine::new(LspEngineConfig::default())));
        let mut syntax = SyntaxEngine::new_background(engine, frontend);
        let path = Path::new("latest.json");
        syntax.compute(path, "{\"value\":1}");
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        syntax.compute(path, "{\"value\":2}");
        syntax.compute(path, "{\"value\":3}");
        release.send(()).unwrap();
        assert_eq!(
            delivered.recv_timeout(Duration::from_secs(5)).unwrap(),
            super::hash_content("{\"value\":3}")
        );
        assert!(delivered.try_recv().is_err());
    }
}
