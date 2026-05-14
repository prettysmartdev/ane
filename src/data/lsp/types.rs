use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Undetected,
    Missing,
    Available,
    Installing,
    Starting,
    Running,
    Stopped,
    Failed,
}

impl ServerState {
    pub fn display(&self) -> &'static str {
        match self {
            Self::Undetected => "LSP: checking...",
            Self::Missing => "LSP: not installed",
            Self::Available => "LSP: available",
            Self::Installing => "LSP: installing...",
            Self::Starting => "LSP: starting...",
            Self::Running => "LSP: ready",
            Self::Stopped => "LSP: stopped",
            Self::Failed => "LSP: failed",
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn is_pending(&self) -> bool {
        matches!(
            self,
            Self::Undetected | Self::Installing | Self::Starting | Self::Available
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Running | Self::Failed | Self::Stopped)
    }

    /// Returns true if a transition from `self` to `new` is permitted by the
    /// engine's state machine. Any unlisted pair is invalid.
    pub fn can_transition_to(self, new: Self) -> bool {
        use ServerState::*;
        if self == new {
            return false;
        }
        matches!(
            (self, new),
            (
                Undetected,
                Missing | Available | Installing | Failed | Stopped
            ) | (Missing, Installing | Available | Failed | Stopped)
                | (Installing, Available | Failed | Stopped)
                | (Available, Starting | Stopped | Failed)
                | (Starting, Running | Failed | Stopped)
                | (Running, Stopped | Failed)
                | (Failed, Available | Installing | Starting | Stopped)
                | (Stopped, Available | Starting)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            Language::Rust => LanguageCapabilities {
                has_tree_sitter: true,
                has_lsp: true,
            },
            Language::Go => LanguageCapabilities {
                has_tree_sitter: true,
                has_lsp: true,
            },
            Language::TypeScript => LanguageCapabilities {
                has_tree_sitter: true,
                has_lsp: true,
            },
            Language::Python => LanguageCapabilities {
                has_tree_sitter: true,
                has_lsp: true,
            },
            Language::Markdown => LanguageCapabilities {
                has_tree_sitter: true,
                has_lsp: false,
            },
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "ts" | "tsx" | "js" | "jsx" => Some(Self::TypeScript),
            "py" => Some(Self::Python),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    pub fn language_id_for_path(path: &std::path::Path) -> Option<&'static str> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| match ext {
                "rs" => Some("rust"),
                "go" => Some("go"),
                "ts" => Some("typescript"),
                "tsx" => Some("typescriptreact"),
                "js" => Some("javascript"),
                "jsx" => Some("javascriptreact"),
                "py" => Some("python"),
                _ => None,
            })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Markdown => "markdown",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Go => "go",
            Self::TypeScript => "ts",
            Self::Python => "py",
            Self::Markdown => "md",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: SymbolRange,
    pub selection_range: Option<SymbolRange>,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Struct,
    Enum,
    Impl,
    Const,
    Field,
    Method,
    Module,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HoverInfo {
    pub contents: String,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub file_path: PathBuf,
    pub range: SymbolRange,
}

#[derive(Debug, Clone)]
pub enum LspEvent {
    StateChanged {
        language: Language,
        old: ServerState,
        new: ServerState,
    },
    DiagnosticsReceived {
        file_path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
    ServerMessage {
        language: Language,
        message: String,
    },
    Error {
        language: Language,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub range: SymbolRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone)]
pub struct SemanticToken {
    pub line: usize,
    pub start_col: usize,
    pub length: usize,
    pub token_type: String,
}

#[derive(Debug, Clone)]
pub struct SelectionRange {
    pub range: SymbolRange,
    pub parent: Option<Box<SelectionRange>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallLine {
    Stdout(String),
    Stderr(String),
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct LspSharedState {
    pub status: HashMap<Language, ServerState>,
    pub install_line: Option<InstallLine>,
}

pub struct LspServerInfo {
    pub language: Language,
    pub server_name: &'static str,
    pub binary_name: &'static str,
    pub install_command: &'static str,
    pub check_command: &'static str,
    pub default_args: &'static [&'static str],
    pub init_options_json: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_capabilities_all_variants() {
        let rust = Language::Rust.capabilities();
        assert!(rust.has_tree_sitter);
        assert!(rust.has_lsp);

        let go = Language::Go.capabilities();
        assert!(go.has_tree_sitter);
        assert!(go.has_lsp);

        let ts = Language::TypeScript.capabilities();
        assert!(ts.has_tree_sitter);
        assert!(ts.has_lsp);

        let py = Language::Python.capabilities();
        assert!(py.has_tree_sitter);
        assert!(py.has_lsp);

        let md = Language::Markdown.capabilities();
        assert!(md.has_tree_sitter);
        assert!(!md.has_lsp, "Markdown has no LSP server");
    }

    #[test]
    fn language_from_extension_work_item_cases() {
        assert_eq!(Language::from_extension("md"), Some(Language::Markdown));
        assert_eq!(
            Language::from_extension("markdown"),
            Some(Language::Markdown)
        );
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
    }

    #[test]
    fn language_id_for_path_all_cases() {
        use std::path::Path;
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.tsx")),
            Some("typescriptreact")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.jsx")),
            Some("javascriptreact")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.ts")),
            Some("typescript")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.js")),
            Some("javascript")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.rs")),
            Some("rust")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.go")),
            Some("go")
        );
        assert_eq!(
            Language::language_id_for_path(Path::new("foo.py")),
            Some("python")
        );
        // Markdown has no LSP languageId
        assert_eq!(Language::language_id_for_path(Path::new("foo.md")), None);
    }

    #[test]
    fn language_detection() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("js"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("jsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("md"), Some(Language::Markdown));
        assert_eq!(
            Language::from_extension("markdown"),
            Some(Language::Markdown)
        );
        assert_eq!(Language::from_extension(""), None);
        assert_eq!(Language::from_extension("txt"), None);
    }

    #[test]
    fn language_name() {
        assert_eq!(Language::Rust.name(), "rust");
        assert_eq!(Language::Go.name(), "go");
        assert_eq!(Language::TypeScript.name(), "typescript");
        assert_eq!(Language::Python.name(), "python");
        assert_eq!(Language::Markdown.name(), "markdown");
    }

    #[test]
    fn is_available_only_for_running() {
        assert!(ServerState::Running.is_available());
        assert!(!ServerState::Undetected.is_available());
        assert!(!ServerState::Missing.is_available());
        assert!(!ServerState::Available.is_available());
        assert!(!ServerState::Installing.is_available());
        assert!(!ServerState::Starting.is_available());
        assert!(!ServerState::Stopped.is_available());
        assert!(!ServerState::Failed.is_available());
    }

    #[test]
    fn is_pending_covers_transient_states() {
        assert!(ServerState::Undetected.is_pending());
        assert!(ServerState::Installing.is_pending());
        assert!(ServerState::Starting.is_pending());
        assert!(ServerState::Available.is_pending());
        assert!(!ServerState::Running.is_pending());
        assert!(!ServerState::Missing.is_pending());
        assert!(!ServerState::Stopped.is_pending());
        assert!(!ServerState::Failed.is_pending());
    }

    #[test]
    fn is_terminal_covers_final_states() {
        assert!(ServerState::Running.is_terminal());
        assert!(ServerState::Failed.is_terminal());
        assert!(ServerState::Stopped.is_terminal());
        assert!(!ServerState::Undetected.is_terminal());
        assert!(!ServerState::Missing.is_terminal());
        assert!(!ServerState::Available.is_terminal());
        assert!(!ServerState::Installing.is_terminal());
        assert!(!ServerState::Starting.is_terminal());
    }

    #[test]
    fn terminal_states_are_not_pending() {
        for state in [
            ServerState::Running,
            ServerState::Failed,
            ServerState::Stopped,
        ] {
            assert!(!state.is_pending(), "{:?} should not be pending", state);
        }
    }

    #[test]
    fn pending_states_are_not_terminal() {
        for state in [
            ServerState::Undetected,
            ServerState::Installing,
            ServerState::Starting,
            ServerState::Available,
        ] {
            assert!(!state.is_terminal(), "{:?} should not be terminal", state);
        }
    }

    #[test]
    fn all_states_have_display() {
        assert_eq!(ServerState::Undetected.display(), "LSP: checking...");
        assert_eq!(ServerState::Missing.display(), "LSP: not installed");
        assert_eq!(ServerState::Available.display(), "LSP: available");
        assert_eq!(ServerState::Installing.display(), "LSP: installing...");
        assert_eq!(ServerState::Starting.display(), "LSP: starting...");
        assert_eq!(ServerState::Running.display(), "LSP: ready");
        assert_eq!(ServerState::Stopped.display(), "LSP: stopped");
        assert_eq!(ServerState::Failed.display(), "LSP: failed");
    }

    #[test]
    fn state_equality() {
        assert_eq!(ServerState::Running, ServerState::Running);
        assert_ne!(ServerState::Running, ServerState::Failed);
        assert_ne!(ServerState::Stopped, ServerState::Failed);
    }

    #[test]
    fn symbol_kind_other_preserves_content() {
        let kind = SymbolKind::Other("custom_42".to_string());
        assert_eq!(kind, SymbolKind::Other("custom_42".to_string()));
        assert_ne!(kind, SymbolKind::Other("custom_99".to_string()));
        assert_ne!(kind, SymbolKind::Function);
    }

    #[test]
    fn symbol_range_equality() {
        let r1 = SymbolRange {
            start_line: 0,
            start_col: 0,
            end_line: 5,
            end_col: 10,
        };
        let r2 = SymbolRange {
            start_line: 0,
            start_col: 0,
            end_line: 5,
            end_col: 10,
        };
        assert_eq!(r1, r2);
    }

    #[test]
    fn can_transition_to_allows_normal_startup_path() {
        assert!(ServerState::Undetected.can_transition_to(ServerState::Available));
        assert!(ServerState::Available.can_transition_to(ServerState::Starting));
        assert!(ServerState::Starting.can_transition_to(ServerState::Running));
        assert!(ServerState::Running.can_transition_to(ServerState::Stopped));
    }

    #[test]
    fn can_transition_to_allows_install_path() {
        assert!(ServerState::Undetected.can_transition_to(ServerState::Missing));
        assert!(ServerState::Missing.can_transition_to(ServerState::Installing));
        assert!(ServerState::Installing.can_transition_to(ServerState::Available));
        assert!(ServerState::Installing.can_transition_to(ServerState::Failed));
    }

    #[test]
    fn can_transition_to_allows_failure_paths() {
        assert!(ServerState::Starting.can_transition_to(ServerState::Failed));
        assert!(ServerState::Running.can_transition_to(ServerState::Failed));
        assert!(ServerState::Available.can_transition_to(ServerState::Failed));
    }

    #[test]
    fn can_transition_to_allows_retry_from_failed() {
        assert!(ServerState::Failed.can_transition_to(ServerState::Available));
        assert!(ServerState::Failed.can_transition_to(ServerState::Installing));
        assert!(ServerState::Failed.can_transition_to(ServerState::Starting));
    }

    #[test]
    fn can_transition_to_rejects_invalid_jumps() {
        assert!(!ServerState::Undetected.can_transition_to(ServerState::Running));
        assert!(!ServerState::Undetected.can_transition_to(ServerState::Starting));
        assert!(!ServerState::Missing.can_transition_to(ServerState::Running));
        assert!(!ServerState::Available.can_transition_to(ServerState::Running));
        assert!(!ServerState::Installing.can_transition_to(ServerState::Running));
        assert!(!ServerState::Installing.can_transition_to(ServerState::Starting));
        assert!(!ServerState::Running.can_transition_to(ServerState::Undetected));
        assert!(!ServerState::Running.can_transition_to(ServerState::Available));
        assert!(!ServerState::Running.can_transition_to(ServerState::Starting));
        assert!(!ServerState::Stopped.can_transition_to(ServerState::Running));
        assert!(!ServerState::Stopped.can_transition_to(ServerState::Failed));
    }

    #[test]
    fn can_transition_to_rejects_self_transitions() {
        for state in [
            ServerState::Undetected,
            ServerState::Missing,
            ServerState::Available,
            ServerState::Installing,
            ServerState::Starting,
            ServerState::Running,
            ServerState::Stopped,
            ServerState::Failed,
        ] {
            assert!(
                !state.can_transition_to(state),
                "{:?} → {:?} should not be allowed",
                state,
                state
            );
        }
    }

    #[test]
    fn lsp_event_state_changed_fields() {
        let event = LspEvent::StateChanged {
            language: Language::Rust,
            old: ServerState::Starting,
            new: ServerState::Running,
        };
        match event {
            LspEvent::StateChanged { language, old, new } => {
                assert_eq!(language, Language::Rust);
                assert_eq!(old, ServerState::Starting);
                assert_eq!(new, ServerState::Running);
            }
            _ => panic!("wrong variant"),
        }
    }
}
