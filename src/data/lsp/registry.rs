use super::types::{Language, LspServerInfo};

static RUST_ANALYZER: LspServerInfo = LspServerInfo {
    language: Language::Rust,
    server_name: "rust-analyzer",
    binary_name: "rust-analyzer",
    install_command: "rustup component add rust-analyzer",
    check_command: "rust-analyzer --version",
    default_args: &[],
    init_options_json: r#"{"checkOnSave":false}"#,
};

static SERVERS: &[&LspServerInfo] = &[&RUST_ANALYZER];

pub fn server_for_language(lang: Language) -> Option<&'static LspServerInfo> {
    SERVERS.iter().find(|s| s.language == lang).copied()
}

pub fn detect_language_from_path(path: &std::path::Path) -> Option<Language> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(Language::from_extension)
}

/// Walk up from `path` looking for a Cargo.toml. The topmost (closest to
/// filesystem root) Cargo.toml wins, so a workspace with nested member
/// crates resolves to the workspace root rather than a member.
pub fn detect_language_from_dir(path: &std::path::Path) -> Option<Language> {
    let mut topmost: Option<&std::path::Path> = None;
    let mut cur: Option<&std::path::Path> = Some(path);
    while let Some(p) = cur {
        if p.join("Cargo.toml").exists() {
            topmost = Some(p);
        }
        cur = p.parent();
    }
    topmost.map(|_| Language::Rust)
}

/// Same as `detect_language_from_dir` but returns the resolved root path
/// (topmost Cargo.toml directory), if any. Useful for setting `rootUri`
/// when starting a server for a nested file.
pub fn workspace_root_for_dir(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut topmost: Option<std::path::PathBuf> = None;
    let mut cur: Option<&std::path::Path> = Some(path);
    while let Some(p) = cur {
        if p.join("Cargo.toml").exists() {
            topmost = Some(p.to_path_buf());
        }
        cur = p.parent();
    }
    topmost
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rust_server_lookup() {
        let server = server_for_language(Language::Rust).unwrap();
        assert_eq!(server.server_name, "rust-analyzer");
        assert_eq!(server.binary_name, "rust-analyzer");
    }

    #[test]
    fn detect_from_extension() {
        assert_eq!(
            detect_language_from_path(Path::new("main.rs")),
            Some(Language::Rust)
        );
        assert_eq!(detect_language_from_path(Path::new("main.py")), None);
    }

    #[test]
    fn detect_from_dir_finds_workspace_root_from_member() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\n").unwrap();
        let member = root.join("a");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname=\"a\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();

        assert_eq!(detect_language_from_dir(&member), Some(Language::Rust));
        let resolved = workspace_root_for_dir(&member).unwrap();
        assert_eq!(resolved, root);
    }

    #[test]
    fn detect_from_dir_returns_none_outside_any_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_language_from_dir(tmp.path()), None);
        assert_eq!(workspace_root_for_dir(tmp.path()), None);
    }
}
