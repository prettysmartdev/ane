use anyhow::Result;

use crate::commands::chord_engine::types::{ChordAction, EditorMode};
use crate::data::state::{EditorState, Mode};

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

impl ApplyChordAction for TuiFrontend {
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
        }

        if let Some(ref cursor) = action.cursor_destination {
            state.cursor_line = cursor.line;
            state.cursor_col = cursor.col;
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
