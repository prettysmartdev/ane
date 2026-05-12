use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::data::lsp::types::{SemanticToken, ServerState};
use crate::data::state::{EditorState, Mode};

fn token_type_color(token_type: &str) -> Color {
    match token_type {
        "keyword" | "modifier" => Color::Blue,
        "function" | "method" => Color::Yellow,
        "type" | "class" | "struct" | "enum" | "interface" | "typeParameter" => Color::Cyan,
        "string" => Color::Green,
        "number" => Color::Magenta,
        "comment" => Color::DarkGray,
        "variable" | "parameter" | "property" => Color::White,
        "macro" => Color::LightMagenta,
        "operator" => Color::LightRed,
        "namespace" => Color::LightCyan,
        "enumMember" => Color::Cyan,
        _ => Color::Gray,
    }
}

fn styled_line_with_tokens<'a>(
    line_text: &'a str,
    line_num: usize,
    tokens: &[SemanticToken],
    base_style: Style,
) -> Vec<Span<'a>> {
    let line_tokens: Vec<&SemanticToken> = tokens.iter().filter(|t| t.line == line_num).collect();

    if line_tokens.is_empty() {
        return vec![Span::styled(line_text.to_string(), base_style)];
    }

    let mut spans = Vec::new();
    let mut pos = 0;
    let chars: Vec<char> = line_text.chars().collect();

    for token in &line_tokens {
        let start = token.start_col;
        let end = (token.start_col + token.length).min(chars.len());

        if start > pos {
            let text: String = chars[pos..start.min(chars.len())].iter().collect();
            spans.push(Span::styled(text, base_style));
        }

        if start < chars.len() {
            let text: String = chars[start..end].iter().collect();
            let color = token_type_color(&token.token_type);
            spans.push(Span::styled(text, Style::default().fg(color)));
        }

        pos = end;
    }

    if pos < chars.len() {
        let text: String = chars[pos..].iter().collect();
        spans.push(Span::styled(text, base_style));
    }

    spans
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_status: ServerState,
    semantic_tokens: &[SemanticToken],
) {
    match state.current_buffer() {
        Some(buf) => {
            let visible_height = area.height as usize;
            let start = state.scroll_offset;
            let end = (start + visible_height).min(buf.line_count());

            let line_num_width = format!("{}", buf.line_count()).len();
            let use_highlighting = lsp_status == ServerState::Running;

            let lines: Vec<Line> = buf.lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line_text)| {
                    let line_num = start + i;
                    let num_str = format!("{:>width$} ", line_num + 1, width = line_num_width);

                    let is_current = line_num == state.cursor_line;
                    let num_style = if is_current && state.mode == Mode::Edit {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let text_style = if is_current && state.mode == Mode::Edit {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    let mut spans = vec![Span::styled(num_str, num_style)];

                    if use_highlighting {
                        let token_spans = styled_line_with_tokens(
                            line_text,
                            line_num,
                            semantic_tokens,
                            text_style,
                        );
                        spans.extend(token_spans);
                    } else {
                        spans.push(Span::styled(line_text.clone(), text_style));
                    }

                    Line::from(spans)
                })
                .collect();

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, area);

            if state.mode == Mode::Edit && !state.focus_tree {
                let cursor_row = state.cursor_line.saturating_sub(start);
                let cursor_x = area.x + line_num_width as u16 + 1 + state.cursor_col as u16;
                let cursor_y = area.y + cursor_row as u16;
                if state.cursor_line >= start
                    && state.cursor_line < end
                    && cursor_x < area.right()
                    && cursor_y < area.bottom()
                {
                    frame.set_cursor_position(Position::new(cursor_x, cursor_y));
                }
            } else if state.mode == Mode::Chord
                && !state.focus_tree
                && state.cursor_line >= start
                && state.cursor_line < end
            {
                let cursor_row = state.cursor_line - start;
                let cursor_x = area.x + line_num_width as u16 + 1 + state.cursor_col as u16;
                let cursor_y = area.y + cursor_row as u16;
                if cursor_x < area.right() && cursor_y < area.bottom() {
                    let cursor_char = buf
                        .lines
                        .get(state.cursor_line)
                        .and_then(|l| l.chars().nth(state.cursor_col))
                        .unwrap_or(' ');
                    let cell = Rect::new(cursor_x, cursor_y, 1, 1);
                    let widget = Paragraph::new(cursor_char.to_string())
                        .style(Style::default().bg(Color::Blue).fg(Color::White));
                    frame.render_widget(widget, cell);
                }
            }
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
            let paragraph = Paragraph::new(welcome);
            frame.render_widget(paragraph, area);
        }
    }
}
