use anyhow::Result;

use crate::commands::chord_engine::types::ChordAction;
use crate::data::state::EditorState;

use super::traits::ApplyChordAction;

pub struct CliFrontend;

impl Default for CliFrontend {
    fn default() -> Self {
        Self
    }
}

impl CliFrontend {
    pub fn new() -> Self {
        Self
    }
}

impl ApplyChordAction for CliFrontend {
    fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<String> {
        if let Some(ref diff) = action.diff {
            if let Some(buf) = state.current_buffer_mut() {
                let new_lines: Vec<String> = diff.modified.lines().map(String::from).collect();
                buf.lines = if new_lines.is_empty() {
                    vec![String::new()]
                } else {
                    new_lines
                };
                buf.dirty = true;
            }
            Ok(diff.modified.clone())
        } else if let Some(ref yanked) = action.yanked_content {
            Ok(yanked.clone())
        } else {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::chord_engine::types::UnifiedDiff;
    use crate::data::state::EditorState;
    use std::io::Write;

    fn make_state(content: &str) -> (tempfile::NamedTempFile, EditorState) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        let state = EditorState::for_file(f.path()).unwrap();
        (f, state)
    }

    fn diff_action(modified: &str) -> ChordAction {
        ChordAction {
            buffer_name: "test".to_string(),
            diff: Some(UnifiedDiff {
                original: String::new(),
                modified: modified.to_string(),
                hunks: vec![],
            }),
            yanked_content: None,
            cursor_destination: None,
            mode_after: None,
            highlight_ranges: vec![],
            warnings: vec![],
        }
    }

    fn yank_action(content: &str) -> ChordAction {
        ChordAction {
            buffer_name: "test".to_string(),
            diff: None,
            yanked_content: Some(content.to_string()),
            cursor_destination: None,
            mode_after: None,
            highlight_ranges: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn apply_diff_updates_buffer_lines_and_returns_modified_content() {
        let (_f, mut state) = make_state("old line 1\nold line 2");
        let action = diff_action("new line 1\nnew line 2");

        let mut frontend = CliFrontend::new();
        let result = frontend.apply(&mut state, &action).unwrap();

        assert_eq!(result, "new line 1\nnew line 2");
        let buf = state.current_buffer().unwrap();
        assert_eq!(buf.lines, vec!["new line 1", "new line 2"]);
        assert!(buf.dirty);
    }

    #[test]
    fn apply_yank_returns_content_without_modifying_buffer() {
        let (_f, mut state) = make_state("original line 1\noriginal line 2");
        let original_lines = state.current_buffer().unwrap().lines.clone();
        let action = yank_action("yanked content");

        let mut frontend = CliFrontend::new();
        let result = frontend.apply(&mut state, &action).unwrap();

        assert_eq!(result, "yanked content");
        let buf = state.current_buffer().unwrap();
        assert_eq!(
            buf.lines, original_lines,
            "yank must not modify buffer lines"
        );
        assert!(!buf.dirty, "yank must not mark buffer dirty");
    }
}
