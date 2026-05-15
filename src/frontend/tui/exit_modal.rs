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

fn centered_rect(width_chars: u16, height: u16, area: Rect) -> Rect {
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
