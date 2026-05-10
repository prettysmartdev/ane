use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::data::lsp::registry;
use crate::data::lsp::types::{
    CompletionItem, DocumentSymbol, HoverInfo, Language, Location, LspEvent, LspServerInfo,
    ServerState, SymbolKind, SymbolRange,
};

use super::detector;
use super::health::{self, HealthStatus};
use super::installer;
use super::transport::LspTransport;

const SHUTDOWN_GRACE: Duration = Duration::from_millis(1_500);

pub struct LspEngineConfig {
    pub auto_install: bool,
    pub startup_timeout: Duration,
    binary_name_override: Option<String>,
    binary_args_override: Vec<String>,
    check_command_override: Option<String>,
}

impl Default for LspEngineConfig {
    fn default() -> Self {
        Self {
            auto_install: true,
            startup_timeout: Duration::from_secs(30),
            binary_name_override: None,
            binary_args_override: Vec::new(),
            check_command_override: None,
        }
    }
}

impl LspEngineConfig {
    pub fn with_startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    pub fn with_auto_install(mut self, auto: bool) -> Self {
        self.auto_install = auto;
        self
    }

    pub fn with_server_override(
        mut self,
        binary: impl Into<String>,
        args: Vec<String>,
        check_cmd: impl Into<String>,
    ) -> Self {
        self.binary_name_override = Some(binary.into());
        self.binary_args_override = args;
        self.check_command_override = Some(check_cmd.into());
        self
    }
}

#[derive(Clone)]
struct ServerOverrides {
    binary_name: Option<String>,
    binary_args: Vec<String>,
    check_command: Option<String>,
}

struct StartupContext {
    lang: Language,
    server_info: &'static LspServerInfo,
    root_path: PathBuf,
    auto_install: bool,
    overrides: ServerOverrides,
    startup_timeout: Duration,
}

struct ServerInstance {
    state: Arc<(Mutex<ServerState>, Condvar)>,
    server_info: &'static LspServerInfo,
    /// Shared with startup thread and watchdog so any of them can kill the child.
    child: Arc<Mutex<Option<Child>>>,
    transport: Option<LspTransport>,
    startup_rx: Option<mpsc::Receiver<LspTransport>>,
    opened_files: HashSet<PathBuf>,
    root_path: PathBuf,
}

impl ServerInstance {
    fn get_state(&self) -> ServerState {
        *self.state.0.lock().unwrap()
    }
}

/// Atomically attempt a state transition. Returns the previous state if the
/// transition was permitted, or `None` if it was rejected. Emits the
/// `StateChanged` event on success.
fn try_transition(
    state: &Arc<(Mutex<ServerState>, Condvar)>,
    lang: Language,
    new_state: ServerState,
    event_tx: &mpsc::Sender<LspEvent>,
) -> Option<ServerState> {
    let (lock, cvar) = &**state;
    let mut s = lock.lock().unwrap();
    let old = *s;
    if !old.can_transition_to(new_state) {
        return None;
    }
    *s = new_state;
    drop(s);
    cvar.notify_all();
    let _ = event_tx.send(LspEvent::StateChanged {
        language: lang,
        old,
        new: new_state,
    });
    Some(old)
}

pub struct LspEngine {
    servers: HashMap<Language, ServerInstance>,
    config: LspEngineConfig,
    event_tx: mpsc::Sender<LspEvent>,
    event_rx: mpsc::Receiver<LspEvent>,
    #[cfg(test)]
    test_symbols: HashMap<PathBuf, Vec<DocumentSymbol>>,
}

