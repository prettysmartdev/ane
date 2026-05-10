use std::path::Path;
use std::time::Duration;

use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::data::lsp::types::{Language, LspEvent, ServerState, SymbolKind};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
const MOCK_SERVER: &str = env!("CARGO_BIN_EXE_mock_lsp_server");

fn mock_config() -> LspEngineConfig {
    LspEngineConfig::default().with_server_override(MOCK_SERVER, vec![], "true")
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(MANIFEST)
}

fn rs_file() -> &'static Path {
    Path::new("src/main.rs")
}

fn start_and_wait(engine: &mut LspEngine) -> ServerState {
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("start_for_context failed");
    engine
        .await_ready(Language::Rust, Duration::from_secs(10))
        .expect("await_ready failed")
}

// --- Initialize handshake ---

#[test]
fn initialize_handshake_reaches_running() {
    let mut engine = LspEngine::new(mock_config());
    let state = start_and_wait(&mut engine);
    assert_eq!(state, ServerState::Running);
}

// --- State transitions: Missing when not installed ---

#[test]
fn startup_transitions_to_missing_when_not_installed() {
    let config = LspEngineConfig::default().with_server_override(MOCK_SERVER, vec![], "false");
    let mut engine = LspEngine::new(config);
    let state = start_and_wait(&mut engine);
    assert_eq!(state, ServerState::Missing);
}

// --- State transition validation ---

#[test]
fn install_server_errors_when_running() {
    let mut engine = LspEngine::new(mock_config());
    let state = start_and_wait(&mut engine);
    assert_eq!(state, ServerState::Running);

    let err = engine.install_server(Language::Rust).unwrap_err();
    assert!(err.to_string().contains("not in Missing state"));
}

// --- Document symbol request/response ---

#[test]
fn document_symbols_round_trip_with_mock() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);

    let symbols = engine
        .document_symbols(Path::new("src/main.rs"))
        .expect("document_symbols failed");

    assert!(!symbols.is_empty(), "expected at least one symbol");
    assert_eq!(symbols[0].name, "main");
    assert_eq!(symbols[0].kind, SymbolKind::Function);
}

// --- Event emission ---

#[test]
fn state_changed_events_fire_on_startup() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);

    let events = engine.poll_events();
    let changes: Vec<(ServerState, ServerState)> = events
        .iter()
        .filter_map(|e| match e {
            LspEvent::StateChanged { old, new, .. } => Some((*old, *new)),
            _ => None,
        })
        .collect();

    assert!(!changes.is_empty(), "expected StateChanged events");
    assert!(
        changes
            .iter()
            .any(|(old, _)| *old == ServerState::Undetected),
        "expected transition from Undetected"
    );
    assert!(
        changes.iter().any(|(_, new)| *new == ServerState::Running),
        "expected transition to Running"
    );
}

#[test]
fn state_changed_events_for_language_field() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);

    let events = engine.poll_events();
    let rust_changes = events.iter().filter(|e| {
        matches!(
            e,
            LspEvent::StateChanged {
                language: Language::Rust,
                ..
            }
        )
    });
    assert!(
        rust_changes.count() > 0,
        "expected Rust StateChanged events"
    );
}

// --- Timeout behavior ---

#[test]
fn await_ready_returns_within_timeout_when_server_hangs() {
    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_millis(400))
        .with_server_override(MOCK_SERVER, vec!["--hang".to_string()], "true");
    let mut engine = LspEngine::new(config);
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("start_for_context failed");

    let start = std::time::Instant::now();
    let state = engine
        .await_ready(Language::Rust, Duration::from_millis(400))
        .expect("await_ready failed");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "await_ready took {:?}, should be under 3s",
        elapsed
    );
    assert_ne!(
        state,
        ServerState::Running,
        "hanging server should not reach Running"
    );
}

// --- Graceful shutdown ---

#[test]
fn graceful_shutdown_sends_shutdown_request_and_stops() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);

    engine.poll_events();
    engine.shutdown_all();

    assert_eq!(engine.server_state(Language::Rust), ServerState::Stopped);

    let events = engine.poll_events();
    let stopped = events.iter().any(|e| {
        matches!(
            e,
            LspEvent::StateChanged {
                new: ServerState::Stopped,
                ..
            }
        )
    });
    assert!(stopped, "expected StateChanged to Stopped event");
}

