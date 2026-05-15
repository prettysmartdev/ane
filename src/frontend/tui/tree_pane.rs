use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::data::file_tree::FileEntry;
use crate::data::state::EditorState;

fn entry_display_width(entry: &FileEntry) -> usize {
    2 * entry.depth + 2 + entry.name().chars().count()
}

pub fn content_width(tree_view: &[FileEntry]) -> usize {
    tree_view.iter().map(entry_display_width).max().unwrap_or(0)
}

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    let border_style = if state.focus_tree {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = state
        .file_tree
        .as_ref()
        .map(|t| {
            t.root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .border_style(border_style);

    if state.tree_view.is_empty() {
        frame.render_widget(block, area);
        return;
    }

    let inner = block.inner(area);
    let inner_height = inner.height as usize;
    let inner_width = inner.width as usize;

    let v_scroll = if state.tree_selected >= inner_height {
        state.tree_selected - inner_height + 1
    } else {
        0
    };

    let h_scroll = state
        .tree_view
        .get(state.tree_selected)
        .map(|sel| entry_display_width(sel).saturating_sub(inner_width))
        .unwrap_or(0);

    let lines: Vec<Line> = state
        .tree_view
        .iter()
        .enumerate()
        .skip(v_scroll)
        .take(inner_height)
        .map(|(i, entry)| {
            let indent = "  ".repeat(entry.depth);
            let icon = if entry.is_dir {
                let is_expanded = state
                    .tree_view
                    .get(i + 1)
                    .map(|next| next.depth > entry.depth)
                    .unwrap_or(false);
                if is_expanded { "▾" } else { "▸" }
            } else {
                " "
            };
            let name = entry.name();
            let display = format!("{indent}{icon} {name}");

            let scrolled: String = display.chars().skip(h_scroll).collect();

            let is_selected = i == state.tree_selected;
            let style = if is_selected && state.focus_tree {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            let text = if is_selected && state.focus_tree {
                format!("{scrolled:<inner_width$}")
            } else {
                scrolled
            };

            Line::from(Span::styled(text, style))
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub fn expand(state: &mut EditorState, idx: usize) {
    let entry = match state.tree_view.get(idx) {
        Some(e) if e.is_dir => e.clone(),
        _ => return,
    };

    let already_expanded = state
        .tree_view
        .get(idx + 1)
        .map(|next| next.depth > entry.depth)
        .unwrap_or(false);
    if already_expanded {
        return;
    }

    let tree = match &state.file_tree {
        Some(t) => t,
        None => return,
    };

    let children: Vec<_> = tree
        .entries
        .iter()
        .filter(|e| {
            e.depth == entry.depth + 1 && e.path.parent().map(|p| p == entry.path).unwrap_or(false)
        })
        .cloned()
        .collect();

    for (offset, child) in children.into_iter().enumerate() {
        state.tree_view.insert(idx + 1 + offset, child);
    }
}

pub fn collapse(state: &mut EditorState, idx: usize) {
    let entry = match state.tree_view.get(idx) {
        Some(e) if e.is_dir => e.clone(),
        _ => return,
    };

    let is_expanded = state
        .tree_view
        .get(idx + 1)
        .map(|next| next.depth > entry.depth)
        .unwrap_or(false);
    if !is_expanded {
        return;
    }

    let mut end = idx + 1;
    while end < state.tree_view.len() && state.tree_view[end].depth > entry.depth {
        end += 1;
    }

    state.tree_view.drain(idx + 1..end);
    state.tree_selected = state
        .tree_selected
        .min(state.tree_view.len().saturating_sub(1));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::data::file_tree::{FileEntry, FileTree};
    use crate::data::lsp::types::LspSharedState;
    use crate::data::state::{EditorState, Mode};

    use super::{collapse, expand};

    fn make_tree_state() -> EditorState {
        let root = PathBuf::from("/test");
        let a = root.join("A");
        let a_b = root.join("A/B");
        let a_b_c = root.join("A/B/C");
        let a_b_c_d = root.join("A/B/C/d.rs");

        let all_entries = vec![
            FileEntry {
                path: a.clone(),
                depth: 0,
                is_dir: true,
            },
            FileEntry {
                path: a_b.clone(),
                depth: 1,
                is_dir: true,
            },
            FileEntry {
                path: a_b_c.clone(),
                depth: 2,
                is_dir: true,
            },
            FileEntry {
                path: a_b_c_d.clone(),
                depth: 3,
                is_dir: false,
            },
        ];
        let tree_view = vec![FileEntry {
            path: a.clone(),
            depth: 0,
            is_dir: true,
        }];

        EditorState {
            buffers: Vec::new(),
            active_buffer: 0,
            file_tree: Some(FileTree {
                root: root.clone(),
                entries: all_entries,
            }),
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
            opened_path: root,
            chord_cursor_col: 0,
            chord_error: false,
            chord_running: false,
            chord_history: Vec::new(),
            chord_history_index: None,
            pre_tree_mode: Mode::Chord,
            pending_open_path: None,
            tree_view,
            lsp_state: Arc::new(Mutex::new(LspSharedState::default())),
        }
    }

    #[test]
    fn initial_tree_view_contains_only_root_dir() {
        let state = make_tree_state();
        assert_eq!(state.tree_view.len(), 1);
        assert_eq!(state.tree_view[0].path, PathBuf::from("/test/A"));
    }

    #[test]
    fn expand_a_shows_direct_child() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        assert_eq!(state.tree_view.len(), 2);
        assert_eq!(state.tree_view[0].path, PathBuf::from("/test/A"));
        assert_eq!(state.tree_view[1].path, PathBuf::from("/test/A/B"));
    }

    #[test]
    fn expand_a_b_shows_grandchild() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        expand(&mut state, 1);
        assert_eq!(state.tree_view.len(), 3);
        assert_eq!(state.tree_view[2].path, PathBuf::from("/test/A/B/C"));
    }

    #[test]
    fn expand_a_b_c_shows_file() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        expand(&mut state, 1);
        expand(&mut state, 2);
        assert_eq!(state.tree_view.len(), 4);
        assert_eq!(state.tree_view[3].path, PathBuf::from("/test/A/B/C/d.rs"));
    }

    #[test]
    fn collapse_a_b_c_removes_file_and_clamps_selected() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        expand(&mut state, 1);
        expand(&mut state, 2);
        state.tree_selected = 3;

        collapse(&mut state, 2);

        assert_eq!(state.tree_view.len(), 3);
        assert_eq!(state.tree_view[2].path, PathBuf::from("/test/A/B/C"));
        assert_eq!(state.tree_selected, 2, "tree_selected should be clamped");
    }

    #[test]
    fn collapse_a_drains_entire_subtree() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        expand(&mut state, 1);
        expand(&mut state, 2);

        collapse(&mut state, 0);

        assert_eq!(state.tree_view.len(), 1);
        assert_eq!(state.tree_view[0].path, PathBuf::from("/test/A"));
    }

    #[test]
    fn expand_already_expanded_dir_is_noop() {
        let mut state = make_tree_state();
        expand(&mut state, 0);
        let paths_before: Vec<PathBuf> = state.tree_view.iter().map(|e| e.path.clone()).collect();

        expand(&mut state, 0);

        let paths_after: Vec<PathBuf> = state.tree_view.iter().map(|e| e.path.clone()).collect();
        assert_eq!(paths_before, paths_after);
    }
}