impl LspEngine {
    pub fn new(config: LspEngineConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            servers: HashMap::new(),
            config,
            event_tx,
            event_rx,
            #[cfg(test)]
            test_symbols: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn inject_test_symbols(&mut self, path: PathBuf, symbols: Vec<DocumentSymbol>) {
        self.test_symbols.insert(path, symbols);
    }

    pub fn start_for_context(&mut self, root_path: &Path, files: &[&Path]) -> Result<()> {
        let languages = detector::detect_languages(root_path, files);

        for lang in languages {
            if self.servers.contains_key(&lang) {
                continue;
            }

            let server_info = match registry::server_for_language(lang) {
                Some(info) => info,
                None => continue,
            };

            // Resolve to the workspace root so rust-analyzer indexes the
            // whole workspace, not a nested member crate.
            let resolved_root = registry::workspace_root_for_dir(root_path)
                .unwrap_or_else(|| root_path.to_path_buf());
            self.spawn_server(lang, server_info, resolved_root);
        }

        Ok(())
    }

    fn spawn_server(
        &mut self,
        lang: Language,
        server_info: &'static LspServerInfo,
        root_path: PathBuf,
    ) {
        let state = Arc::new((Mutex::new(ServerState::Undetected), Condvar::new()));
        let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let (transport_tx, transport_rx) = mpsc::channel();
        let event_tx = self.event_tx.clone();
        let state_clone = Arc::clone(&state);
        let child_clone = Arc::clone(&child);
        let ctx = StartupContext {
            lang,
            server_info,
            root_path: root_path.clone(),
            auto_install: self.config.auto_install,
            overrides: ServerOverrides {
                binary_name: self.config.binary_name_override.clone(),
                binary_args: self.config.binary_args_override.clone(),
                check_command: self.config.check_command_override.clone(),
            },
            startup_timeout: self.config.startup_timeout,
        };

        thread::spawn(move || {
            startup_thread(ctx, state_clone, child_clone, transport_tx, event_tx);
        });

        self.servers.insert(
            lang,
            ServerInstance {
                state,
                server_info,
                child,
                transport: None,
                startup_rx: Some(transport_rx),
                opened_files: HashSet::new(),
                root_path,
            },
        );
    }

    pub fn server_state(&self, lang: Language) -> ServerState {
        self.servers
            .get(&lang)
            .map(|s| s.get_state())
            .unwrap_or(ServerState::Undetected)
    }

    pub fn await_ready(&mut self, lang: Language, timeout: Duration) -> Result<ServerState> {
        let state_arc = {
            let server = self
                .servers
                .get(&lang)
                .ok_or_else(|| anyhow::anyhow!("no server registered for {:?}", lang))?;
            Arc::clone(&server.state)
        };

        let (lock, cvar) = &*state_arc;
        let started = Instant::now();
        let mut state = lock.lock().unwrap();

        while !state.is_terminal() {
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                break;
            }
            let remaining = timeout - elapsed;
            let (new_state, wait_result) = cvar.wait_timeout(state, remaining).unwrap();
            state = new_state;
            if wait_result.timed_out() {
                break;
            }
        }
        let final_state = *state;
        drop(state);

        if final_state == ServerState::Running {
            self.try_recv_startup(lang);
        }

        Ok(final_state)
    }

    pub fn any_pending(&self) -> bool {
        self.servers.values().any(|s| s.get_state().is_pending())
    }

