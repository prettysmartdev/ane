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

pub(crate) fn byte_col_from_display(line: &str, target_display: usize) -> usize {
    let mut display = 0;
    let mut byte_idx = 0;
    for c in line.chars() {
        let w = if c == '\t' { 4 } else { 1 };
        if display + w > target_display {
            break;
        }
        display += w;
        byte_idx += c.len_utf8();
    }
    byte_idx
}

pub(crate) fn wrap_offsets(line: &str, text_width: usize) -> Vec<usize> {
    if text_width == 0 {
        return vec![0];
    }
    let expanded = expand_tabs(line);
    let chars: Vec<char> = expanded.chars().collect();
    if chars.is_empty() {
        return vec![0];
    }

    let mut offsets = vec![0usize];
    let mut col = 0;
    let mut last_space = None;
    let mut row_start = 0;

    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            last_space = Some(i);
        }
        col += 1;

        if col >= text_width && i + 1 < chars.len() {
            if let Some(sp) = last_space {
                if sp > row_start {
                    let break_at = sp + 1;
                    offsets.push(break_at);
                    row_start = break_at;
                    col = (i + 1) - break_at;
                    last_space = None;
                    continue;
                }
            }
            let break_at = i + 1;
            offsets.push(break_at);
            row_start = break_at;
            col = 0;
            last_space = None;
        }
    }

    offsets
}

pub(crate) fn visual_row_count(line: &str, text_width: usize) -> usize {
    wrap_offsets(line, text_width).len()
}

pub(crate) fn display_col_to_wrap_pos(offsets: &[usize], display_col: usize) -> (usize, usize) {
    let mut row = 0;
    for (i, &off) in offsets.iter().enumerate().rev() {
        if display_col >= off {
            row = i;
            break;
        }
    }
    (row, display_col - offsets[row])
}

pub(crate) fn wrap_row_start(offsets: &[usize], row: usize) -> usize {
    offsets.get(row).copied().unwrap_or(0)
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

    let full_text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let offsets = wrap_offsets(&full_text, text_width);

    let mut span_chars: Vec<(char, Style)> = Vec::new();
    for span in &spans {
        let style = span.style;
        for ch in span.content.chars() {
            span_chars.push((ch, style));
        }
    }

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    for w in 0..offsets.len() {
        let start = offsets[w];
        let end = if w + 1 < offsets.len() {
            offsets[w + 1]
        } else {
            span_chars.len()
        };
        let slice = &span_chars[start..end];

        let mut row: Vec<Span<'static>> = Vec::new();
        if !slice.is_empty() {
            let mut cur_style = slice[0].1;
            let mut cur_text = String::new();
            for &(ch, style) in slice {
                if style != cur_style {
                    if !cur_text.is_empty() {
                        row.push(Span::styled(cur_text, cur_style));
                        cur_text = String::new();
                    }
                    cur_style = style;
                }
                cur_text.push(ch);
            }
            if !cur_text.is_empty() {
                row.push(Span::styled(cur_text, cur_style));
            }
        }
        rows.push(row);
    }

    if rows.is_empty() {
        rows.push(Vec::new());
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
            let line_num_width = format!("{}", buf.line_count().saturating_sub(1)).len();
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
                    let offsets = wrap_offsets(line_text, text_width);
                    let (c_wrap_row, c_col_in_row) =
                        display_col_to_wrap_pos(&offsets, cursor_display_col);
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
                        format!("{:>width$} ", logical_line, width = line_num_width)
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
                        if let Some(cell) = frame
                            .buffer_mut()
                            .cell_mut(Position::new(cursor_x, cursor_y))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_offsets_empty_line() {
        assert_eq!(wrap_offsets("", 80), vec![0]);
    }

    #[test]
    fn wrap_offsets_short_line_no_wrap() {
        assert_eq!(wrap_offsets("hello world", 80), vec![0]);
    }

    #[test]
    fn wrap_offsets_breaks_on_space() {
        // "hello world" with width 8: "hello " fits (6 chars), then "wo" fills to 8
        // but word-wrap should break after "hello " to keep "world" whole
        let offsets = wrap_offsets("hello world", 8);
        assert_eq!(offsets, vec![0, 6]);
    }

    #[test]
    fn wrap_offsets_long_word_falls_back_to_char_break() {
        let offsets = wrap_offsets("abcdefghij", 5);
        assert_eq!(offsets, vec![0, 5]);
    }

    #[test]
    fn wrap_offsets_multiple_wraps() {
        let offsets = wrap_offsets("aaa bbb ccc ddd", 8);
        assert_eq!(offsets, vec![0, 8]);
    }

    #[test]
    fn wrap_offsets_exact_fit_no_extra_row() {
        let offsets = wrap_offsets("12345", 5);
        assert_eq!(offsets, vec![0]);
    }

    #[test]
    fn visual_row_count_uses_word_wrap() {
        assert_eq!(visual_row_count("hello world", 8), 2);
        assert_eq!(visual_row_count("hi", 80), 1);
    }

    #[test]
    fn display_col_to_wrap_pos_basic() {
        let offsets = vec![0, 6];
        assert_eq!(display_col_to_wrap_pos(&offsets, 0), (0, 0));
        assert_eq!(display_col_to_wrap_pos(&offsets, 5), (0, 5));
        assert_eq!(display_col_to_wrap_pos(&offsets, 6), (1, 0));
        assert_eq!(display_col_to_wrap_pos(&offsets, 8), (1, 2));
    }

    #[test]
    fn wrap_row_start_basic() {
        let offsets = vec![0, 6, 12];
        assert_eq!(wrap_row_start(&offsets, 0), 0);
        assert_eq!(wrap_row_start(&offsets, 1), 6);
        assert_eq!(wrap_row_start(&offsets, 2), 12);
        assert_eq!(wrap_row_start(&offsets, 5), 0);
    }
}
