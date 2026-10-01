use anyhow::Result;

use crate::commands::chord::FrontendCapabilities;
use crate::commands::chord_engine::types::{ChordAction, EditorMode, ListFrontend, ListItem};
use crate::data::state::{EditorState, ListDialogState, Mode};

use crate::frontend::traits::ApplyChordAction;

use super::background_chord::{ChordResult, ChordWorker, Job};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

struct PendingChord {
    input: String,
    generation: u64,
    active_buffer: usize,
    path: Option<PathBuf>,
    cursor: (usize, usize),
    mode: Mode,
    focus_tree: bool,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<ChordResult>,
}
pub struct TuiFrontend {
    worker: ChordWorker,
    pending: Option<PendingChord>,
}

impl Default for TuiFrontend {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiFrontend {
    pub fn new() -> Self {
        Self {
            worker: ChordWorker::new(),
            pending: None,
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn cancel_chord(&mut self, state: &mut EditorState) {
        if let Some(pending) = self.pending.take() {
            pending.cancel.store(true, Ordering::Release);
            state.chord_running = false;
            if state.status_msg == "chord running — Esc to cancel" {
                state.status_msg = "chord cancelled".into();
            }
        }
    }

    pub fn submit_chord(
        &mut self,
        state: &mut EditorState,
        input: &str,
        query: crate::commands::chord_engine::types::ChordQuery,
        provider: Box<dyn crate::commands::lsp_engine::LspProvider + Send>,
        cancel: Arc<AtomicBool>,
    ) {
        self.cancel_chord(state);
        let mut buffers = std::collections::HashMap::new();
        if let Some(buffer) = state.current_buffer() {
            buffers.insert(buffer.path.to_string_lossy().into_owned(), buffer.clone());
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.worker.submit(Job {
            query,
            buffers,
            provider,
            cancel: Arc::clone(&cancel),
            reply,
        });
        self.pending = Some(PendingChord {
            input: input.into(),
            generation: state.buffer_generation,
            active_buffer: state.active_buffer,
            path: state.current_buffer().map(|b| b.path.clone()),
            cursor: (state.cursor_line, state.cursor_col),
            mode: state.mode,
            focus_tree: state.focus_tree,
            cancel,
            result,
        });
        state.chord_running = true;
        state.status_msg = "chord running — Esc to cancel".into();
    }

    /// Returns true after applying a completed chord; the caller refreshes caches.
    pub fn poll_chord(&mut self, state: &mut EditorState) -> bool {
        if state.show_exit_modal
            || state.pending_open_path.is_some()
            || state.list_dialog.is_some()
            || state.tree_rename_state.is_some()
            || state.tree_delete_confirm.is_some()
            || state.tree_new_file_state.is_some()
        {
            return false;
        }
        let Some(pending) = &self.pending else {
            return false;
        };
        if pending.generation != state.buffer_generation
            || pending.active_buffer != state.active_buffer
            || pending.path.as_deref() != state.current_buffer().map(|b| b.path.as_path())
            || pending.cursor != (state.cursor_line, state.cursor_col)
            || pending.mode != state.mode
            || pending.focus_tree != state.focus_tree
        {
            self.cancel_chord(state);
            return false;
        }
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("chord worker stopped".into()),
        };
        let pending = self.pending.take().unwrap();
        state.chord_running = false;
        state.status_msg.clear();
        match result {
            Ok(actions) => {
                for action in actions.values() {
                    match self.apply(state, action) {
                        Ok(message) if !message.is_empty() && state.status_msg.is_empty() => {
                            state.status_msg = message
                        }
                        Err(error) => {
                            state.status_msg = format!("error: {error}");
                            return false;
                        }
                        _ => {}
                    }
                }
                state.chord_history.push(pending.input);
                true
            }
            Err(error) => {
                state.status_msg = error;
                false
            }
        }
    }
}

impl Drop for TuiFrontend {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Release);
        }
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
    #[test]
    fn editing_cancels_a_pending_chord_and_discards_its_result() {
        use crate::commands::lsp_engine::LspProvider;
        use crate::data::lsp::types::{DocumentSymbol, SelectionRange};
        use std::path::Path;
        use std::time::Duration;
        struct Blocking {
            entered: mpsc::SyncSender<()>,
            release: mpsc::Receiver<()>,
        }
        impl LspProvider for Blocking {
            fn document_symbols(&mut self, _: &Path) -> Result<Vec<DocumentSymbol>> {
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(3)).unwrap();
                anyhow::bail!("delayed response")
            }
            fn selection_range(&mut self, _: &Path, _: usize, _: usize) -> Result<SelectionRange> {
                anyhow::bail!("unused")
            }
        }
        let (_f, mut state) = make_state("fn main() {}\n");
        let mut query = crate::commands::chord_engine::ChordEngine::parse("cifn").unwrap();
        query.args.cursor_pos = Some((0, 3));
        let (entered, started) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let mut frontend = TuiFrontend::new();
        frontend.submit_chord(
            &mut state,
            "cifn",
            query,
            Box::new(Blocking {
                entered,
                release: released,
            }),
            Arc::new(AtomicBool::new(false)),
        );
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        state.buffer_generation += 1;
        state.buffers[0].lines[0] = "edited while waiting".into();
        assert!(!frontend.poll_chord(&mut state));
        assert!(!frontend.has_pending());
        release.send(()).unwrap();
        assert!(!frontend.poll_chord(&mut state));
        assert_eq!(state.buffers[0].lines[0], "edited while waiting");
    }
}
