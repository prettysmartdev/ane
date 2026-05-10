use std::collections::HashMap;

use crate::data::chord_types::{Action, Component, Positional, Scope};

#[derive(Debug, Clone)]
pub struct ChordQuery {
    pub action: Action,
    pub positional: Positional,
    pub scope: Scope,
    pub component: Component,
    pub args: ChordArgs,
    pub requires_lsp: bool,
}

impl ChordQuery {
    pub fn short_form(&self) -> String {
        format!(
            "{}{}{}{}",
            self.action.short(),
            self.positional.short(),
            self.scope.short(),
            self.component.short(),
        )
    }

    pub fn long_form(&self) -> String {
        format!(
            "{}{}{}{}",
            self.action, self.positional, self.scope, self.component,
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChordArgs {
    pub target_name: Option<String>,
    pub parent_name: Option<String>,
    pub target_line: Option<usize>,
    pub cursor_pos: Option<(usize, usize)>,
    pub value: Option<String>,
    pub find: Option<String>,
    pub replace: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedChord {
    pub query: ChordQuery,
    pub resolutions: HashMap<String, BufferResolution>,
}

#[derive(Debug, Clone)]
pub struct BufferResolution {
    pub target_ranges: Vec<TextRange>,
    pub scope_range: TextRange,
    pub component_range: TextRange,
    pub replacement: Option<String>,
    pub cursor_destination: Option<CursorPosition>,
    pub mode_after: Option<EditorMode>,
}

impl BufferResolution {
    pub fn primary_target(&self) -> TextRange {
        self.target_ranges
            .first()
            .copied()
            .unwrap_or(TextRange::point(0, 0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl TextRange {
    pub fn point(line: usize, col: usize) -> Self {
        Self {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start_line == self.end_line && self.start_col == self.end_col
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Edit,
    Chord,
}

#[derive(Debug, Clone)]
pub struct ChordAction {
    pub buffer_name: String,
    pub diff: Option<UnifiedDiff>,
    pub yanked_content: Option<String>,
    pub cursor_destination: Option<CursorPosition>,
    pub mode_after: Option<EditorMode>,
    pub highlight_ranges: Vec<TextRange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UnifiedDiff {
    pub original: String,
    pub modified: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}