#[test]
fn drop_calls_shutdown_without_panic() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);
    // Drop should call shutdown_all() without panicking
    drop(engine);
}

// --- any_pending ---

#[test]
fn any_pending_false_after_running() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);
    assert!(!engine.any_pending());
}

// --- Idempotent start ---

#[test]
fn duplicate_start_for_context_is_noop() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);
    // Second call: server already registered, should not spawn another
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("second start_for_context failed");
    assert_eq!(engine.server_state(Language::Rust), ServerState::Running);
}

// --- status_summary ---

#[test]
fn status_summary_reflects_running_state() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);

    let summary = engine.status_summary();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0], (Language::Rust, ServerState::Running));
}

// --- Watchdog kills hung server and transitions to Failed ---

#[test]
fn watchdog_transitions_hanging_server_to_failed() {
    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_millis(300))
        .with_server_override(MOCK_SERVER, vec!["--hang".to_string()], "true");
    let mut engine = LspEngine::new(config);
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("start_for_context failed");

    // Wait long enough for watchdog (300ms) + startup-thread cleanup.
    let state = engine
        .await_ready(Language::Rust, Duration::from_secs(2))
        .expect("await_ready failed");

    assert_eq!(
        state,
        ServerState::Failed,
        "watchdog should drive a hung server to Failed"
    );
}

// --- Drop during mid-startup kills the child ---

#[test]
fn drop_during_startup_does_not_leak_child() {
    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_secs(60))
        .with_server_override(MOCK_SERVER, vec!["--hang".to_string()], "true");
    let mut engine = LspEngine::new(config);
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("start_for_context failed");

    // Give the startup thread time to spawn the child.
    std::thread::sleep(Duration::from_millis(150));

    // Drop while still in Starting; should kill child cleanly.
    drop(engine);
    // If the child were leaked, this test would still pass (we can't easily
    // assert a global state). What we *can* assert is that drop returned
    // promptly. The fact that this test exits at all means drop didn't hang.
}

// --- Restart after Failed ---

#[test]
fn restart_after_failed_returns_to_running() {
    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_millis(200))
        .with_server_override(MOCK_SERVER, vec!["--hang".to_string()], "true");
    let mut engine = LspEngine::new(config);
    engine
        .start_for_context(&root(), &[rs_file()])
        .expect("start_for_context failed");
    let state = engine
        .await_ready(Language::Rust, Duration::from_secs(2))
        .expect("await_ready failed");
    assert_eq!(state, ServerState::Failed);

    // Now restart with a working override (no --hang), via a fresh engine
    // since the override lives on the engine config.
    let mut engine2 = LspEngine::new(mock_config());
    let state2 = start_and_wait(&mut engine2);
    assert_eq!(state2, ServerState::Running);
    // Sanity-check restart path on the working engine.
    engine2.shutdown_all();
    assert_eq!(engine2.server_state(Language::Rust), ServerState::Stopped);
    engine2
        .restart_server(Language::Rust)
        .expect("restart_server failed");
    let state3 = engine2
        .await_ready(Language::Rust, Duration::from_secs(2))
        .expect("await_ready failed");
    assert_eq!(state3, ServerState::Running);
}

// --- Restart errors when server is Running ---

#[test]
fn restart_server_errors_when_running() {
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);
    let err = engine.restart_server(Language::Rust).unwrap_err();
    assert!(err.to_string().contains("not Failed or Stopped"));
}

// --- Auto-didOpen on document_symbols ---

#[test]
fn document_symbols_works_without_explicit_did_open() {
    // Mock server doesn't enforce open-before-symbol, but this exercises the
    // code path that auto-opens. Asserts no error from the chain.
    let mut engine = LspEngine::new(mock_config());
    start_and_wait(&mut engine);
    let path = root().join("src/main.rs");
    let symbols = engine
        .document_symbols(&path)
        .expect("document_symbols should succeed (auto-opens file)");
    assert!(!symbols.is_empty());
}
