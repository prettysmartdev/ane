# Work Item: Feature

Title: Tree-sitter + LSP syntax rendering; add Go, TypeScript, Python, Markdown
Issue: N/A

## Summary

Overhaul the syntax highlighting pipeline to use tree-sitter as a universal baseline computed in Layer 1, with LSP semantic tokens as a per-language overlay when a server is running. Introduce a language capability matrix so each language independently declares whether it supports tree-sitter, an LSP server, both, or neither. Add Go, TypeScript, and Python (both tree-sitter and LSP) and Markdown (tree-sitter only, no LSP server). `SyntaxEngine` (Layer 1) owns references to `LspEngine` instances and manages the full highlighting pipeline internally: `compute()` runs tree-sitter synchronously (<2ms), queues a debounced LSP token request, and returns immediately. Merged tokens are delivered to the TUI via a `SyntaxFrontend` callback trait defined in Layer 1 and implemented in Layer 2 — `EditorState` never owns display tokens. The TUI remains a dumb renderer: it receives token updates through the callback and passes them to `editor_pane::render`.

---

## User Stories

### User Story 1
As a: developer opening a Go, TypeScript, Python, or Markdown file in ane

I want to: see syntax highlighting appear immediately when the file opens, before any language server has started

So I can: read and navigate code from frame one, without waiting for LSP initialization

### User Story 2
As a: developer working in a project with an active language server

I want to: see richer semantic highlighting (type-aware coloring of identifiers, parameters, type aliases) layer on top of the structural baseline once the LSP is ready

So I can: benefit from type-inference-aware colors without any configuration or manual reload

### User Story 3
As a: developer on a full-stack project with a Go backend and TypeScript frontend

I want to: see per-language LSP status indicators in the bottom-right corner of the TUI, each colored to reflect that server's state

So I can: know at a glance whether both servers are ready, which is still installing, and which has failed

---

## Implementation Details

### Architecture overview

The highlighting pipeline uses a callback-driven architecture where `SyntaxEngine` (Layer 1) owns the full token lifecycle and pushes results to the frontend:

```
Buffer change (Layer 2 detects)
        │
        ▼
SyntaxEngine::compute(path, content)                   ← Layer 1, returns immediately
  │
  ├─ [sync]  tree-sitter parse (cached by content hash)
  │          merge with cached LSP tokens
  │          → SyntaxFrontend::set_semantic_tokens()     (~2ms)
  │
  └─ [async] queue debounced LSP request → background worker:
             wait 300ms debounce window
             LspEngine::semantic_tokens()
             check content hash for staleness
             merge with cached tree-sitter tokens
             → SyntaxFrontend::set_semantic_tokens()     (300ms+ after last edit)
                    │
                    ▼
TuiSyntaxReceiver (Layer 2 impl of SyntaxFrontend trait)
stores tokens per-path in Arc<Mutex<HashMap>>
                    │
                    ▼
editor_pane::render(&tokens)                            ← Layer 2, no TS/LSP awareness
```

`app.rs` (Layer 2) calls `syntax_engine.compute()` on buffer change or file switch. It does not manage debounce timers, LSP token channels, or version counters — all of that is internal to `SyntaxEngine`. The editor pane reads tokens from `TuiSyntaxReceiver` and applies styling without knowing how the tokens were produced.

---

### 1. Language capability matrix — `src/data/lsp/types.rs` (Layer 0)

Add `Markdown` to the `Language` enum and a `capabilities()` method:

```rust
pub enum Language {
    Rust,
    Go,
    TypeScript,
    Python,
    Markdown,
}

pub struct LanguageCapabilities {
    pub has_tree_sitter: bool,
    pub has_lsp: bool,
}

impl Language {
    pub fn capabilities(self) -> LanguageCapabilities {
        match self {
            Language::Rust       => LanguageCapabilities { has_tree_sitter: true,  has_lsp: true  },
            Language::Go         => LanguageCapabilities { has_tree_sitter: true,  has_lsp: true  },
            Language::TypeScript => LanguageCapabilities { has_tree_sitter: true,  has_lsp: true  },
            Language::Python     => LanguageCapabilities { has_tree_sitter: true,  has_lsp: true  },
            Language::Markdown   => LanguageCapabilities { has_tree_sitter: true,  has_lsp: false },
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs"              => Some(Self::Rust),
            "go"              => Some(Self::Go),
            "ts" | "tsx"
            | "js" | "jsx"   => Some(Self::TypeScript),
            "py"              => Some(Self::Python),
            "md" | "markdown" => Some(Self::Markdown),
            _                 => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Rust       => "rust",
            Self::Go         => "go",
            Self::TypeScript => "typescript",
            Self::Python     => "python",
            Self::Markdown   => "markdown",
        }
    }
}
```

