use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    let width = area.width as usize;

    let (name, dirty) = match state.current_buffer() {
        Some(buf) => {
            let name = buf
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("untitled");
            (name.to_string(), buf.dirty)
        }
        None => {
            let name = state
                .opened_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("ane")
                .to_string();
            (name, false)
        }
    };

    let center_text = if dirty {
        format!("[+] {name}")
    } else {
        name.clone()
    };

    let right_text = format!(" {}:{} ", state.cursor_line + 1, state.cursor_col + 1);

    let center_len = center_text.len();
    let right_len = right_text.len();

    let center_start = width.saturating_sub(center_len) / 2;
    let center_end = center_start + center_len;
    let right_start = width.saturating_sub(right_len);

    let left_pad = center_start;
    let mid_pad = right_start.saturating_sub(center_end);

    let spans = vec![
        Span::styled(" ".repeat(left_pad), Style::default()),
        Span::styled(center_text, Style::default().fg(Color::White)),
        Span::styled(" ".repeat(mid_pad), Style::default()),
        Span::styled(right_text, Style::default().fg(Color::DarkGray)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
