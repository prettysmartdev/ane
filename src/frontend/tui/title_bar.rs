use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::data::state::EditorState;

pub fn compute_loc(lines: &[String]) -> usize {
    lines.iter().filter(|l| !l.trim().is_empty()).count()
}

pub fn compute_token_count(content: &str) -> usize {
    let enc = tiktoken::get_encoding("o200k_base")
        .expect("tiktoken o200k_base encoding must be available");
    enc.count(content)
}

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState) {
    let width = area.width as usize;

    let (name, dirty, loc, lines_count) = match state.current_buffer() {
        Some(buf) => {
            let name = buf
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("untitled");
            let loc = compute_loc(&buf.lines);
            let lines_count = buf.lines.len();
            (name.to_string(), buf.dirty, loc, lines_count)
        }
        None => {
            let name = state
                .opened_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("ane")
                .to_string();
            (name, false, 0, 0)
        }
    };

    let center_text = if dirty {
        format!("[+] {name}")
    } else {
        name.clone()
    };

    let right_text = format!(
        " {}:{} | {} loc | {} lines | ~{} tokens ",
        state.cursor_line, state.cursor_col, loc, lines_count, state.cached_token_count
    );

    let center_len = center_text.chars().count();
    let right_len = right_text.chars().count();

    // Hide the right block if it can't fit cleanly beside the centered
    // filename (need at least one space of separation). This avoids the
    // right text being truncated at the bar's right edge on narrow terminals.
    if right_len >= width || center_len + right_len + 1 > width {
        let spans = vec![Span::styled(center_text, Style::default().fg(Color::White))];
        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
        frame.render_widget(paragraph, area);
        return;
    }

    let right_start = width - right_len;
    // Center the filename within the gap left of the right block so the two
    // never overlap (the guard above already ensures center_len + 1 <= right_start).
    let center_start = right_start.saturating_sub(center_len) / 2;
    let center_end = center_start + center_len;

    let left_pad = center_start;
    let mid_pad = right_start - center_end;

    let spans = vec![
        Span::styled(" ".repeat(left_pad), Style::default()),
        Span::styled(center_text, Style::default().fg(Color::White)),
        Span::styled(" ".repeat(mid_pad), Style::default()),
        Span::styled(right_text, Style::default().fg(Color::White)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use tempfile::TempDir;

    use crate::data::state::EditorState;

    use super::{compute_loc, compute_token_count, render};

    // --- compute_loc ---

    #[test]
    fn compute_loc_all_blank_returns_zero() {
        let lines: Vec<String> = vec!["".into(), "   ".into(), "\t".into(), "  \t  ".into()];
        assert_eq!(compute_loc(&lines), 0);
    }

    #[test]
    fn compute_loc_mixed_content_counts_non_blank() {
        let lines: Vec<String> = vec![
            "fn main() {".into(),
            "".into(),
            "    let x = 1;".into(),
            "   ".into(),
            "}".into(),
        ];
        assert_eq!(compute_loc(&lines), 3);
    }

    #[test]
    fn compute_loc_whitespace_only_lines_treated_as_empty() {
        let lines: Vec<String> = vec!["  ".into(), "\t\t".into(), " \t ".into()];
        assert_eq!(compute_loc(&lines), 0);
    }

    // --- compute_token_count ---

    #[test]
    fn compute_token_count_empty_returns_zero() {
        assert_eq!(compute_token_count(""), 0);
    }

    #[test]
    fn compute_token_count_known_input_is_plausible() {
        // "hello world" encodes to 2 tokens with o200k_base; we assert a
        // reasonable range rather than pinning an exact tiktoken version.
        let count = compute_token_count("hello world");
        assert!(
            (1..=5).contains(&count),
            "expected 1-5 tokens for \"hello world\", got {count}"
        );
    }

    // --- render ---

    fn state_without_buffer() -> (TempDir, EditorState) {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "x").unwrap();
        let state = EditorState::for_directory(tmp.path()).unwrap();
        (tmp, state)
    }

    fn state_with_content(content: &str) -> (tempfile::NamedTempFile, EditorState) {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        let state = EditorState::for_file(f.path()).unwrap();
        (f, state)
    }

    #[test]
    fn render_does_not_panic_when_no_buffer() {
        let (_tmp, state) = state_without_buffer();
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, Rect::new(0, 0, 80, 1), &state))
            .unwrap();
    }

    #[test]
    fn render_right_block_fits_within_bar_width() {
        for width in [40u16, 80, 120] {
            let (_f, state) = state_with_content("fn main() {}\n");
            let backend = TestBackend::new(width, 1);
            let mut terminal = Terminal::new(backend).unwrap();
            let cf = terminal
                .draw(|frame| render(frame, Rect::new(0, 0, width, 1), &state))
                .unwrap();
            assert_eq!(
                cf.area.width, width,
                "frame width should match terminal width={width}"
            );
            let row: String = (0..width)
                .map(|x| cf.buffer.cell((x, 0)).unwrap().symbol().to_string())
                .collect();
            // The right block is either fully present (its end inside [0, width))
            // or entirely hidden when there isn't room beside the filename.
            // It must never be partially rendered.
            if let Some(idx) = row.find("tokens") {
                assert!(
                    idx + "tokens".len() <= width as usize,
                    "right info block must end within bar width={width} (row={row:?})"
                );
            }
        }
    }

    #[test]
    fn render_right_block_visible_when_filename_short() {
        // Filename "a" is short, so the right block must be visible at all three
        // widths and fully inside [0, width).
        for width in [40u16, 80, 120] {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("a");
            std::fs::write(&path, "").unwrap();
            let state = EditorState::for_file(&path).unwrap();
            let backend = TestBackend::new(width, 1);
            let mut terminal = Terminal::new(backend).unwrap();
            let cf = terminal
                .draw(|frame| render(frame, Rect::new(0, 0, width, 1), &state))
                .unwrap();
            let row: String = (0..width)
                .map(|x| cf.buffer.cell((x, 0)).unwrap().symbol().to_string())
                .collect();
            let idx = row.find("tokens").unwrap_or_else(|| {
                panic!("expected `tokens` label in rendered bar at width={width}: {row:?}")
            });
            assert!(
                idx + "tokens".len() <= width as usize,
                "right info block must end within bar width={width} (row={row:?})"
            );
        }
    }
}