Add a helper that returns the correct LSP `languageId` for a specific file path — needed because vtsls distinguishes `.ts` (`"typescript"`) from `.tsx` (`"typescriptreact"`) and `.js`/`.jsx` similarly:

```rust
impl Language {
    pub fn language_id_for_path(path: &std::path::Path) -> Option<&'static str> {
        path.extension().and_then(|e| e.to_str()).and_then(|ext| match ext {
            "rs"   => Some("rust"),
            "go"   => Some("go"),
            "ts"   => Some("typescript"),
            "tsx"  => Some("typescriptreact"),
            "js"   => Some("javascript"),
            "jsx"  => Some("javascriptreact"),
            "py"   => Some("python"),
            _      => None,
        })
    }
}
```

The LSP engine (`do_notify_open`) uses this instead of `lang.name()` so vtsls receives the correct `languageId` per file.

Remove `semantic_tokens` from `LspSharedState` — token management is now internal to `SyntaxEngine`:

```rust
pub struct LspSharedState {
    pub status: ServerState,
    pub install_line: Option<InstallLine>,
    // semantic_tokens removed — owned by SyntaxEngine
}
```

`EditorState` gains no new token-related fields. Display tokens are stored in `TuiSyntaxReceiver` (Layer 2), not in `EditorState` (Layer 0).

---

### 2. New module: `src/commands/syntax_engine/` (Layer 1)

```
src/commands/syntax_engine/
    mod.rs          SyntaxFrontend trait, SyntaxEngine struct, background worker
    tree_sitter.rs  parse(Language, &str) -> Vec<SemanticToken>
    merge.rs        merge(ts: &[SemanticToken], lsp: &[SemanticToken]) -> Vec<SemanticToken>
```

**`mod.rs` — `SyntaxFrontend` trait**

Defined in Layer 1, implemented by Layer 2. This is the only channel through which computed tokens reach the frontend — dependency inversion ensures Layer 1 never imports from Layer 2:

```rust
use std::path::Path;
use crate::data::lsp::types::SemanticToken;

/// Callback trait for delivering computed syntax tokens to the frontend.
/// Layer 1 defines this; Layer 2 implements it.
pub trait SyntaxFrontend: Send + Sync {
    fn set_semantic_tokens(&self, path: &Path, tokens: Vec<SemanticToken>);
}
```

**`mod.rs` — `SyntaxEngine` struct**

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use crate::commands::lsp_engine::LspEngine;
use crate::data::lsp::types::{Language, SemanticToken};

struct LspRequest {
    path: PathBuf,
    content: String,
    content_hash: u64,
    ts_tokens: Vec<SemanticToken>,
}

