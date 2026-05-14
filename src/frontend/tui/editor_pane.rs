use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
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

fn expand_tabs(s: &str) -> String {
    s.replace('\t', "    ")
}

pub(crate) fn display_col(line: &str, byte_idx: usize) -> usize {
    let mut safe_idx = byte_idx.min(line.len());
    while safe_idx > 0 && !line.is_char_boundary(safe_idx) {
        safe_idx -= 1;
    }
    line[..safe_idx]
        .chars()
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

pub(crate) fn visual_row_count(line: &str, text_width: usize) -> usize {
    if text_width == 0 {
        return 1;
    }
    let display_width: usize = line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
    if display_width == 0 {
        return 1;
    }
    (display_width + text_width - 1) / text_width
}

fn styled_line_with_tokens<'a>(
    line_text: &'a str,
    line_num: usize,
    tokens: &[SemanticToken],
    base_style: Style,
) -> Vec<Span<'a>> {
    let line_tokens: Vec<&SemanticToken> = tokens.iter().filter(|t| t.line == line_num).collect();

    if line_tokens.is_empty() {
        return vec![Span::styled(expand_tabs(line_text), base_style)];
    }

    let mut spans = Vec::new();
    let mut pos = 0;
    let chars: Vec<char> = line_text.chars().collect();

    for token in &line_tokens {
        let start = token.start_col;
        let end = (token.start_col + token.length).min(chars.len());

        if start > pos {
            let text: String = chars[pos..start.min(chars.len())].iter().collect();
            spans.push(Span::styled(expand_tabs(&text), base_style));
        }

        if start < chars.len() {
            let text: String = chars[start..end].iter().collect();
            let color = token_type_color(&token.token_type);
            spans.push(Span::styled(expand_tabs(&text), Style::default().fg(color)));
        }

        pos = end;
    }

    if pos < chars.len() {
        let text: String = chars[pos..].iter().collect();
        spans.push(Span::styled(expand_tabs(&text), base_style));
    }

    spans
}

fn wrap_spans(spans: Vec<Span<'_>>, text_width: usize) -> Vec<Vec<Span<'static>>> {
    if text_width == 0 {
        let row = spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect();
        return vec![row];
    }

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_row: Vec<Span<'static>> = Vec::new();
    let mut col: usize = 0;

    for span in spans {
        let style = span.style;
        let chars: Vec<char> = span.content.chars().collect();
        let mut pos = 0;

        while pos < chars.len() {
            let space_left = text_width - col;
            let remaining = chars.len() - pos;
            let take = space_left.min(remaining);

            let chunk: String = chars[pos..pos + take].iter().collect();
            if !chunk.is_empty() {
                current_row.push(Span::styled(chunk, style));
            }
            col += take;
            pos += take;

            if col >= text_width {
                rows.push(current_row);
                current_row = Vec::new();
                col = 0;
            }
        }
    }

    if !current_row.is_empty() || rows.is_empty() {
        rows.push(current_row);
    }

    rows
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_status: ServerState,
    semantic_tokens: &[SemanticToken],
) {
    frame.render_widget(Clear, area);
    match state.current_buffer() {
        Some(buf) => {
            let visible_height = area.height as usize;
            let line_num_width = format!("{}", buf.line_count()).len();
            let text_width = (area.width as usize).saturating_sub(line_num_width + 1);
            let use_highlighting = lsp_status == ServerState::Running;

            let mut visual_lines: Vec<Line> = Vec::new();
            let mut cursor_visual_row: Option<usize> = None;
            let mut cursor_visual_col: Option<usize> = None;
            let mut logical_line = state.scroll_offset;

            while visual_lines.len() < visible_height && logical_line < buf.line_count() {
                let line_text = &buf.lines[logical_line];
                let is_current = logical_line == state.cursor_line;

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

                let content_spans = if use_highlighting {
                    styled_line_with_tokens(line_text, logical_line, semantic_tokens, text_style)
                } else {
                    vec![Span::styled(expand_tabs(line_text), text_style)]
                };

                let wrapped_rows = wrap_spans(content_spans, text_width);

                if is_current {
                    let cursor_display_col = display_col(line_text, state.cursor_col);
                    let (c_wrap_row, c_col_in_row) = if text_width > 0 {
                        (
                            cursor_display_col / text_width,
                            cursor_display_col % text_width,
                        )
                    } else {
                        (0, cursor_display_col)
                    };
                    let max_row = wrapped_rows.len().saturating_sub(1);
                    let (final_row, final_col) = if c_wrap_row > max_row {
                        let last_row_len: usize = wrapped_rows.last().map_or(0, |spans| {
                            spans.iter().map(|s| s.content.chars().count()).sum()
                        });
                        (max_row, last_row_len)
                    } else {
                        (c_wrap_row, c_col_in_row)
                    };
                    cursor_visual_row = Some(visual_lines.len() + final_row);
                    cursor_visual_col = Some(final_col);
                }

                for (wrap_idx, row_spans) in wrapped_rows.into_iter().enumerate() {
                    if visual_lines.len() >= visible_height {
                        break;
                    }

                    let num_str = if wrap_idx == 0 {
                        format!("{:>width$} ", logical_line + 1, width = line_num_width)
                    } else {
                        " ".repeat(line_num_width + 1)
                    };

                    let mut spans = vec![Span::styled(num_str, num_style)];
                    spans.extend(row_spans);
                    visual_lines.push(Line::from(spans));
                }

                logical_line += 1;
            }

            let paragraph = Paragraph::new(visual_lines);
            frame.render_widget(paragraph, area);

            if let (Some(vis_row), Some(vis_col)) = (cursor_visual_row, cursor_visual_col) {
                let cursor_x = area.x + line_num_width as u16 + 1 + vis_col as u16;
                let cursor_y = area.y + vis_row as u16;
                if cursor_x < area.right() && cursor_y < area.bottom() {
                    if state.mode == Mode::Edit && !state.focus_tree {
                        frame.set_cursor_position(Position::new(cursor_x, cursor_y));
                    } else if state.mode == Mode::Chord && !state.focus_tree {
                        if let Some(cell) =
                            frame.buffer_mut().cell_mut(Position::new(cursor_x, cursor_y))
                        {
                            cell.set_style(Style::default().bg(Color::Blue).fg(Color::White));
                        }
                    }
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
