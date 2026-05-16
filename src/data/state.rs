use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use super::buffer::Buffer;
use super::file_tree::{FileEntry, FileTree};
use super::lsp::types::LspSharedState;

#[derive(Debug, Clone)]
pub struct ListDialogState {
    pub items: Vec<(String, usize, usize)>, // (val, line, col)
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Chord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor_line: usize,
    pub anchor_col: usize,
    pub head_line: usize,
    pub head_col: usize,
}

impl Selection {
    pub fn ordered(&self) -> (usize, usize, usize, usize) {
        if (self.anchor_line, self.anchor_col) <= (self.head_line, self.head_col) {
            (
                self.anchor_line,
                self.anchor_col,
                self.head_line,
                self.head_col,
            )
        } else {
            (
                self.head_line,
                self.head_col,
                self.anchor_line,
                self.anchor_col,
            )
        }
    }
}

#[derive(Debug)]
pub struct EditorState {
    pub buffers: Vec<Buffer>,
    pub active_buffer: usize,
    pub file_tree: Option<FileTree>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub mode: Mode,
    pub should_quit: bool,
    pub status_msg: String,
    pub tree_selected: usize,
    pub focus_tree: bool,
    pub chord_input: String,
    pub show_exit_modal: bool,
    pub opened_path: PathBuf,
    pub chord_cursor_col: usize,
    pub chord_error: bool,
    pub chord_running: bool,
    pub chord_history: Vec<String>,
    pub chord_history_index: Option<usize>,
    pub pre_tree_mode: Mode,
    pub pending_open_path: Option<PathBuf>,
    pub tree_view: Vec<FileEntry>,
    pub lsp_state: Arc<Mutex<LspSharedState>>,
    pub selection: Option<Selection>,
    pub list_dialog: Option<ListDialogState>,
}

impl EditorState {
    pub fn for_file(path: &Path) -> Result<Self> {
        let buf = if path.exists() {
            Buffer::from_file(path)?
        } else {
            Buffer::empty(path)
        };
        Ok(Self {
            buffers: vec![buf],
            active_buffer: 0,
            file_tree: None,
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            mode: Mode::Chord,
            should_quit: false,
            status_msg: String::new(),
            tree_selected: 0,
            focus_tree: false,
            chord_input: String::new(),
            show_exit_modal: false,
            opened_path: path.to_path_buf(),
            chord_cursor_col: 0,
            chord_error: false,
            chord_running: false,
            chord_history: Vec::new(),
            chord_history_index: None,
            pre_tree_mode: Mode::Chord,
            pending_open_path: None,
            tree_view: Vec::new(),
            lsp_state: Arc::new(Mutex::new(LspSharedState::default())),
            selection: None,
            list_dialog: None,
        })
    }

    pub fn for_directory(path: &Path) -> Result<Self> {
        let tree = FileTree::from_dir(path)?;
        let tree_view: Vec<FileEntry> = tree
            .entries
            .iter()
            .filter(|e| e.depth == 0)
            .cloned()
            .collect();
        Ok(Self {
            buffers: Vec::new(),
            active_buffer: 0,
            file_tree: Some(tree),
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            mode: Mode::Chord,
            should_quit: false,
            status_msg: String::new(),
            tree_selected: 0,
            focus_tree: true,
            chord_input: String::new(),
            show_exit_modal: false,
            opened_path: path.to_path_buf(),
            chord_cursor_col: 0,
            chord_error: false,
            chord_running: false,
            chord_history: Vec::new(),
            chord_history_index: None,
            pre_tree_mode: Mode::Chord,
            pending_open_path: None,
            tree_view,
            lsp_state: Arc::new(Mutex::new(LspSharedState::default())),
            selection: None,
            list_dialog: None,
        })
    }

    pub fn current_buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.active_buffer)
    }

    pub fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active_buffer)
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        if let Some(idx) = self.buffers.iter().position(|b| b.path == path) {
            self.active_buffer = idx;
        } else {
            let buf = Buffer::from_file(path)?;
            self.buffers.push(buf);
            self.active_buffer = self.buffers.len() - 1;
        }
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
        self.focus_tree = false;
        Ok(())
    }

    pub fn snapshot_contents(&self) -> Vec<(PathBuf, String)> {
        self.buffers
            .iter()
            .map(|b| (b.path.clone(), b.content()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn for_file_initializes_no_tree_and_empty_tree_view() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.rs");
        fs::write(&path, "fn main() {}").unwrap();

        let state = EditorState::for_file(&path).unwrap();

        assert!(state.file_tree.is_none());
        assert!(state.tree_view.is_empty());
    }

    #[test]
    fn for_file_chord_fields_initialized_to_defaults() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.rs");
        fs::write(&path, "fn main() {}").unwrap();

        let state = EditorState::for_file(&path).unwrap();

        assert_eq!(state.chord_cursor_col, 0);
        assert!(!state.chord_error);
        assert!(!state.chord_running);
    }

    #[test]
    fn for_directory_tree_view_contains_only_depth_zero_entries() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("file.rs"), "").unwrap();
        fs::write(tmp.path().join("subdir/nested.rs"), "").unwrap();

        let state = EditorState::for_directory(tmp.path()).unwrap();

        assert!(!state.tree_view.is_empty());
        for entry in &state.tree_view {
            assert_eq!(entry.depth, 0, "unexpected depth for {:?}", entry.path);
        }
        let tree = state.file_tree.as_ref().unwrap();
        assert!(
            tree.entries.iter().any(|e| e.depth > 0),
            "full tree should have nested entries"
        );
    }

    #[test]
    fn for_directory_chord_fields_initialized_to_defaults() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("x.rs"), "").unwrap();

        let state = EditorState::for_directory(tmp.path()).unwrap();

        assert_eq!(state.chord_cursor_col, 0);
        assert!(!state.chord_error);
        assert!(!state.chord_running);
    }

    #[test]
    fn selection_ordered_forward_drag() {
        let sel = Selection {
            anchor_line: 0,
            anchor_col: 5,
            head_line: 2,
            head_col: 10,
        };
        assert_eq!(sel.ordered(), (0, 5, 2, 10));
    }

    #[test]
    fn selection_ordered_backward_drag() {
        let sel = Selection {
            anchor_line: 2,
            anchor_col: 10,
            head_line: 0,
            head_col: 5,
        };
        assert_eq!(sel.ordered(), (0, 5, 2, 10));
    }

    #[test]
    fn selection_ordered_same_line_backward() {
        let sel = Selection {
            anchor_line: 3,
            anchor_col: 20,
            head_line: 3,
            head_col: 5,
        };
        assert_eq!(sel.ordered(), (3, 5, 3, 20));
    }
}
