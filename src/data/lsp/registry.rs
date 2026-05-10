use super::types::{Language, LspServerInfo};

static RUST_ANALYZER: LspServerInfo = LspServerInfo {
    language: Language::Rust,
    server_name: "rust-analyzer",
    binary_name: "rust-analyzer",
    install_command: "rustup component add rust-analyzer",
    check_command: "rust-analyzer --version",
    default_args: &[],
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

pub fn detect_language_from_dir(path: &std::path::Path) -> Option<Language> {
    if path.join("Cargo.toml").exists() {
        return Some(Language::Rust);
    }
    None
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
}