    pub fn poll_events(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn shutdown_all(&mut self) {
        let languages: Vec<Language> = self.servers.keys().copied().collect();
        for lang in languages {
            self.shutdown_server(lang);
        }
    }

    pub fn install_server(&mut self, lang: Language) -> Result<()> {
        let event_tx = self.event_tx.clone();

        let server = self
            .servers
            .get(&lang)
            .ok_or_else(|| anyhow::anyhow!("no server registered for {:?}", lang))?;

        let current = server.get_state();
        if current != ServerState::Missing {
            bail!(
                "server for {:?} is not in Missing state (current: {:?})",
                lang,
                current
            );
        }

        let server_info = server.server_info;
        let state = Arc::clone(&server.state);

        if try_transition(&state, lang, ServerState::Installing, &event_tx).is_none() {
            bail!("could not transition to Installing");
        }

        match installer::install(server_info) {
            Ok(()) => {
                try_transition(&state, lang, ServerState::Available, &event_tx);
                Ok(())
            }
            Err(e) => {
                try_transition(&state, lang, ServerState::Failed, &event_tx);
                let _ = event_tx.send(LspEvent::Error {
                    language: lang,
                    error: e.to_string(),
                });
                Err(e)
            }
        }
    }

    /// Restart a server that has Failed (or Stopped). Tears down any lingering
    /// process, then re-runs the full startup pipeline.
    pub fn restart_server(&mut self, lang: Language) -> Result<()> {
        let (server_info, root_path) = {
            let server = self
                .servers
                .get(&lang)
                .ok_or_else(|| anyhow::anyhow!("no server registered for {:?}", lang))?;
            let s = server.get_state();
            if !matches!(s, ServerState::Failed | ServerState::Stopped) {
                bail!(
                    "server for {:?} is not Failed or Stopped (current: {:?})",
                    lang,
                    s
                );
            }
            (server.server_info, server.root_path.clone())
        };
        self.shutdown_server(lang);
        self.servers.remove(&lang);
        self.spawn_server(lang, server_info, root_path);
        Ok(())
    }

    pub fn status_summary(&self) -> Vec<(Language, ServerState)> {
        self.servers
            .iter()
            .map(|(lang, server)| (*lang, server.get_state()))
            .collect()
    }

    // --- LSP Query Methods ---

    pub fn document_symbols(&mut self, file_path: &Path) -> Result<Vec<DocumentSymbol>> {
        #[cfg(test)]
        if let Some(syms) = self.test_symbols.get(file_path) {
            return Ok(syms.clone());
        }
        let lang = self.language_for_file(file_path)?;
        self.ensure_open(lang, file_path)?;
        let uri = path_to_uri(file_path);
        let params = json!({ "textDocument": { "uri": uri } });
        let result = self.send_request(lang, "textDocument/documentSymbol", params)?;
        parse_document_symbols(&result)
    }

    pub fn symbol_at_position(
        &mut self,
        file_path: &Path,
        line: usize,
        col: usize,
    ) -> Result<Option<DocumentSymbol>> {
        let symbols = self.document_symbols(file_path)?;
        Ok(find_symbol_at_position(&symbols, line, col))
    }

    pub fn symbol_range(
        &mut self,
        file_path: &Path,
        symbol_name: &str,
    ) -> Result<Option<SymbolRange>> {
        let symbols = self.document_symbols(file_path)?;
        Ok(find_symbol_by_name(&symbols, symbol_name).map(|s| s.range))
    }

    pub fn notify_document_open(&mut self, file_path: &Path, content: &str) -> Result<()> {
        let lang = self.language_for_file(file_path)?;
        self.do_notify_open(lang, file_path, content)?;
        if let Some(server) = self.servers.get_mut(&lang) {
            server.opened_files.insert(file_path.to_path_buf());
        }
        Ok(())
    }

    pub fn notify_document_change(
        &mut self,
        file_path: &Path,
        content: &str,
        version: i32,
    ) -> Result<()> {
        let lang = self.language_for_file(file_path)?;
        self.ensure_open(lang, file_path)?;
        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": content }]
        });
        self.send_notification(lang, "textDocument/didChange", params)
    }

    pub fn completions(
        &mut self,
        file_path: &Path,
        line: usize,
        col: usize,
    ) -> Result<Vec<CompletionItem>> {
        let lang = self.language_for_file(file_path)?;
        self.ensure_open(lang, file_path)?;
        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });
        let result = self.send_request(lang, "textDocument/completion", params)?;
        parse_completions(&result)
    }

    pub fn hover(
        &mut self,
        file_path: &Path,
        line: usize,
        col: usize,
    ) -> Result<Option<HoverInfo>> {
        let lang = self.language_for_file(file_path)?;
        self.ensure_open(lang, file_path)?;
        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });
        let result = self.send_request(lang, "textDocument/hover", params)?;
        Ok(parse_hover(&result))
    }

    pub fn goto_definition(
        &mut self,
        file_path: &Path,
        line: usize,
        col: usize,
    ) -> Result<Option<Location>> {
        let lang = self.language_for_file(file_path)?;
        self.ensure_open(lang, file_path)?;
        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });
        let result = self.send_request(lang, "textDocument/definition", params)?;
        Ok(parse_location(&result))
    }

    // --- Internal Methods ---

    fn try_recv_startup(&mut self, lang: Language) {
        if let Some(server) = self.servers.get_mut(&lang) {
            if let Some(ref rx) = server.startup_rx {
                match rx.try_recv() {
                    Ok(transport) => {
                        server.transport = Some(transport);
                        server.startup_rx = None;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        server.startup_rx = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        }
    }

    fn language_for_file(&self, file_path: &Path) -> Result<Language> {
        registry::detect_language_from_path(file_path)
            .ok_or_else(|| anyhow::anyhow!("no language detected for {}", file_path.display()))
    }

    /// Send a request, surfacing transport errors and translating crashes into
    /// a `Failed` state transition + event before returning the error.
    fn send_request(&mut self, lang: Language, method: &str, params: Value) -> Result<Value> {
        self.try_recv_startup(lang);
        let result = {
            let transport = self.get_transport(lang)?;
            transport.send_request(method, params)
        };
        if result.is_err() {
            self.detect_and_report_crash(lang);
        }
        result
    }

    fn send_notification(&mut self, lang: Language, method: &str, params: Value) -> Result<()> {
        self.try_recv_startup(lang);
        let result = {
            let transport = self.get_transport(lang)?;
            transport.send_notification(method, params)
        };
        if result.is_err() {
            self.detect_and_report_crash(lang);
        }
        result
    }

    fn get_transport(&mut self, lang: Language) -> Result<&mut LspTransport> {
        // Quick liveness check first; drop the immutable borrow before reporting.
        let exited = match self.servers.get(&lang) {
            Some(server) => {
                let mut child = server.child.lock().unwrap();
                child
                    .as_mut()
                    .map(|c| matches!(health::check_process(c), HealthStatus::ProcessExited(_)))
                    .unwrap_or(false)
            }
            None => bail!("no server for {:?}", lang),
        };
        if exited {
            let current = self
                .servers
                .get(&lang)
                .map(|s| s.get_state())
                .unwrap_or(ServerState::Failed);
            self.report_crash(lang, current, Some(None));
            bail!("LSP server for {:?} has exited", lang);
        }

        let server = self
            .servers
            .get_mut(&lang)
            .ok_or_else(|| anyhow::anyhow!("no server for {:?}", lang))?;
        let current_state = server.get_state();
        server.transport.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "LSP server for {:?} is not running (state: {:?})",
                lang,
                current_state
            )
        })
    }

    /// Re-check process health after a transport error. If it's dead, transition
    /// to Failed and emit events.
    fn detect_and_report_crash(&mut self, lang: Language) {
        let exited = {
            if let Some(server) = self.servers.get(&lang) {
                let mut child = server.child.lock().unwrap();
                child
                    .as_mut()
                    .map(|c| match health::check_process(c) {
                        HealthStatus::ProcessExited(code) => Some(code),
                        HealthStatus::Healthy => None,
                    })
                    .unwrap_or(Some(None))
            } else {
                return;
            }
        };
        if let Some(code) = exited {
            let current = self.servers.get(&lang).map(|s| s.get_state());
            if let Some(s) = current {
                self.report_crash(lang, s, Some(code));
            }
        } else {
            // Process still alive but transport call failed (e.g., serialization
            // error). Treat as a fatal transport-level error: transition to Failed.
            let current = self.servers.get(&lang).map(|s| s.get_state());
            if let Some(s) = current {
                self.report_crash(lang, s, None);
            }
        }
    }

    fn report_crash(&mut self, lang: Language, current: ServerState, code: Option<Option<i32>>) {
        if let Some(server) = self.servers.get_mut(&lang) {
            let state_arc = Arc::clone(&server.state);
            let event_tx = self.event_tx.clone();
            // Reset transport so subsequent calls clearly fail.
            server.transport = None;
            server.opened_files.clear();
            // Reap the child so we don't leave a zombie.
            if let Some(mut c) = server.child.lock().unwrap().take() {
                let _ = c.wait();
            }
            if !current.is_terminal() || current == ServerState::Running {
                try_transition(&state_arc, lang, ServerState::Failed, &event_tx);
            }
            let msg = match code {
                Some(Some(c)) => format!("LSP process exited with code {}", c),
                Some(None) => "LSP process exited (no status)".to_string(),
                None => "LSP transport failure".to_string(),
            };
            let _ = event_tx.send(LspEvent::Error {
                language: lang,
                error: msg,
            });
        }
    }

    fn shutdown_server(&mut self, lang: Language) {
        let server = match self.servers.get_mut(&lang) {
            Some(s) => s,
            None => return,
        };
        let state_arc = Arc::clone(&server.state);
        let event_tx = self.event_tx.clone();

        // Try a graceful shutdown handshake on a background thread, bounded by
        // SHUTDOWN_GRACE. Take ownership of the transport so the worker thread
        // can drive it without an aliased borrow.
        if let Some(mut transport) = server.transport.take() {
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let handle = thread::spawn(move || {
                let _ = transport.send_request("shutdown", Value::Null);
                let _ = transport.send_notification("exit", Value::Null);
                let _ = done_tx.send(());
            });
            let _ = done_rx.recv_timeout(SHUTDOWN_GRACE);
            // Whether the handshake finished or timed out, we proceed to kill
            // the child. Detach the join handle if not done — the kill below
            // will unblock any pending pipe I/O.
            if handle.is_finished() {
                let _ = handle.join();
            }
        }

        // Take and kill the child process (works even mid-startup).
        if let Some(mut child) = server.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        server.opened_files.clear();
        server.startup_rx = None;

        // Best-effort transition to Stopped from any state. If the validator
        // rejects (already Stopped, etc.) we silently move on.
        try_transition(&state_arc, lang, ServerState::Stopped, &event_tx);
    }

    fn ensure_open(&mut self, lang: Language, file_path: &Path) -> Result<()> {
        let already_open = self
            .servers
            .get(&lang)
            .map(|s| s.opened_files.contains(file_path))
            .unwrap_or(false);
        if already_open {
            return Ok(());
        }
        let content = std::fs::read_to_string(file_path).unwrap_or_default();
        self.do_notify_open(lang, file_path, &content)?;
        if let Some(server) = self.servers.get_mut(&lang) {
            server.opened_files.insert(file_path.to_path_buf());
        }
        Ok(())
    }

    fn do_notify_open(&mut self, lang: Language, file_path: &Path, content: &str) -> Result<()> {
        let uri = path_to_uri(file_path);
        let params = json!({
            "textDocument": {
                "uri": uri,
                "languageId": lang.name(),
                "version": 1,
                "text": content
            }
        });
        self.send_notification(lang, "textDocument/didOpen", params)
    }
}

