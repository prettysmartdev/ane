use anyhow::Result;

use crate::commands::chord::FrontendCapabilities;
use crate::commands::chord_engine::types::{ChordAction, EditorMode, ListFrontend, ListItem};
use crate::data::state::{EditorState, ListDialogState, Mode};

use crate::frontend::traits::ApplyChordAction;

pub struct TuiFrontend;

impl Default for TuiFrontend {
    fn default() -> Self {
        Self
    }
}

impl TuiFrontend {
    pub fn new() -> Self {
        Self
    }
}

impl FrontendCapabilities for TuiFrontend {
    fn is_interactive(&self) -> bool {
        true
    }
}

impl ListFrontend for TuiFrontend {
    fn show_list(&mut self, state: &mut EditorState, items: &[ListItem]) -> Result<()> {
        state.list_dialog = Some(ListDialogState {
            items: items
                .iter()
                .map(|i| (i.val.clone(), i.line, i.col))
                .collect(),
            selected: 0,
        });
        Ok(())
    }
}

impl ApplyChordAction for TuiFrontend {
    fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<String> {
        if !action.listed_items.is_empty() {
            self.show_list(state, &action.listed_items)?;
            return Ok(String::new());
        }
        if let Some(ref diff) = action.diff
            && let Some(buf) = state.current_buffer_mut()
        {
            let new_lines: Vec<String> = diff.modified.lines().map(String::from).collect();
            buf.lines = if new_lines.is_empty() {
                vec![String::new()]
            } else {
                new_lines
            };
            buf.dirty = true;
        }

        if let Some(ref cursor) = action.cursor_destination {
            let line_count = state.current_buffer().map(|b| b.line_count()).unwrap_or(1);
            state.cursor_line = cursor.line.min(line_count.saturating_sub(1));
            let line_len = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
                .map(|l| l.len())
                .unwrap_or(0);
            let mut col = cursor.col.min(line_len);
            if let Some(line) = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
            {
                while col > 0 && !line.is_char_boundary(col) {
                    col -= 1;
                }
            }
            state.cursor_col = col;
        }

        if let Some(ref mode) = action.mode_after {
            match mode {
                EditorMode::Edit => {
                    state.mode = Mode::Edit;
                    state.status_msg = "-- EDIT --".into();
                }
                EditorMode::Chord => {
                    state.mode = Mode::Chord;
                    state.status_msg.clear();
                }
            }
        }

        for warning in &action.warnings {
            state.status_msg = format!("warning: {warning}");
        }

        if let Some(ref yanked) = action.yanked_content {
            state.status_msg = format!("{} bytes yanked", yanked.len());
            return Ok(yanked.clone());
        }

        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::commands::chord_engine::types::{ChordAction, CursorPosition, EditorMode};
    use crate::data::state::{EditorState, Mode};
    use crate::frontend::traits::ApplyChordAction;

    fn jump_action(line: usize, col: usize) -> ChordAction {
        ChordAction {
            buffer_name: "test".to_string(),
            diff: None,
            yanked_content: None,
            cursor_destination: Some(CursorPosition { line, col }),
            mode_after: Some(EditorMode::Edit),
            highlight_ranges: vec![],
            warnings: vec![],
            listed_items: vec![],
        }
    }

    fn make_state(content: &str) -> (tempfile::NamedTempFile, EditorState) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        let state = EditorState::for_file(f.path()).unwrap();
        (f, state)
    }

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn tui_frontend_is_interactive() {
        assert!(TuiFrontend::new().is_interactive());
    }

    // --- work item 0011: List action ---

    #[test]
    fn tui_show_list_populates_list_dialog_state() {
        use crate::commands::chord_engine::types::ListItem;
        let (_f, mut state) = make_state("hello\nworld");
        let items = vec![
            ListItem {
                val: "foo".to_string(),
                line: 2,
                col: 5,
            },
            ListItem {
                val: "bar".to_string(),
                line: 7,
                col: 0,
            },
        ];
        let mut frontend = TuiFrontend::new();
        frontend.show_list(&mut state, &items).unwrap();
        let dialog = state
            .list_dialog
            .as_ref()
            .expect("list_dialog should be set");
        assert_eq!(dialog.items.len(), 2);
        assert_eq!(dialog.items[0], ("foo".to_string(), 2, 5));
        assert_eq!(dialog.items[1], ("bar".to_string(), 7, 0));
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn tui_apply_jump_updates_cursor_line_and_col() {
        let (_f, mut state) = make_state("line zero\nline one\nline two");
        let action = jump_action(2, 4);
        let mut frontend = TuiFrontend::new();
        frontend.apply(&mut state, &action).unwrap();
        assert_eq!(state.cursor_line, 2);
        assert_eq!(state.cursor_col, 4);
        assert_eq!(state.mode, Mode::Edit);
    }

    #[test]
    fn tui_apply_jump_clamps_col_to_line_length() {
        let (_f, mut state) = make_state("hi\nthere");
        // "hi" has 2 chars; requesting col 999 should clamp to 2
        let action = jump_action(0, 999);
        let mut frontend = TuiFrontend::new();
        frontend.apply(&mut state, &action).unwrap();
        assert_eq!(state.cursor_line, 0);
        assert_eq!(state.cursor_col, 2);
    }
}
