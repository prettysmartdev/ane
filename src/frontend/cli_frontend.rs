use std::sync::Arc;

use anyhow::Result;

use crate::commands::chord::FrontendCapabilities;
use crate::commands::chord_engine::types::{ChordAction, ListFrontend, ListItem};
use crate::commands::lsp_engine::InstallProgress;
use crate::data::state::EditorState;

use super::traits::ApplyChordAction;

pub struct CliInstallProgress;

impl InstallProgress for CliInstallProgress {
    fn on_stdout(&self, line: &str) {
        println!("{line}");
    }
    fn on_stderr(&self, line: &str) {
        eprintln!("{line}");
    }
    fn on_failed(&self, message: &str) {
        eprintln!("{message}");
    }
    fn on_complete(&self) {}
}

pub fn cli_install_progress() -> Arc<dyn InstallProgress> {
    Arc::new(CliInstallProgress)
}

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

impl FrontendCapabilities for CliFrontend {
    fn is_interactive(&self) -> bool {
        false
    }
}

impl ListFrontend for CliFrontend {
    fn show_list(&mut self, _state: &mut EditorState, items: &[ListItem]) -> Result<()> {
        for item in items {
            println!("{}:{}  {}", item.line + 1, item.col + 1, item.val);
        }
        Ok(())
    }
}

impl ApplyChordAction for CliFrontend {
    fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<String> {
        if !action.listed_items.is_empty() {
            self.show_list(state, &action.listed_items)?;
            return Ok(String::new());
        }
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
            listed_items: vec![],
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
            listed_items: vec![],
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

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn cli_frontend_is_not_interactive() {
        let frontend = CliFrontend::new();
        assert!(!frontend.is_interactive());
    }

    // --- work item 0011: List action ---

    #[test]
    fn show_list_format_is_one_indexed_line_and_col() {
        // The format string used by show_list: "{}:{}  {}", line+1, col+1, val
        use crate::commands::chord_engine::types::ListItem;
        let item = ListItem { val: "foo".to_string(), line: 0, col: 0 };
        let formatted = format!("{}:{}  {}", item.line + 1, item.col + 1, item.val);
        assert_eq!(formatted, "1:1  foo");

        let item2 = ListItem { val: "bar".to_string(), line: 4, col: 0 };
        let formatted2 = format!("{}:{}  {}", item2.line + 1, item2.col + 1, item2.val);
        assert_eq!(formatted2, "5:1  bar");
    }

    #[test]
    fn apply_with_listed_items_returns_empty_string() {
        use crate::commands::chord_engine::types::ListItem;
        let (_f, mut state) = make_state("hello");
        let action = ChordAction {
            buffer_name: "test".to_string(),
            diff: None,
            yanked_content: None,
            cursor_destination: None,
            mode_after: None,
            highlight_ranges: vec![],
            warnings: vec![],
            listed_items: vec![
                ListItem { val: "foo".to_string(), line: 0, col: 0 },
                ListItem { val: "bar".to_string(), line: 4, col: 0 },
            ],
        };
        let mut frontend = CliFrontend::new();
        let result = frontend.apply(&mut state, &action).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn apply_with_empty_listed_items_falls_through_to_diff() {
        let (_f, mut state) = make_state("old");
        let action = diff_action("new");
        let mut frontend = CliFrontend::new();
        let result = frontend.apply(&mut state, &action).unwrap();
        assert_eq!(result, "new");
    }
}