impl Drop for LspEngine {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

// --- Background Startup Thread ---

fn startup_thread(
    ctx: StartupContext,
    state: Arc<(Mutex<ServerState>, Condvar)>,
    child_slot: Arc<Mutex<Option<Child>>>,
    transport_tx: mpsc::Sender<LspTransport>,
    event_tx: mpsc::Sender<LspEvent>,
) {
    let lang = ctx.lang;
    let server_info = ctx.server_info;

    // Detect installation
    let installed = if let Some(ref check_cmd) = ctx.overrides.check_command {
        Command::new("sh")
            .arg("-c")
            .arg(check_cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        installer::is_installed(server_info)
    };

    if !installed {
        if ctx.auto_install {
            try_transition(&state, lang, ServerState::Installing, &event_tx);
            if let Err(e) = installer::install(server_info) {
                try_transition(&state, lang, ServerState::Failed, &event_tx);
                let _ = event_tx.send(LspEvent::Error {
                    language: lang,
                    error: format!("install failed: {}", e),
                });
                return;
            }
            try_transition(&state, lang, ServerState::Available, &event_tx);
        } else {
            try_transition(&state, lang, ServerState::Missing, &event_tx);
            return;
        }
    } else {
        try_transition(&state, lang, ServerState::Available, &event_tx);
    }

    try_transition(&state, lang, ServerState::Starting, &event_tx);

    // Validate init options up-front so we surface a real error rather than
    // silently sending an empty object.
    let init_options: Value = if server_info.init_options_json.is_empty() {
        Value::Null
    } else {
        match serde_json::from_str(server_info.init_options_json) {
            Ok(v) => v,
            Err(e) => {
                try_transition(&state, lang, ServerState::Failed, &event_tx);
                let _ = event_tx.send(LspEvent::Error {
                    language: lang,
                    error: format!(
                        "invalid init_options_json for {}: {}",
                        server_info.server_name, e
                    ),
                });
                return;
            }
        }
    };

    let binary = ctx
        .overrides
        .binary_name
        .as_deref()
        .unwrap_or(server_info.binary_name);
    let mut cmd = Command::new(binary);
    if ctx.overrides.binary_name.is_some() {
        cmd.args(&ctx.overrides.binary_args);
    } else {
        cmd.args(server_info.default_args);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            try_transition(&state, lang, ServerState::Failed, &event_tx);
            let _ = event_tx.send(LspEvent::Error {
                language: lang,
                error: format!("failed to spawn {}: {}", binary, e),
            });
            return;
        }
    };

    let stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            try_transition(&state, lang, ServerState::Failed, &event_tx);
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            try_transition(&state, lang, ServerState::Failed, &event_tx);
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
    };

