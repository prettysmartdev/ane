use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::data::state::{EditorState, Mode};

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    let title = match state.current_buffer() {
        Some(buf) => {
            let name = buf
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("untitled");
            let dirty = if buf.dirty { " [+]" } else { "" };
            format!(" {name}{dirty} ")
        }
        None => " ane ".to_string(),
    };

    let border_style = if !state.focus_tree {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);

    let inner = block.inner(area);

    match state.current_buffer() {
        Some(buf) => {
            let visible_height = inner.height as usize;
            let start = state.scroll_offset;
            let end = (start + visible_height).min(buf.line_count());

            let line_num_width = format!("{}", buf.line_count()).len();

            let lines: Vec<Line> = buf.lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line_text)| {
                    let line_num = start + i;
                    let num_str = format!("{:>width$} ", line_num + 1, width = line_num_width);

                    let is_current = line_num == state.cursor_line && state.mode != Mode::Chord;
                    let num_style = if is_current {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let text_style = if is_current {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    Line::from(vec![
                        Span::styled(num_str, num_style),
                        Span::styled(line_text.clone(), text_style),
                    ])
                })
                .collect();

            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, area);
        }
        None => {
            let welcome = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  ane — Agent Native Editor",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Select a file from the tree to begin editing",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  Ctrl-E to toggle Edit mode | Ctrl-T to open file tree",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let paragraph = Paragraph::new(welcome).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}
