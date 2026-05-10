use anyhow::{bail, Result};

use crate::commands::chord::ParsedChord;
use crate::data::chord_types::Scope;
use crate::data::state::{EditorState, Mode};

use crate::frontend::traits::*;

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

impl ChangeFrontend for TuiFrontend {
    fn execute_change(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let line: usize = chord
                    .target
                    .as_deref()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(state.cursor_line);

                if let Some(buf) = state.current_buffer_mut() {
                    if line < buf.line_count() {
                        buf.lines[line].clear();
                        buf.dirty = true;
                    }
                }

                state.cursor_line = line;
                state.cursor_col = 0;
                state.mode = Mode::Edit;
                state.status_msg = "-- EDIT --".into();
                Ok("entered edit mode at cleared line".into())
            }
            Scope::Function => {
                // LSP-dependent: would delete function body, position cursor, enter edit mode
                state.status_msg = format!(
                    "chord {} requires LSP (status: {})",
                    chord.spec.short_form(),
                    state.lsp_status.display()
                );
                bail!(
                    "chord {} requires LSP — function body editing not yet wired to LSP",
                    chord.spec.short_form()
                )
            }
            _ => bail!(
                "chord {} not yet implemented in TUI",
                chord.spec.short_form()
            ),
        }
    }
}

impl DeleteFrontend for TuiFrontend {
    fn execute_delete(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let line: usize = chord
                    .target
                    .as_deref()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(state.cursor_line);

                if let Some(buf) = state.current_buffer_mut() {
                    buf.remove_line(line);
                }

                if state.cursor_line > 0 {
                    state.cursor_line = state.cursor_line.saturating_sub(1);
                }
                state.status_msg = format!("deleted line {}", line + 1);
                Ok(format!("deleted line {}", line + 1))
            }
            _ if chord.spec.scope.requires_lsp() => {
                bail!(
                    "chord {} requires LSP — not yet wired",
                    chord.spec.short_form()
                )
            }
            _ => bail!(
                "chord {} not yet implemented in TUI",
                chord.spec.short_form()
            ),
        }
    }
}

impl InsertFrontend for TuiFrontend {
    fn execute_insert(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let line: usize = chord
                    .target
                    .as_deref()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(state.cursor_line);

                if let Some(buf) = state.current_buffer_mut() {
                    buf.insert_line(line, String::new());
                }

                state.cursor_line = line;
                state.cursor_col = 0;
                state.mode = Mode::Edit;
                state.status_msg = "-- EDIT --".into();
                Ok("inserted blank line, entered edit mode".into())
            }
            _ => bail!(
                "chord {} not yet implemented in TUI",
                chord.spec.short_form()
            ),
        }
    }
}

impl ReadFrontend for TuiFrontend {
    fn execute_read(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        let content = state
            .current_buffer()
            .map(|b| b.content())
            .unwrap_or_default();
        let _ = chord;
        state.status_msg = format!("{} bytes", content.len());
        Ok(content)
    }
}

impl MoveFrontend for TuiFrontend {
    fn execute_move(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        match chord.spec.scope {
            Scope::Line => {
                let line: usize = chord.target.as_deref().unwrap_or("0").parse().unwrap_or(0);

                let max = state
                    .current_buffer()
                    .map(|b| b.line_count().saturating_sub(1))
                    .unwrap_or(0);
                state.cursor_line = line.min(max);
                state.cursor_col = 0;
                state.status_msg = format!("moved to line {}", state.cursor_line + 1);
                Ok(format!("moved to line {}", state.cursor_line + 1))
            }
            _ => bail!(
                "chord {} not yet implemented in TUI",
                chord.spec.short_form()
            ),
        }
    }
}

impl SelectFrontend for TuiFrontend {
    fn execute_select(&mut self, _state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        bail!(
            "chord {} not yet implemented in TUI",
            chord.spec.short_form()
        )
    }
}

impl YankFrontend for TuiFrontend {
    fn execute_yank(&mut self, state: &mut EditorState, chord: &ParsedChord) -> Result<String> {
        self.execute_read(state, chord)
    }
}

impl ChordFrontend for TuiFrontend {}