    // Stash the child immediately so Drop / shutdown / watchdog can kill it.
    *child_slot.lock().unwrap() = Some(child);

    // Watchdog: after startup_timeout, if state hasn't reached Running, kill
    // the child. The handshake's blocking read will then return EOF.
    let watchdog_state = Arc::clone(&state);
    let watchdog_child = Arc::clone(&child_slot);
    let timeout = ctx.startup_timeout;
    let watchdog_event_tx = event_tx.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        let (lock, cvar) = &*watchdog_state;
        let mut s = lock.lock().unwrap();
        while !matches!(
            *s,
            ServerState::Running | ServerState::Stopped | ServerState::Failed
        ) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (new_s, _) = cvar.wait_timeout(s, deadline - now).unwrap();
            s = new_s;
        }
        let still_pending = !matches!(
            *s,
            ServerState::Running | ServerState::Stopped | ServerState::Failed
        );
        drop(s);
        if still_pending {
            if let Some(mut c) = watchdog_child.lock().unwrap().take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            let _ = watchdog_event_tx.send(LspEvent::Error {
                language: lang,
                error: format!("startup timed out after {:?}", timeout),
            });
        }
    });

    let mut transport = LspTransport::new(stdin, stdout);

    let root_uri = path_to_uri(&ctx.root_path);
    let init_params = json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {
            "textDocument": {
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
            }
        },
        "initializationOptions": init_options
    });

    if let Err(e) = transport.send_request("initialize", init_params) {
        try_transition(&state, lang, ServerState::Failed, &event_tx);
        if let Some(mut c) = child_slot.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = event_tx.send(LspEvent::Error {
            language: lang,
            error: format!("initialize handshake failed: {}", e),
        });
        return;
    }

    if let Err(e) = transport.send_notification("initialized", json!({})) {
        try_transition(&state, lang, ServerState::Failed, &event_tx);
        if let Some(mut c) = child_slot.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = event_tx.send(LspEvent::Error {
            language: lang,
            error: format!("initialized notification failed: {}", e),
        });
        return;
    }

    if transport_tx.send(transport).is_err() {
        // Engine dropped the receiver (engine itself was dropped). Kill child.
        if let Some(mut c) = child_slot.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        return;
    }

    try_transition(&state, lang, ServerState::Running, &event_tx);
}

