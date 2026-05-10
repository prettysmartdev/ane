use anyhow::Result;

use crate::commands::chord_engine::types::ChordAction;
use crate::data::state::EditorState;

pub trait ApplyChordAction {
    fn apply(&mut self, state: &mut EditorState, action: &ChordAction) -> Result<String>;
}
