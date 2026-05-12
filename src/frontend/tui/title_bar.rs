use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
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

    let name_span = Span::styled(format!(" {name} "), Style::default().fg(Color::White));

    let indicator = if dirty {
        Span::styled("[+]", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };

    let padding_len = area
        .width
        .saturating_sub(name.len() as u16 + 2 + if dirty { 3 } else { 0 });
    let padding = Span::raw(" ".repeat(padding_len as usize));

    let line = Line::from(vec![name_span, padding, indicator]);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