// --- Utility Functions ---

fn path_to_uri(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let s = abs.to_string_lossy();
    let mut encoded = String::with_capacity(s.len() + 7);
    encoded.push_str("file://");
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(byte as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(&mut encoded, "%{:02X}", byte);
            }
        }
    }
    encoded
}

// --- Response Parsing ---

fn parse_document_symbols(value: &Value) -> Result<Vec<DocumentSymbol>> {
    match value {
        Value::Array(arr) => {
            let mut symbols = Vec::new();
            for item in arr {
                if let Some(sym) = parse_single_symbol(item) {
                    symbols.push(sym);
                }
            }
            Ok(symbols)
        }
        Value::Null => Ok(Vec::new()),
        _ => bail!("unexpected documentSymbol response format"),
    }
}

fn parse_single_symbol(value: &Value) -> Option<DocumentSymbol> {
    let name = value.get("name")?.as_str()?.to_string();
    let kind_num = value.get("kind")?.as_u64()? as u32;
    let range = parse_range(value.get("range")?)?;

    let children = value
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(parse_single_symbol).collect())
        .unwrap_or_default();

    Some(DocumentSymbol {
        name,
        kind: convert_symbol_kind(kind_num),
        range,
        children,
    })
}

fn parse_range(value: &Value) -> Option<SymbolRange> {
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(SymbolRange {
        start_line: start.get("line")?.as_u64()? as usize,
        start_col: start.get("character")?.as_u64()? as usize,
        end_line: end.get("line")?.as_u64()? as usize,
        end_col: end.get("character")?.as_u64()? as usize,
    })
}

