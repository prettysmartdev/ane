use std::path::Path;

use crate::data::lsp::registry;
use crate::data::lsp::types::Language;

pub fn detect_languages(root_path: &Path, files: &[&Path]) -> Vec<Language> {
    let mut langs = Vec::new();

    if let Some(lang) = registry::detect_language_from_dir(root_path) {
        if !langs.contains(&lang) {
            langs.push(lang);
        }
    }

    for file in files {
        if let Some(lang) = registry::detect_language_from_path(file) {
            if !langs.contains(&lang) {
                langs.push(lang);
            }
        }
    }

    langs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_from_rs_extension() {
        let files = [Path::new("src/main.rs")];
        let langs = detect_languages(Path::new("/tmp"), &files);
        assert!(langs.contains(&Language::Rust));
    }

    #[test]
    fn detects_rust_from_cargo_toml_in_dir() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let langs = detect_languages(root, &[]);
        assert!(langs.contains(&Language::Rust));
    }

    #[test]
    fn no_duplicates_from_multiple_rs_files() {
        let files = [Path::new("src/main.rs"), Path::new("src/lib.rs")];
        let langs = detect_languages(Path::new("/tmp"), &files);
        let count = langs.iter().filter(|&&l| l == Language::Rust).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn no_duplicates_when_dir_and_file_both_detect_rust() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = [Path::new("src/main.rs")];
        let langs = detect_languages(root, &files);
        let count = langs.iter().filter(|&&l| l == Language::Rust).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn unknown_extensions_produce_no_languages() {
        let files = [Path::new("README.md"), Path::new("config.json")];
        let langs = detect_languages(Path::new("/tmp"), &files);
        assert!(langs.is_empty());
    }

    #[test]
    fn empty_inputs_produce_no_languages() {
        let langs = detect_languages(Path::new("/tmp"), &[]);
        assert!(langs.is_empty());
    }

    #[test]
    fn non_rs_file_in_non_cargo_dir_produces_nothing() {
        let files = [Path::new("main.py")];
        let langs = detect_languages(Path::new("/tmp"), &files);
        assert!(langs.is_empty());
    }

    #[test]
    fn rs_file_without_cargo_toml_dir_still_detects_rust() {
        let files = [Path::new("foo.rs")];
        let langs = detect_languages(Path::new("/tmp"), &files);
        assert!(langs.contains(&Language::Rust));
    }
}
