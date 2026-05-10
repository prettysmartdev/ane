use anyhow::Result;

use crate::commands::chord::ParsedChord;
use crate::data::chord_types::Action;
use crate::data::state::EditorState;

pub trait ChangeFrontend {
    fn execute_change(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait DeleteFrontend {
    fn execute_delete(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait InsertFrontend {
    fn execute_insert(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait ReadFrontend {
    fn execute_read(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait MoveFrontend {
    fn execute_move(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait SelectFrontend {
    fn execute_select(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait YankFrontend {
    fn execute_yank(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String>;
}

pub trait ChordFrontend:
    ChangeFrontend
    + DeleteFrontend
    + InsertFrontend
    + ReadFrontend
    + MoveFrontend
    + SelectFrontend
    + YankFrontend
{
    fn dispatch(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.action {
            Action::Change => self.execute_change(state, chord),
            Action::Delete => self.execute_delete(state, chord),
            Action::Insert => self.execute_insert(state, chord),
            Action::Read => self.execute_read(state, chord),
            Action::Move => self.execute_move(state, chord),
            Action::Select => self.execute_select(state, chord),
            Action::Yank => self.execute_yank(state, chord),
        }
    }
}
