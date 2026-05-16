use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

use crate::commands::chord_engine::types::ListItem as ChordListItem;

#[derive(Debug, Clone)]
pub struct ListDialog {
    pub items: Vec<ChordListItem>,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl ListDialog {
    pub fn new(items: Vec<ChordListItem>) -> Self {
        Self {
            items,
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() && self.selected < self.items.len() - 1 {
            self.selected += 1;
        }
    }
}

pub fn render(frame: &mut Frame, dialog: &ListDialog) {
    let area = frame.area();
    let width = (area.width * 3 / 4).max(30).min(area.width);
    let height = (area.height * 3 / 4).max(10).min(area.height);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    if dialog.items.is_empty() {
        let block = Block::default()
            .title(" List Results ")
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);
        let msg = List::new(vec![ListItem::new("No results")]);
        frame.render_widget(msg, inner);
        return;
    }

    let block = Block::default()
        .title(" List Results ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let visible_height = inner.height as usize;
    let items: Vec<ListItem> = dialog
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let line_num = format!("{:>4}:{:<3}", item.line + 1, item.col + 1);
            let style = if i == dialog.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    line_num,
                    Style::default().fg(if i == dialog.selected {
                        Color::Black
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::raw("  "),
                Span::styled(&item.val, style),
            ]))
            .style(style)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(dialog.selected));

    let list = List::new(items).highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    // Adjust offset for scrolling
    let _ = visible_height;
    frame.render_stateful_widget(list, inner, &mut list_state);
}
