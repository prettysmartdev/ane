use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspStatus {
    Unknown,
    NotInstalled,
    Installing,
    Starting,
    Ready,
    Failed,
}

impl LspStatus {
    pub fn display(&self) -> &'static str {
        match self {
            Self::Unknown => "LSP: checking...",
            Self::NotInstalled => "LSP: not installed",
            Self::Installing => "LSP: installing...",
            Self::Starting => "LSP: starting...",
            Self::Ready => "LSP: ready",
            Self::Failed => "LSP: failed",
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Unknown | Self::Installing | Self::Starting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolLocation {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone)]
pub struct LspServerInfo {
    pub language: Language,
    pub server_name: &'static str,
    pub binary_name: &'static str,
    pub install_command: &'static str,
    pub check_command: &'static str,
    pub default_args: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct LspInitParams {
    pub root_path: PathBuf,
    pub language: Language,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detection() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("py"), None);
        assert_eq!(Language::from_extension(""), None);
    }

    #[test]
    fn status_transitions() {
        assert!(LspStatus::Ready.is_available());
        assert!(!LspStatus::Failed.is_available());
        assert!(LspStatus::Starting.is_pending());
        assert!(!LspStatus::Ready.is_pending());
    }
}