pub struct SyntaxEngine {
    ts_cache: HashMap<PathBuf, (u64, Vec<SemanticToken>)>,
    lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
    content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    frontend: Arc<dyn SyntaxFrontend>,
    lsp_tx: mpsc::SyncSender<LspRequest>,
}
```

**`mod.rs` — `SyntaxEngine::new`**

The constructor takes an `Arc<Mutex<LspEngine>>` and an `Arc<dyn SyntaxFrontend>`, then spawns a background worker thread that handles debounced LSP requests:

```rust
impl SyntaxEngine {
    pub fn new(
        lsp_engine: Arc<Mutex<LspEngine>>,
        frontend: Arc<dyn SyntaxFrontend>,
    ) -> Self {
        let (lsp_tx, lsp_rx) = mpsc::sync_channel::<LspRequest>(1);
        let lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let w_frontend = Arc::clone(&frontend);
        let w_lsp_cache = Arc::clone(&lsp_cache);
        let w_hashes = Arc::clone(&content_hashes);

        std::thread::spawn(move || {
            Self::lsp_worker(lsp_engine, lsp_rx, w_frontend, w_lsp_cache, w_hashes);
        });

        Self {
            ts_cache: HashMap::new(),
            lsp_cache,
            content_hashes,
            frontend,
            lsp_tx,
        }
    }
}
```

**`mod.rs` — `SyntaxEngine::compute`**

Called by the TUI when the buffer changes or when switching files. Returns immediately. Tree-sitter runs synchronously and delivers tokens via `set_semantic_tokens` within the call. A debounced LSP request is queued for the background worker — when it completes, the worker calls `set_semantic_tokens` again with merged tokens:

```rust
impl SyntaxEngine {
    /// Returns immediately. Runs tree-sitter synchronously (<2ms), then
    /// queues a debounced LSP token request on the background worker.
    /// Calls frontend.set_semantic_tokens() synchronously with tree-sitter
    /// tokens (merged with any cached LSP tokens), and again asynchronously
    /// when the LSP responds.
    pub fn compute(&mut self, path: &Path, content: &str) {
        let lang = match Language::from_path(path) {
            Some(l) => l,
            None => return,
        };
        let caps = lang.capabilities();
        let content_hash = hash(content);

        // Phase 1: tree-sitter (synchronous, cached by content hash)
        let ts_tokens = if caps.has_tree_sitter {
            if self.ts_cache.get(path).map(|(h, _)| *h) != Some(content_hash) {
                let tokens = tree_sitter::parse(lang, content);
                self.ts_cache.insert(path.to_path_buf(), (content_hash, tokens));
            }
            self.ts_cache.get(path).unwrap().1.clone()
        } else {
            vec![]
        };

        // Merge with any previously cached LSP tokens for this path
        let cached_lsp = self.lsp_cache.lock().unwrap()
            .get(path).cloned().unwrap_or_default();
        let merged = if caps.has_lsp && !cached_lsp.is_empty() {
            merge::merge(&ts_tokens, &cached_lsp)
        } else {
            ts_tokens.clone()
        };

        // Deliver best-effort tokens to frontend immediately
        self.frontend.set_semantic_tokens(path, merged);

        // Update content hash for staleness detection by the worker
        self.content_hashes.lock().unwrap()
            .insert(path.to_path_buf(), content_hash);

        // Phase 2: queue debounced LSP request (try_send drops if full — latest-wins)
        if caps.has_lsp {
            let _ = self.lsp_tx.try_send(LspRequest {
                path: path.to_path_buf(),
                content: content.to_string(),
                content_hash,
                ts_tokens,
            });
        }
    }
}
```

The content hash is a fast non-cryptographic hash (e.g. FNV or `std::collections::hash_map::DefaultHasher`) over the content bytes. It serves two purposes: (1) avoid re-running tree-sitter when content hasn't changed, and (2) detect staleness — if the background worker finishes an LSP request but the content hash no longer matches, the result is discarded.

**`mod.rs` — background worker**

The worker thread implements debouncing: when it receives a request, it waits 300ms for newer requests, always keeping the latest. After the debounce window closes, it calls `LspEngine::semantic_tokens()`, checks for staleness, caches the result, merges with tree-sitter tokens, and delivers via `set_semantic_tokens`:

```rust
impl SyntaxEngine {
    fn lsp_worker(
        engine: Arc<Mutex<LspEngine>>,
        rx: mpsc::Receiver<LspRequest>,
        frontend: Arc<dyn SyntaxFrontend>,
        lsp_cache: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
        content_hashes: Arc<Mutex<HashMap<PathBuf, u64>>>,
    ) {
        let debounce = Duration::from_millis(300);

        while let Ok(mut req) = rx.recv() {
            // Debounce: drain newer requests for 300ms, keeping the latest
            loop {
                match rx.recv_timeout(debounce) {
                    Ok(newer) => req = newer,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }

            // Fetch LSP semantic tokens
            let lsp_tokens = engine.lock().unwrap()
                .semantic_tokens(&req.path, &req.content)
                .unwrap_or_default();

            // Staleness check: discard if content changed since request was queued
            let current = content_hashes.lock().unwrap()
                .get(&req.path).copied();
            if current != Some(req.content_hash) {
                continue;
            }

            // Cache LSP tokens for use by future compute() calls
            lsp_cache.lock().unwrap()
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
```

**`merge.rs` — merge algorithm**

LSP tokens take priority: for any byte range where an LSP token exists, the tree-sitter token covering the same range is discarded.

```rust
/// lsp_tokens win over ts_tokens on any overlapping character range.
/// Both inputs must be sorted by (line, start_col).
pub fn merge(ts: &[SemanticToken], lsp: &[SemanticToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(ts.len() + lsp.len());

    for ts_tok in ts {
        let overlaps_lsp = lsp.iter().any(|l| {
            l.line == ts_tok.line
                && l.start_col < ts_tok.start_col + ts_tok.length
                && l.start_col + l.length > ts_tok.start_col
        });
        if !overlaps_lsp {
            result.push(ts_tok.clone());
        }
    }

    result.extend_from_slice(lsp);
    result.sort_by_key(|t| (t.line, t.start_col));
    result
}
```

**`tree_sitter.rs` — per-language parsing**

```rust
pub fn parse(lang: Language, content: &str) -> Vec<SemanticToken> {
    match lang {
        Language::Rust       => parse_with(tree_sitter_rust::language(),       content, rust_node_type),
        Language::Go         => parse_with(tree_sitter_go::language(),         content, go_node_type),
        Language::TypeScript => parse_with(tree_sitter_typescript::language_typescript(), content, ts_node_type),
        Language::Python     => parse_with(tree_sitter_python::language(),     content, python_node_type),
        Language::Markdown   => parse_with(tree_sitter_md::language(),         content, md_node_type),
    }
}
```

`parse_with` creates a `tree_sitter::Parser`, parses `content`, then walks the resulting syntax tree with a depth-first cursor, calling the language-specific `*_node_type` function to map each node's type string to a semantic token type string. Nodes that map to `None` are skipped. Multi-line nodes emit one `SemanticToken` per covered line. Note: verify the exact crate name for the markdown grammar at implementation time — the crate has been published as both `tree-sitter-md` and `tree-sitter-markdown` depending on the source.

**Node type → token type mappings**

Common across languages (LSP standard token types, reused so `token_type_color` in `editor_pane` needs no changes for existing languages):

| Node type (examples)          | token_type string |
|-------------------------------|-------------------|
| `fn`, `let`, `const`, `func`  | `"keyword"`       |
| `fn`-name, function call ident | `"function"`     |
| `struct`, `class`, `type`     | `"type"`          |
| `string_literal`, `raw_string`| `"string"`        |
| `integer_literal`, `float`    | `"number"`        |
| `line_comment`, `block_comment`| `"comment"`      |

Markdown-specific (new token types added to `token_type_color`):

| Node type                          | token_type string  | Rendered style              |
|------------------------------------|--------------------|-----------------------------|
| `atx_heading`, `setext_heading`    | `"heading"`        | Bold + Yellow               |
| `strong_emphasis`                  | `"strong"`         | Bold + White                |
| `emphasis`                         | `"emphasis"`       | Italic + White (if terminal supports) |
| `code_span`, `fenced_code_block`,`indented_code_block` | `"code"` | Green        |
| `link`, `image`                    | `"link"`           | Cyan + Underline            |
| `block_quote`                      | `"quote"`          | DarkGray                    |
| `list_marker_dot`, `list_marker_minus`, `list_marker_star` | `"list_marker"` | Yellow |
| `thematic_break`                   | `"punctuation"`    | DarkGray                    |

**`token_type_color` → `token_style` refactor in `editor_pane.rs`**

The function currently returns `Color`. Change it to return `Style` so markdown bold/italic modifiers can be expressed. Update `styled_line_with_tokens` to apply the full style rather than just the foreground color:

```rust
// Before:
let color = token_type_color(&token.token_type);
spans.push(Span::styled(text, Style::default().fg(color)));

// After:
let style = token_style(&token.token_type);
spans.push(Span::styled(text, style));
```

All existing token types return `Style::default().fg(their_color)` — no behavioral change. New markdown types add modifiers on top.

---

### 3. LSP server registry — `src/data/lsp/registry.rs` (Layer 0)

Add server entries for the three new LSP languages. The registry skips languages where `capabilities().has_lsp == false`, so `Markdown` never gets a server entry.

```rust
static GOPLS: LspServerInfo = LspServerInfo {
    language: Language::Go,
    server_name: "gopls",
    binary_name: "gopls",
    install_command: "go install golang.org/x/tools/gopls@latest",
    check_command: "gopls version",
    default_args: &[],
    init_options_json: "",
};

static VTSLS: LspServerInfo = LspServerInfo {
    language: Language::TypeScript,
    server_name: "vtsls",
    binary_name: "vtsls",
    install_command: "npm install -g @vtsls/language-server",
    check_command: "vtsls --version",
    default_args: &["--stdio"],
    init_options_json: "",
};

static BASEDPYRIGHT: LspServerInfo = LspServerInfo {
    language: Language::Python,
    server_name: "basedpyright-langserver",
    binary_name: "basedpyright-langserver",
    install_command: "pip install basedpyright",
    check_command: "basedpyright-langserver --version",
    default_args: &["--stdio"],
    init_options_json: "",
};

static SERVERS: &[&LspServerInfo] = &[&RUST_ANALYZER, &GOPLS, &VTSLS, &BASEDPYRIGHT];
```

**Multi-language directory detection**: replace `detect_language_from_dir` (returns `Option<Language>`) with `detect_languages_from_dir` (returns `Vec<Language>`) that scans for all manifest types in a single upward walk:

```rust
pub fn detect_languages_from_dir(path: &Path) -> Vec<Language> {
    const MANIFESTS: &[(&str, Language)] = &[
        ("Cargo.toml",       Language::Rust),
        ("go.mod",           Language::Go),
        ("go.work",          Language::Go),
        ("package.json",     Language::TypeScript),
        ("tsconfig.json",    Language::TypeScript),
        ("pyproject.toml",   Language::Python),
        ("pyrightconfig.json", Language::Python),
        ("setup.py",         Language::Python),
    ];
    let mut found = Vec::new();
    let mut cur = Some(path);
    while let Some(p) = cur {
        for (file, lang) in MANIFESTS {
            if p.join(file).exists() && !found.contains(lang) {
                found.push(*lang);
            }
        }
        cur = p.parent();
    }
    found
}
```

The LSP engine's `start_for_context` filters detected languages by `capabilities().has_lsp` before looking up server entries, so Markdown is silently skipped.

**Per-language workspace root resolution**: extend `workspace_root_for_dir` to accept a `Language` and resolve the correct anchor file. Rust uses topmost `Cargo.toml` (workspace root over member crate). Go prefers topmost `go.work` then topmost `go.mod`. TypeScript uses the _nearest_ (innermost) `tsconfig.json` to match how tsc resolves project boundaries. Python uses the nearest `pyrightconfig.json`, then `pyproject.toml`.

**Fix `languageId` in `do_notify_open`** (`src/commands/lsp_engine/engine.rs`): use `Language::language_id_for_path(file_path).unwrap_or(lang.name())` instead of `lang.name()` so vtsls receives `"typescriptreact"` for `.tsx` files.

---

### 4. Serialize concurrent installs — `src/commands/lsp_engine/engine.rs` (Layer 1)

Add a shared install mutex to `LspEngine` and pass it through `StartupContext`. Each startup thread acquires the lock only during the `installer::install()` call — not during server process startup — so multiple servers can run concurrently once installed, while installs themselves are strictly sequential:

```rust
pub struct LspEngine {
    // ...existing fields...
    install_lock: Arc<Mutex<()>>,
}

// In startup_thread, around the install call:
if !installed && ctx.auto_install {
    let _guard = ctx.install_lock.lock().unwrap();
    // Re-check inside the lock: a prior install may have satisfied the dep.
    if !installer::is_installed(server_info) {
        try_transition(&state, lang, ServerState::Installing, &event_tx);
        if let Err(e) = installer::install(server_info, ctx.install_progress.as_ref()) {
            try_transition(&state, lang, ServerState::Failed, &event_tx);
            return;
        }
    }
    // _guard drops here, before spawning the server process.
    try_transition(&state, lang, ServerState::Available, &event_tx);
}
```

---

### 5. TUI wiring — `src/frontend/tui/` (Layer 2)

**`TuiSyntaxReceiver` — implements `SyntaxFrontend`**

The TUI's implementation of the Layer 1 trait stores tokens per-path behind an `Arc<Mutex<>>` so the background worker thread and the render loop can both access it safely:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::commands::syntax_engine::SyntaxFrontend;
use crate::data::lsp::types::SemanticToken;

pub struct TuiSyntaxReceiver {
    tokens: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
}

impl TuiSyntaxReceiver {
    pub fn new() -> Self {
        Self { tokens: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn tokens_for(&self, path: &Path) -> Vec<SemanticToken> {
        self.tokens.lock().unwrap()
            .get(path).cloned().unwrap_or_default()
    }
}

impl SyntaxFrontend for TuiSyntaxReceiver {
    fn set_semantic_tokens(&self, path: &Path, tokens: Vec<SemanticToken>) {
        self.tokens.lock().unwrap().insert(path.to_path_buf(), tokens);
    }
}
```

**`app.rs` — initialization**

Replace the old `token_request_task` and debounce logic with `SyntaxEngine` + `TuiSyntaxReceiver` wiring:

```rust
// Create the syntax receiver (Layer 2's impl of SyntaxFrontend)
let syntax_receiver = Arc::new(TuiSyntaxReceiver::new());

// Create SyntaxEngine (Layer 1) — it owns the LspEngine reference
// and spawns its own background worker for debounced LSP requests
let mut syntax_engine = SyntaxEngine::new(
    Arc::clone(&lsp_engine),
    Arc::clone(&syntax_receiver) as Arc<dyn SyntaxFrontend>,
);

// Initial compute for the opened file
if let Some(buf) = state.current_buffer() {
    syntax_engine.compute(&buf.path, &buf.content());
}
```

**`app.rs` — event loop**

The event loop is simplified: no debounce timers, no `token_tx`/`token_rx` channels, no `last_seen_lsp_version`. Just call `compute()` when the buffer changes or when switching files:

```rust
// Inside event_loop, after handling key events:

if buffer_modified {
    if let Some(buf) = state.current_buffer() {
        syntax_engine.compute(&buf.path, &buf.content());
    }
}

// Also trigger compute on file switch (e.g. Ctrl-T tree navigation):
if file_switched {
    if let Some(buf) = state.current_buffer() {
        syntax_engine.compute(&buf.path, &buf.content());
    }
}
```

`compute()` handles everything internally: tree-sitter runs immediately and delivers tokens via the `set_semantic_tokens` callback within the call (~2ms). The debounced LSP request is queued to the background worker, which will call `set_semantic_tokens` again with merged tokens after the 300ms debounce window and LSP response.

**`app.rs` — render**

Read tokens from `TuiSyntaxReceiver` during the draw call:

```rust
// In the draw closure:
let tokens = if let Some(buf) = state.current_buffer() {
    syntax_receiver.tokens_for(&buf.path)
} else {
    vec![]
};
editor_pane::render(frame, area, state, &tokens);
```

**Removed from `app.rs`**

The following are no longer needed and should be deleted:

- `token_request_task` function — replaced by `SyntaxEngine`'s internal background worker
- `token_tx` / `token_rx` channel — debounce channel is internal to `SyntaxEngine`
- `last_edit: Option<Instant>` debounce timer — debouncing is internal to `SyntaxEngine`
- `last_seen_lsp_version` tracking — callback replaces polling
- Any code that reads `lsp_state.semantic_tokens` — field no longer exists

**LSP server readiness**

When an LSP server transitions to `Running` after startup, `app.rs` should trigger a `compute()` call so that the first LSP token request is sent:

```rust
// When status polling detects a server became Running:
if lsp_became_ready {
    if let Some(buf) = state.current_buffer() {
        syntax_engine.compute(&buf.path, &buf.content());
    }
}
```

This ensures LSP tokens are fetched as soon as the server is ready, even if the user hasn't edited since opening the file. The `compute()` call is cheap when content hasn't changed (tree-sitter cache hit), and the LSP request flows through the normal debounced path.

---

### 6. `editor_pane::render` cleanup — `src/frontend/tui/editor_pane.rs` (Layer 2)

Remove `lsp_status: ServerState` from the signature — it was only used to gate highlighting. Replace with data-driven check:

```rust
// Before:
pub fn render(frame, area, state, lsp_status: ServerState, semantic_tokens: &[SemanticToken])
let use_highlighting = lsp_status == ServerState::Running;

// After:
pub fn render(frame, area, state, tokens: &[SemanticToken])
let use_highlighting = !tokens.is_empty();
```

The call site in `app.rs` passes `&tokens` read from `TuiSyntaxReceiver`. No other changes to the render logic.

---

### 7. Multi-language status bar — `src/frontend/tui/status_bar.rs` (Layer 2)

Change `render` to accept `&[(Language, ServerState)]` from `engine.status_summary()` instead of a single `ServerState`. Render one compact colored indicator per detected language on the right side:

- `●` Green = `Running`
- `◌` Yellow = `Installing | Starting | Available`
- `✖` Red = `Failed | Missing`
- (omitted) = `Undetected | Stopped`

Example right side for a Go+TypeScript project: ` go:● ts:◌ `

Languages with `capabilities().has_lsp == false` (Markdown) never appear in the LSP status — they have no server to report on.

---

### 8. New Cargo.toml dependencies

```toml
[dependencies]
tree-sitter = "0.22"
tree-sitter-rust = "0.21"
tree-sitter-go = "0.21"
tree-sitter-typescript = "0.21"
tree-sitter-python = "0.21"
# Verify the exact crate name for markdown at implementation time;
# published as either tree-sitter-md or tree-sitter-markdown.
tree-sitter-md = "0.3"
```

All grammar crates compile the C parser via a `build.rs` build script. They link into the binary — check release binary size delta before merging, since LTO + strip is already configured.

---

## Edge Case Considerations

- **Language with no tree-sitter and no LSP**: `SyntaxEngine::compute` calls `set_semantic_tokens` with `Vec::new()`. `use_highlighting` is false. The file renders in plain gray — same as today's behavior for unknown files.

- **Language with LSP but no tree-sitter** (future case): `compute` skips the TS parse, uses LSP tokens directly with no merge step. The `has_tree_sitter: false` branch falls through cleanly.

- **Language with tree-sitter but no LSP** (Markdown): `compute` delivers pure tree-sitter tokens via `set_semantic_tokens`. `has_lsp: false` means no LSP request is queued regardless of server state.

- **LSP tokens arrive before tree-sitter cache is warm** (unlikely but possible): `compute` is called with no cached TS tokens. The tree-sitter parse runs synchronously on the first call, which is fast enough (< 5ms for any file a human edits). The cache is populated immediately and tokens are delivered via callback before `compute()` returns.

- **Content changes while LSP request is in flight**: The background worker checks the content hash against `content_hashes` after the LSP response arrives. If the hash no longer matches (content changed), the stale result is discarded. The next `compute()` call will have already delivered fresh tree-sitter tokens and queued a new LSP request.

- **Rapid typing (debounce behavior)**: Each keystroke calls `compute()`, which delivers tree-sitter tokens immediately (~2ms per call). LSP requests are debounced: the background worker waits 300ms after the last received request before processing. The bounded channel (`sync_channel(1)`) with `try_send` provides latest-wins backpressure — if the worker is busy with an LSP call, only the most recent request survives.

- **`languageId` mismatch for `.tsx`**: vtsls silently fails to type-check React files if sent `"typescript"` instead of `"typescriptreact"`. The `language_id_for_path` fix in `do_notify_open` is required for correct vtsls behavior.

- **Go toolchain absent**: `go install gopls` fails cleanly if `go` is not on PATH. The startup thread transitions to `Failed`. The status bar shows `go:✖`. ane does not manage Go toolchain installation.

- **npm absent for vtsls**: same pattern. Consider surfacing a hint in the `LspEvent::Error` message: "vtsls install requires npm — install Node.js first."

- **pip/Python absent for basedpyright**: same. The `Failed` state surfaces this via `LspEvent::Error`.

- **TypeScript project without `tsconfig.json`**: vtsls creates an implicit project. No special handling needed.

- **Go workspace with `go.work` and multiple modules**: the workspace root resolver must prefer `go.work` over `go.mod` — gopls uses the `go.work` directory as `rootUri` to index the full workspace rather than a single module.

- **Python virtualenv**: basedpyright auto-detects via `pyrightconfig.json` or `pyproject.toml`. No venv management in ane.

- **Multi-line markdown tree-sitter tokens** (fenced code blocks, block quotes): tree-sitter nodes spanning multiple lines must be emitted as one `SemanticToken` per line in `parse_with`, since `styled_line_with_tokens` filters by `t.line == line_num`. The walker splits multi-line nodes on `\n` boundaries.

- **Install lock starvation with three missing servers**: acceptable — sequential install is the explicit requirement. Status bar shows all three as `◌` (yellow); install output streams through the center message area one server at a time.

- **Content hash collision**: statistically negligible for a 64-bit hash over file content. If it occurs, stale tree-sitter tokens are shown for one render cycle until the next edit clears them.

- **Status bar width**: five languages at ~6 chars each = ~30 chars. Omit `Undetected`/`Stopped` entries to keep it short. Truncate from the right if the terminal is very narrow.

- **File switch token latency**: when switching to a previously opened file, `compute()` delivers cached tree-sitter tokens immediately (cache hit). If cached LSP tokens exist for that file, they're merged in the same synchronous call. No flicker.

- **Background worker thread lifetime**: the worker thread runs until the `SyncSender` is dropped (when `SyntaxEngine` is dropped). No explicit shutdown signal is needed.

---

## Test Considerations

- **Unit: `Language::capabilities()`** — assert each variant returns the expected `has_tree_sitter`/`has_lsp` booleans. Assert `Markdown` has `has_lsp: false`.

- **Unit: `Language::from_extension`** — `.md` and `.markdown` → `Markdown`; `.tsx` → `TypeScript`; `.py` → `Python`; `.go` → `Go`. Existing `.rs` → `Rust` still passes.

- **Unit: `Language::language_id_for_path`** — `.tsx` → `"typescriptreact"`, `.jsx` → `"javascriptreact"`, `.ts` → `"typescript"`, `.js` → `"javascript"`.

- **Unit: `merge::merge` LSP wins on overlap** — ts token at (line 3, col 5, len 4); lsp token at (line 3, col 4, len 6); result contains only the lsp token for that range.

- **Unit: `merge::merge` non-overlapping tokens preserved** — ts token at col 0, lsp token at col 10; both appear in result, sorted by start_col.

- **Unit: `merge::merge` lsp-only** — empty ts slice + lsp tokens → returns lsp tokens unchanged.

- **Unit: `merge::merge` ts-only** — ts tokens + empty lsp slice → returns ts tokens unchanged.

- **Unit: `SyntaxEngine::compute` with `has_lsp: false` language** — compute on a `.md` file; assert `set_semantic_tokens` is called with pure tree-sitter tokens and no LSP request is queued.

- **Unit: `SyntaxEngine::compute` tree-sitter cache hit** — call `compute` twice with same content; assert tree-sitter is only invoked once (verify via counter or by hashing parse calls in a test double).

- **Unit: `SyntaxEngine::compute` cache miss on content change** — call `compute` twice with different content; assert new ts tokens in second `set_semantic_tokens` call.

- **Unit: `SyntaxEngine::compute` returns immediately** — call `compute` and verify it completes in < 10ms (no blocking on LSP). The `set_semantic_tokens` callback fires synchronously within `compute()` for the tree-sitter path.

- **Unit: debounce coalesces rapid calls** — call `compute` 10 times in quick succession; assert the background worker only issues one LSP request (after the 300ms debounce window closes).

- **Unit: staleness check discards outdated LSP tokens** — call `compute` with content A, then immediately call `compute` with content B. When the LSP response for content A arrives, it should be discarded (hash mismatch). Only content B's LSP response should be delivered.

- **Unit: `SyntaxFrontend::set_semantic_tokens` delivers to `TuiSyntaxReceiver`** — create a `TuiSyntaxReceiver`, call `set_semantic_tokens`, assert `tokens_for` returns the tokens.

- **Unit: `TuiSyntaxReceiver` per-path isolation** — set tokens for path A and path B; assert `tokens_for(A)` and `tokens_for(B)` return their respective tokens.

- **Unit: `detect_languages_from_dir` multi-language** — temp dir with `Cargo.toml` + `go.mod`; assert both `Rust` and `Go` in result.

- **Unit: `detect_languages_from_dir` Markdown not in dir detection** — `.md` files are detected by extension only, not by manifest. Assert `detect_languages_from_dir` on a dir with only `.md` files returns `[]`.

- **Unit: install lock serializes installations** — spawn two threads both calling through the install path with a slow mock install command; assert their install intervals do not overlap.

- **Unit: `token_style` for markdown types** — `"heading"` → style with `Modifier::BOLD`; `"emphasis"` → style with `Modifier::ITALIC`; `"code"` → `Color::Green`.

- **Unit: `do_notify_open` sends correct `languageId` for `.tsx`** — use mock LSP infrastructure to capture `didOpen` params; assert `languageId == "typescriptreact"`.

- **Integration: LSP server readiness triggers token fetch** — start with LSP not ready, call `compute()` (delivers tree-sitter only). Simulate server becoming `Running`, trigger `compute()` again. Assert that `set_semantic_tokens` is eventually called with merged tokens.

- **Manual checklist**:
  - Open a `.md` file — headings are bold yellow, code spans green, links cyan, immediately on open (no LSP wait)
  - Open a `.rs` file — basic keyword/type/string colors appear immediately (tree-sitter); after rust-analyzer starts, semantic colors layer on top (e.g. type aliases take a different color than plain struct names)
  - Open a monorepo with Go + TypeScript dirs — `go:●` and `ts:●` appear in status bar
  - With all three LSP servers uninstalled, open a tri-language project — install output streams for one server at a time; second server install begins only after first completes
  - Open a `.tsx` file and verify completions work (confirms `languageId: typescriptreact` is sent)
  - Open a file with an unknown extension — no highlighting, plain gray, no crash
  - Type rapidly in a `.rs` file — tree-sitter colors update on every keystroke; LSP colors update ~300ms after typing stops

---

## Codebase Integration

- **Layer 0** (`src/data/lsp/types.rs`, `src/data/state.rs`, `src/data/lsp/registry.rs`):
  - Add `Go`, `TypeScript`, `Python`, `Markdown` to `Language`. Add `LanguageCapabilities` and `capabilities()`. Add `language_id_for_path()`.
  - Remove `semantic_tokens` from `LspSharedState` — token lifecycle is managed by `SyntaxEngine` (Layer 1). Keep `status` and `install_line`.
  - `EditorState` gains no token-related fields. Display tokens are stored in `TuiSyntaxReceiver` (Layer 2).
  - Replace `detect_language_from_dir` with `detect_languages_from_dir`. Add per-language `workspace_root_for_language`. Add `GOPLS`, `VTSLS`, `BASEDPYRIGHT` to `SERVERS`.
  - Existing `detect_language_from_dir` callers in tests: update to use the new function or add a compatibility shim returning `found.into_iter().next()`.

- **Layer 1** (`src/commands/syntax_engine/` — new module):
  - `SyntaxFrontend` trait: defines `set_semantic_tokens(path, tokens)`. Layer 2 implements this.
  - `SyntaxEngine`: constructed with `Arc<Mutex<LspEngine>>` + `Arc<dyn SyntaxFrontend>`. Owns tree-sitter cache, LSP token cache, content hash map, and a background worker thread. `compute()` runs tree-sitter synchronously, delivers tokens via callback, and queues debounced LSP requests to the worker.
  - Background worker: receives debounced LSP requests, calls `LspEngine::semantic_tokens()`, checks staleness via content hash, caches LSP tokens, merges with tree-sitter tokens, delivers via `set_semantic_tokens` callback.
  - `tree_sitter::parse`, `merge::merge` — pure functions, no state.
  - `src/commands/lsp_engine/engine.rs`: add `install_lock`, fix `do_notify_open` `languageId`, filter `start_for_context` by `capabilities().has_lsp`.
  - `src/commands/mod.rs`: expose `syntax_engine` module.

- **Layer 2** (`src/frontend/tui/`):
  - `TuiSyntaxReceiver`: implements `SyntaxFrontend`. Stores `Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>`. Provides `tokens_for(path)` for the render loop.
  - `app.rs`: creates `TuiSyntaxReceiver` and `SyntaxEngine`, wiring them together. Calls `syntax_engine.compute()` on buffer change, file switch, and LSP server readiness. Reads tokens from `syntax_receiver.tokens_for()` during render. Removes `token_request_task`, debounce timer, version counter, and all direct `lsp_state.semantic_tokens` access.
  - `editor_pane.rs`: remove `lsp_status` param, gate highlighting on `!tokens.is_empty()`, change `token_type_color` → `token_style` returning `Style`.
  - `status_bar.rs`: accept `&[(Language, ServerState)]`, render per-language indicators.
  - No tree-sitter imports anywhere in `src/frontend/`.

- **`Cargo.toml`**: add `tree-sitter` + five grammar crates. Verify markdown crate name before adding.
