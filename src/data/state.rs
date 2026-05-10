use std::path::{Path, PathBuf};

use anyhow::Result;

use super::buffer::Buffer;
use super::file_tree::FileTree;
use super::lsp::types::LspStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Chord,
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
    pub lsp_status: LspStatus,
    pub opened_path: PathBuf,
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
            lsp_status: LspStatus::Unknown,
            opened_path: path.to_path_buf(),
        })
    }

    pub fn for_directory(path: &Path) -> Result<Self> {
        let tree = FileTree::from_dir(path)?;
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
            lsp_status: LspStatus::Unknown,
            opened_path: path.to_path_buf(),
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
