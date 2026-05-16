use std::path::{Path, PathBuf};

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

pub fn server_for_language(lang: Language) -> Option<&'static LspServerInfo> {
    SERVERS.iter().find(|s| s.language == lang).copied()
}

pub fn detect_language_from_path(path: &Path) -> Option<Language> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(Language::from_extension)
}

const MANIFESTS: &[(&str, Language)] = &[
    ("Cargo.toml", Language::Rust),
    ("go.mod", Language::Go),
    ("go.work", Language::Go),
    ("package.json", Language::TypeScript),
    ("tsconfig.json", Language::TypeScript),
    ("pyproject.toml", Language::Python),
    ("pyrightconfig.json", Language::Python),
    ("setup.py", Language::Python),
];

/// Walk up from `path` looking for project manifest files. Returns all
/// languages whose manifests are found anywhere in the ancestor chain.
pub fn detect_languages_from_dir(path: &Path) -> Vec<Language> {
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

/// Backwards-compatible single-language detection. Returns the first
/// detected language, preferring manifest-based detection.
pub fn detect_language_from_dir(path: &Path) -> Option<Language> {
    detect_languages_from_dir(path).into_iter().next()
}

/// Resolve workspace root for a specific language. Different languages
/// have different root-finding strategies:
/// - Rust: topmost Cargo.toml (workspace root over member crate)
/// - Go: topmost go.work, else topmost go.mod
/// - TypeScript: nearest (innermost) tsconfig.json
/// - Python: nearest pyrightconfig.json, then pyproject.toml
pub fn workspace_root_for_language(path: &Path, lang: Language) -> Option<PathBuf> {
    match lang {
        Language::Rust => topmost_ancestor_with(path, "Cargo.toml"),
        Language::Go => {
            topmost_ancestor_with(path, "go.work").or_else(|| topmost_ancestor_with(path, "go.mod"))
        }
        Language::TypeScript => nearest_ancestor_with(path, "tsconfig.json")
            .or_else(|| nearest_ancestor_with(path, "package.json")),
        Language::Python => nearest_ancestor_with(path, "pyrightconfig.json")
            .or_else(|| nearest_ancestor_with(path, "pyproject.toml")),
        Language::Markdown
        | Language::Json
        | Language::Yaml
        | Language::Toml
        | Language::Dockerfile
        | Language::Xml => None,
    }
}

/// Same as the old `workspace_root_for_dir` — returns topmost Cargo.toml
/// directory. Kept for backwards compatibility.
pub fn workspace_root_for_dir(path: &Path) -> Option<PathBuf> {
    workspace_root_for_language(path, Language::Rust)
}

fn topmost_ancestor_with(path: &Path, filename: &str) -> Option<PathBuf> {
    let mut topmost: Option<PathBuf> = None;
    let mut cur: Option<&Path> = Some(path);
    while let Some(p) = cur {
        if p.join(filename).exists() {
            topmost = Some(p.to_path_buf());
        }
        cur = p.parent();
    }
    topmost
}

fn nearest_ancestor_with(path: &Path, filename: &str) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(path);
    while let Some(p) = cur {
        if p.join(filename).exists() {
            return Some(p.to_path_buf());
        }
        cur = p.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_server_lookup() {
        let server = server_for_language(Language::Rust).unwrap();
        assert_eq!(server.server_name, "rust-analyzer");
        assert_eq!(server.binary_name, "rust-analyzer");
    }

    #[test]
    fn go_server_lookup() {
        let server = server_for_language(Language::Go).unwrap();
        assert_eq!(server.server_name, "gopls");
    }

    #[test]
    fn typescript_server_lookup() {
        let server = server_for_language(Language::TypeScript).unwrap();
        assert_eq!(server.server_name, "vtsls");
    }

    #[test]
    fn python_server_lookup() {
        let server = server_for_language(Language::Python).unwrap();
        assert_eq!(server.server_name, "basedpyright-langserver");
    }

    #[test]
    fn markdown_has_no_server() {
        assert!(server_for_language(Language::Markdown).is_none());
    }

    #[test]
    fn detect_from_extension() {
        assert_eq!(
            detect_language_from_path(std::path::Path::new("main.rs")),
            Some(Language::Rust)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("main.go")),
            Some(Language::Go)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("index.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("app.py")),
            Some(Language::Python)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("README.md")),
            Some(Language::Markdown)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("config.json")),
            Some(Language::Json)
        );
        assert_eq!(
            detect_language_from_path(std::path::Path::new("unknown.zzz")),
            None
        );
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

        let langs = detect_languages_from_dir(&member);
        assert!(langs.contains(&Language::Rust));
        let resolved = workspace_root_for_dir(&member).unwrap();
        assert_eq!(resolved, root);
    }

    #[test]
    fn detect_from_dir_returns_empty_outside_any_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_languages_from_dir(tmp.path()).is_empty());
        assert_eq!(workspace_root_for_dir(tmp.path()), None);
    }

    #[test]
    fn detect_languages_multi_language() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(root.join("go.mod"), "module example\n").unwrap();

        let langs = detect_languages_from_dir(root);
        assert!(langs.contains(&Language::Rust));
        assert!(langs.contains(&Language::Go));
    }

    #[test]
    fn detect_languages_from_dir_markdown_not_in_dir_detection() {
        // Markdown has no manifest file — a dir with only .md files returns []
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("README.md"), "# Hello\n").unwrap();
        std::fs::write(root.join("CHANGELOG.md"), "## Changes\n").unwrap();

        let langs = detect_languages_from_dir(root);
        assert!(
            !langs.contains(&Language::Markdown),
            "Markdown must not appear from manifest detection"
        );
        assert!(
            langs.is_empty(),
            "dir with only .md files should produce no detected languages"
        );
    }

    #[test]
    fn workspace_root_go_prefers_go_work() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("go.work"), "go 1.21\n").unwrap();
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("go.mod"), "module sub\n").unwrap();

        let resolved = workspace_root_for_language(&sub, Language::Go).unwrap();
        assert_eq!(resolved, root);
    }

    #[test]
    fn workspace_root_typescript_nearest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
        let sub = root.join("packages/app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("tsconfig.json"), "{}").unwrap();

        let resolved = workspace_root_for_language(&sub, Language::TypeScript).unwrap();
        assert_eq!(resolved, sub);
    }
}
