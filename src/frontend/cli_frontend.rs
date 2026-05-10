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
