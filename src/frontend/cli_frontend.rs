use anyhow::{bail, Result};

use crate::commands::chord::{self, ParsedChord};
use crate::data::chord_types::Scope;
use crate::data::state::EditorState;

use super::traits::*;

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

impl ChangeFrontend for CliFrontend {
    fn execute_change(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let path = &state
                    .current_buffer()
                    .map(|b| b.path.clone())
                    .unwrap_or_default();
                let result = chord::execute_chord(path, chord)?;
                Ok(result.modified)
            }
            scope if scope.requires_lsp() => {
                bail!(
                    "chord {} requires LSP — not yet implemented in CLI mode",
                    chord.spec.long_form()
                );
            }
            _ => bail!("chord {} not implemented", chord.spec.long_form()),
        }
    }
}

impl DeleteFrontend for CliFrontend {
    fn execute_delete(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let path = &state
                    .current_buffer()
                    .map(|b| b.path.clone())
                    .unwrap_or_default();
                let result = chord::execute_chord(path, chord)?;
                Ok(result.modified)
            }
            scope if scope.requires_lsp() => {
                bail!(
                    "chord {} requires LSP — not yet implemented in CLI mode",
                    chord.spec.long_form()
                );
            }
            _ => bail!("chord {} not implemented", chord.spec.long_form()),
        }
    }
}

impl InsertFrontend for CliFrontend {
    fn execute_insert(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let path = &state
                    .current_buffer()
                    .map(|b| b.path.clone())
                    .unwrap_or_default();
                let result = chord::execute_chord(path, chord)?;
                Ok(result.modified)
            }
            _ => bail!("chord {} not implemented", chord.spec.long_form()),
        }
    }
}

impl ReadFrontend for CliFrontend {
    fn execute_read(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        let path = &state
            .current_buffer()
            .map(|b| b.path.clone())
            .unwrap_or_default();
        let result = chord::execute_chord(path, chord)?;
        Ok(result.modified)
    }
}

impl MoveFrontend for CliFrontend {
    fn execute_move(&mut self, _state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        bail!(
            "chord {} not yet implemented in CLI mode",
            chord.spec.long_form()
        )
    }
}

impl SelectFrontend for CliFrontend {
    fn execute_select(&mut self, _state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        bail!(
            "chord {} not yet implemented in CLI mode",
            chord.spec.long_form()
        )
    }
}

impl YankFrontend for CliFrontend {
    fn execute_yank(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        self.execute_read(state, chord)
    }
}

impl ChordFrontend for CliFrontend {}