fn convert_symbol_kind(kind: u32) -> SymbolKind {
    match kind {
        1 | 2 => SymbolKind::Module,
        5 => SymbolKind::Struct,
        6 => SymbolKind::Method,
        8 => SymbolKind::Field,
        10 => SymbolKind::Enum,
        12 => SymbolKind::Function,
        13 => SymbolKind::Variable,
        14 => SymbolKind::Const,
        23 => SymbolKind::Struct,
        _ => SymbolKind::Other(format!("kind_{}", kind)),
    }
}

fn find_symbol_at_position(
    symbols: &[DocumentSymbol],
    line: usize,
    col: usize,
) -> Option<DocumentSymbol> {
    for sym in symbols {
        if contains_position(&sym.range, line, col) {
            if let Some(child) = find_symbol_at_position(&sym.children, line, col) {
                return Some(child);
            }
            return Some(sym.clone());
        }
    }
    None
}

fn contains_position(range: &SymbolRange, line: usize, col: usize) -> bool {
    if line < range.start_line || line > range.end_line {
        return false;
    }
    if line == range.start_line && col < range.start_col {
        return false;
    }
    if line == range.end_line && col > range.end_col {
        return false;
    }
    true
}

fn find_symbol_by_name<'a>(
    symbols: &'a [DocumentSymbol],
    name: &str,
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if sym.name == name {
            return Some(sym);
        }
        if let Some(found) = find_symbol_by_name(&sym.children, name) {
            return Some(found);
        }
    }
    None
}

fn parse_completions(value: &Value) -> Result<Vec<CompletionItem>> {
    let items = if let Some(arr) = value.as_array() {
        arr
    } else if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
        items
    } else if value.is_null() {
        return Ok(Vec::new());
    } else {
        bail!("unexpected completion response format");
    };

    Ok(items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let detail = item
                .get("detail")
                .and_then(|d| d.as_str())
                .map(String::from);
            let kind = item
                .get("kind")
                .and_then(|k| k.as_u64())
                .map(|k| format!("{}", k));
            Some(CompletionItem {
                label,
                detail,
                kind,
            })
        })
        .collect())
}

fn parse_hover(value: &Value) -> Option<HoverInfo> {
    if value.is_null() {
        return None;
    }

    let contents = value.get("contents")?;
    let text = if let Some(s) = contents.as_str() {
        s.to_string()
    } else if let Some(obj) = contents.as_object() {
        obj.get("value")?.as_str()?.to_string()
    } else if let Some(arr) = contents.as_array() {
        arr.iter()
            .filter_map(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.get("value").and_then(|v| v.as_str()).map(String::from))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        return None;
    };

    Some(HoverInfo { contents: text })
}

