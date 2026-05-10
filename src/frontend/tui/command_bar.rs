use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::data::state::{EditorState, Mode};

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    match state.mode {
        Mode::Chord => {
            let prompt = Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
            let input = Span::styled(&state.chord_input, Style::default().fg(Color::White));
            let cursor = Span::styled("█", Style::default().fg(Color::Cyan));

            let line = Line::from(vec![prompt, input, cursor]);
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Chord ")
                .border_style(Style::default().fg(Color::Cyan));
            let paragraph = Paragraph::new(line).block(block);
            frame.render_widget(paragraph, area);
        }
        Mode::Edit => {
            let msg = if state.status_msg.is_empty() {
                "-- EDIT --"
            } else {
                &state.status_msg
            };
            let line = Line::from(Span::styled(
                format!(" {msg}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let paragraph = Paragraph::new(line).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}
