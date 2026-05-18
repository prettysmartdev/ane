use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, state: &EditorState) {
    let is_dirty = state.current_buffer().is_some_and(|b| b.dirty);

    let (height, text) = if is_dirty {
        (
            8,
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Unsaved changes!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Ctrl-C again to quit without saving",
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    "  Ctrl-S to save and quit",
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    "  Esc to cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ],
        )
    } else {
        (
            7,
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Press Ctrl-C again to exit",
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    "  Press Esc to cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ],
        )
    };

    let area = centered_rect(50, height, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Exit ane? ")
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_open_modal(frame: &mut Frame) {
    let area = centered_rect(55, 8, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Unsaved Changes ")
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Unsaved changes in current file.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Ctrl-S  save and open",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Ctrl-O  discard and open",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "  Esc     cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_delete_confirm(frame: &mut Frame, state: &EditorState) {
    let delete_state = match &state.tree_delete_confirm {
        Some(d) => d,
        None => return,
    };

    let entry_name = state
        .tree_view
        .get(delete_state.index)
        .map(|e| e.name().to_string())
        .unwrap_or_default();

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Delete {entry_name}?"),
            Style::default().fg(Color::White),
        )),
    ];

    if !delete_state.children_preview.is_empty() {
        lines.push(Line::from(""));
        let max_show = 5;
        for child in delete_state.children_preview.iter().take(max_show) {
            lines.push(Line::from(Span::styled(
                format!("  \u{2022} {child}"),
                Style::default().fg(Color::Gray),
            )));
        }
        if delete_state.children_preview.len() > max_show {
            let remaining = delete_state.children_preview.len() - max_show;
            lines.push(Line::from(Span::styled(
                format!("  (and {remaining} more)"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Enter to confirm",
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(Span::styled(
        "  Esc to cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let height = (lines.len() as u16) + 2;
    let area = centered_rect(50, height, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Delete? ")
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub(super) fn centered_rect(width_chars: u16, height: u16, area: Rect) -> Rect {
    let popup_width = width_chars.min(area.width);
    let popup_height = height.min(area.height);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(popup_height)) / 2),
            Constraint::Length(popup_height),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(popup_width)) / 2),
            Constraint::Length(popup_width),
            Constraint::Min(0),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::data::file_tree::FileEntry;
    use crate::data::state::{EditorState, TreeDeleteState};

    #[test]
    fn render_delete_confirm_truncates_at_five_and_shows_remaining_count() {
        let mut state = EditorState::for_file(&PathBuf::from("dummy.rs")).unwrap();

        state.tree_view = vec![FileEntry {
            path: PathBuf::from("/dummy/mydir"),
            depth: 0,
            is_dir: true,
        }];
        let children: Vec<String> = (1..=8).map(|i| format!("child{i}.rs")).collect();
        state.tree_delete_confirm = Some(TreeDeleteState {
            index: 0,
            children_preview: children,
        });

        let backend = TestBackend::new(80, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_delete_confirm(frame, &state))
            .unwrap();

        let buf = terminal.backend().buffer().clone();
        let rendered: String = buf.content().iter().map(|c| c.symbol()).collect();

        for i in 1..=5 {
            assert!(
                rendered.contains(&format!("child{i}.rs")),
                "child{i}.rs should appear in the rendered modal"
            );
        }
        for i in 6..=8 {
            assert!(
                !rendered.contains(&format!("child{i}.rs")),
                "child{i}.rs should NOT appear (truncated)"
            );
        }
        assert!(
            rendered.contains("(and 3 more)"),
            "expected '(and 3 more)' in rendered output; full output: {rendered:?}"
        );
    }
}