fn parse_location(value: &Value) -> Option<Location> {
    if value.is_null() {
        return None;
    }

    let loc = if let Some(arr) = value.as_array() {
        arr.first()?
    } else {
        value
    };

    let uri = loc.get("uri")?.as_str()?;
    let file_path = uri.strip_prefix("file://").map(PathBuf::from)?;
    let range = parse_range(loc.get("range")?)?;

    Some(Location { file_path, range })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn auto_install_true_by_default() {
        assert!(LspEngineConfig::default().auto_install);
    }

    #[test]
    fn server_state_undetected_when_no_server_registered() {
        let engine = LspEngine::new(LspEngineConfig::default());
        assert_eq!(engine.server_state(Language::Rust), ServerState::Undetected);
    }

    #[test]
    fn any_pending_false_with_no_servers() {
        let engine = LspEngine::new(LspEngineConfig::default());
        assert!(!engine.any_pending());
    }

    #[test]
    fn await_ready_errors_for_unregistered_language() {
        let mut engine = LspEngine::new(LspEngineConfig::default());
        let result = engine.await_ready(Language::Rust, Duration::from_millis(100));
        assert!(result.is_err());
    }

    #[test]
    fn install_server_errors_for_unregistered_language() {
        let mut engine = LspEngine::new(LspEngineConfig::default());
        let result = engine.install_server(Language::Rust);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no server registered"));
    }

    #[test]
    fn restart_server_errors_for_unregistered_language() {
        let mut engine = LspEngine::new(LspEngineConfig::default());
        let result = engine.restart_server(Language::Rust);
        assert!(result.is_err());
    }

    #[test]
    fn parse_document_symbols_handles_null() {
        let result = parse_document_symbols(&serde_json::Value::Null).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_document_symbols_handles_array() {
        let json = serde_json::json!([{
            "name": "my_fn",
            "kind": 12,
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 5, "character": 1}
            },
            "children": []
        }]);
        let symbols = parse_document_symbols(&json).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "my_fn");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn parse_document_symbols_rejects_unexpected_format() {
        let result = parse_document_symbols(&serde_json::json!({"not": "an array"}));
        assert!(result.is_err());
    }

    #[test]
    fn symbol_at_position_finds_innermost() {
        let outer = DocumentSymbol {
            name: "outer".to_string(),
            kind: SymbolKind::Function,
            range: SymbolRange {
                start_line: 0,
                start_col: 0,
                end_line: 10,
                end_col: 1,
            },
            children: vec![DocumentSymbol {
                name: "inner".to_string(),
                kind: SymbolKind::Variable,
                range: SymbolRange {
                    start_line: 3,
                    start_col: 4,
                    end_line: 3,
                    end_col: 20,
                },
                children: vec![],
            }],
        };
        let result = find_symbol_at_position(&[outer], 3, 10);
        assert_eq!(result.unwrap().name, "inner");
    }

    #[test]
    fn symbol_at_position_returns_parent_when_no_child_matches() {
        let sym = DocumentSymbol {
            name: "parent".to_string(),
            kind: SymbolKind::Function,
            range: SymbolRange {
                start_line: 0,
                start_col: 0,
                end_line: 10,
                end_col: 1,
            },
            children: vec![],
        };
        let result = find_symbol_at_position(&[sym], 5, 0);
        assert_eq!(result.unwrap().name, "parent");
    }

    #[test]
    fn symbol_at_position_returns_none_outside_all_symbols() {
        let sym = DocumentSymbol {
            name: "fn1".to_string(),
            kind: SymbolKind::Function,
            range: SymbolRange {
                start_line: 0,
                start_col: 0,
                end_line: 5,
                end_col: 1,
            },
            children: vec![],
        };
        let result = find_symbol_at_position(&[sym], 10, 0);
        assert!(result.is_none());
    }

    #[test]
    fn convert_symbol_kind_maps_known_kinds() {
        assert_eq!(convert_symbol_kind(12), SymbolKind::Function);
        assert_eq!(convert_symbol_kind(5), SymbolKind::Struct);
        assert_eq!(convert_symbol_kind(13), SymbolKind::Variable);
        assert_eq!(convert_symbol_kind(10), SymbolKind::Enum);
        assert_eq!(convert_symbol_kind(6), SymbolKind::Method);
        assert_eq!(convert_symbol_kind(8), SymbolKind::Field);
        assert_eq!(convert_symbol_kind(14), SymbolKind::Const);
        assert!(matches!(convert_symbol_kind(99), SymbolKind::Other(_)));
    }

    #[test]
    fn path_to_uri_encodes_spaces_and_specials() {
        let p = std::path::Path::new("/tmp/has space/foo#bar.rs");
        let uri = path_to_uri(p);
        assert!(uri.starts_with("file:///tmp/has%20space/foo%23bar.rs"));
    }

    #[test]
    fn path_to_uri_passes_through_safe_chars() {
        let p = std::path::Path::new("/tmp/normal_path-1.rs");
        let uri = path_to_uri(p);
        assert_eq!(uri, "file:///tmp/normal_path-1.rs");
    }
}
