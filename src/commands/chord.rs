use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};

use crate::data::buffer::Buffer;

use super::chord_engine::types::ChordQuery;
use super::chord_engine::ChordEngine;
use super::lsp_engine::{LspEngine, LspEngineConfig};

pub fn parse_chord(input: &str) -> Result<ChordQuery> {
    ChordEngine::parse(input)
}

pub struct ChordResult {
    pub original: String,
    pub modified: String,
    pub warnings: Vec<String>,
    pub yanked: Option<String>,
}

pub fn execute_chord(path: &Path, chord: &ChordQuery) -> Result<ChordResult> {
    let buffer = if path.exists() {
        Buffer::from_file(path)?
    } else {
        bail!("file not found: {}", path.display());
    };

    let original = buffer.content();
    let path_str = path.to_string_lossy().to_string();
    let mut buffers = HashMap::new();
    buffers.insert(path_str.clone(), buffer);

    let mut lsp = LspEngine::new(LspEngineConfig::default());

    let resolved = ChordEngine::resolve(chord, &buffers, &mut lsp)?;
    let actions = ChordEngine::patch(&resolved, &buffers)?;

    if let Some(action) = actions.get(&path_str) {
        let modified = if let Some(ref diff) = action.diff {
            diff.modified.clone()
        } else {
            original.clone()
        };

        if modified != original {
            std::fs::write(path, &modified)?;
        }

        Ok(ChordResult {
            original,
            modified,
            warnings: action.warnings.clone(),
            yanked: action.yanked_content.clone(),
        })
    } else {
        Ok(ChordResult {
            original: original.clone(),
            modified: original,
            warnings: Vec::new(),
            yanked: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::chord_types::{Action, Component, Positional, Scope};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_short_form_cifv() {
        let parsed = parse_chord("cifv").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Inside);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Value);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_long_form() {
        let parsed = parse_chord("ChangeInsideFunctionValue").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Inside);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Value);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_change_entire_line_self() {
        let parsed = parse_chord("cels").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
        assert!(!parsed.requires_lsp);
    }

    #[test]
    fn parse_delete_entire_line_self() {
        let parsed = parse_chord("dels").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
    }

    #[test]
    fn parse_delete_entire_line_self_long() {
        let parsed = parse_chord("DeleteEntireLineSelf").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
    }

    #[test]
    fn parse_yank_entire_function_value() {
        let parsed = parse_chord("yefv").unwrap();
        assert_eq!(parsed.action, Action::Yank);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Value);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_append_after_line_end() {
        let parsed = parse_chord("aale").unwrap();
        assert_eq!(parsed.action, Action::Append);
        assert_eq!(parsed.positional, Positional::After);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::End);
        assert!(!parsed.requires_lsp);
    }

    #[test]
    fn parse_with_args() {
        let parsed = parse_chord("cifp(function:getData, value:\"(x: i32)\")").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Parameters);
        assert_eq!(parsed.args.target_name.as_deref(), Some("getData"));
        assert_eq!(parsed.args.value.as_deref(), Some("(x: i32)"));
    }

    #[test]
    fn parse_invalid_combination() {
        let result = parse_chord("cilp");
        assert!(result.is_err());
    }

    #[test]
    fn execute_change_line() {
        let f = temp_file("aaa\nbbb\nccc");
        let mut chord = parse_chord("cels").unwrap();
        chord.args.target_line = Some(1);
        chord.args.value = Some("xxx".to_string());
        let result = execute_chord(f.path(), &chord).unwrap();
        assert!(result.modified.contains("xxx"));
        assert!(!result.modified.contains("bbb"));
    }

    #[test]
    fn execute_delete_line() {
        let f = temp_file("aaa\nbbb\nccc");
        let mut chord = parse_chord("dels").unwrap();
        chord.args.target_line = Some(1);
        let result = execute_chord(f.path(), &chord).unwrap();
        assert!(!result.modified.contains("bbb"));
        assert!(result.modified.contains("aaa"));
        assert!(result.modified.contains("ccc"));
    }

    #[test]
    fn parse_unknown_chord() {
        let result = parse_chord("zzzz");
        assert!(result.is_err());
    }

    #[test]
    fn parse_delete_entire_buffer_self() {
        let parsed = parse_chord("debs").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Buffer);
        assert_eq!(parsed.component, Component::Self_);
        assert!(!parsed.requires_lsp);
    }
}
