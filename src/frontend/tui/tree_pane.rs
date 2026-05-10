use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    let border_style = if state.focus_tree {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Files ")
        .border_style(border_style);

    let tree = match &state.file_tree {
        Some(t) => t,
        None => {
            frame.render_widget(block, area);
            return;
        }
    };

    let inner_height = block.inner(area).height as usize;

    let scroll = if state.tree_selected >= inner_height {
        state.tree_selected - inner_height + 1
    } else {
        0
    };

    let lines: Vec<Line> = tree
        .entries
        .iter()
        .enumerate()
        .skip(scroll)
        .take(inner_height)
        .map(|(i, entry)| {
            let indent = "  ".repeat(entry.depth);
            let icon = if entry.is_dir { "+" } else { " " };
            let name = entry.name();
            let display = format!("{indent}{icon} {name}");

            let is_selected = i == state.tree_selected;
            let style = if is_selected && state.focus_tree {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            Line::from(Span::styled(display, style))
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
