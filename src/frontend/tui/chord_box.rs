use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, editor_area: Rect, state: &EditorState) {
    if editor_area.width < 4 || editor_area.height < 5 {
        return;
    }

    let width = editor_area.width.saturating_sub(2);
    let height = 3;
    let x = editor_area.x + 1;
    let y = editor_area.bottom().saturating_sub(4);

    let area = Rect::new(x, y, width, height);

    let (border_color, text_color) = if state.chord_running {
        (Color::Yellow, Color::DarkGray)
    } else if state.chord_error {
        (Color::Red, Color::White)
    } else {
        (Color::Blue, Color::White)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Chord ")
        .border_style(Style::default().fg(border_color));

    let input = &state.chord_input;
    let col = state.chord_cursor_col.min(input.len());
    let (before, after) = input.split_at(col);

    let spans = vec![
        Span::styled(" > ", Style::default().fg(Color::Cyan)),
        Span::styled(before, Style::default().fg(text_color)),
        Span::styled(after, Style::default().fg(text_color)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(block);

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);

    if !state.chord_running {
        let cursor_x = area.x + 1 + 3 + col as u16;
        let cursor_y = area.y + 1;
        if cursor_x < area.right() && cursor_y < area.bottom() {
            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }
}
