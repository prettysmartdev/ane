use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::data::lsp::types::{InstallLine, Language, ServerState};
use crate::data::state::EditorState;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_statuses: &[(Language, ServerState)],
) {
    let install_line = state.lsp_state.lock().unwrap().install_line.clone();
    render_inner(frame, area, state, lsp_statuses, install_line.as_ref());
}

fn render_inner(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_statuses: &[(Language, ServerState)],
    install_line: Option<&InstallLine>,
) {
    let width = area.width as usize;

    let mode_str = match state.mode {
        crate::data::state::Mode::Edit => "EDIT",
        crate::data::state::Mode::Chord => "CHORD",
    };
    let mode_text = format!(" {mode_str} ");
    let mode_span = Span::styled(
        mode_text.clone(),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let lsp_spans = build_lsp_indicator_spans(lsp_statuses);
    let lsp_char_len: usize = lsp_spans.iter().map(|s| s.content.chars().count()).sum();

    let disk_hint: Option<String> = state.current_buffer().and_then(|buf| {
        if buf.disk_changed {
            let fname = buf
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            Some(if buf.dirty {
                format!(
                    " {} changed on disk. Ctrl-O to open and discard changes, Ctrl-S to overwrite with current changes ",
                    fname
                )
            } else {
                format!(" {} changed on disk. Ctrl-O to open newer version ", fname)
            })
        } else {
            None
        }
    });

    let hint_text_owned: String;
    let hint_text: &str = if let Some(ref dh) = disk_hint {
        dh.as_str()
    } else if state.selection.is_some() {
        " Ctrl-Y: copy "
    } else {
        hint_text_owned = String::new();
        &hint_text_owned
    };
    let hint_char_len = hint_text.chars().count();

    let fixed_len = mode_text.len() + lsp_char_len + hint_char_len;
    let available = width.saturating_sub(fixed_len);

    let (msg_text, msg_color) = if let Some(il) = install_line {
        match il {
            InstallLine::Stdout(s) => (s.clone(), Color::White),
            InstallLine::Stderr(s) => (s.clone(), Color::Red),
            InstallLine::Failed(s) => (s.clone(), Color::Red),
        }
    } else if !state.status_msg.is_empty() {
        (state.status_msg.clone(), Color::Yellow)
    } else {
        (String::new(), Color::Reset)
    };

    let (msg_span, pad_span) = if msg_text.is_empty() {
        (Span::raw(""), Span::raw(" ".repeat(available)))
    } else {
        let raw_msg = format!(" {} ", msg_text);
        let msg_chars: Vec<char> = raw_msg.chars().collect();
        let truncated = if msg_chars.len() > available {
            let end = available.saturating_sub(1);
            let mut s: String = msg_chars[..end].iter().collect();
            s.push('\u{2026}');
            s
        } else {
            raw_msg
        };
        let pad = available.saturating_sub(truncated.chars().count());
        (
            Span::styled(truncated, Style::default().fg(msg_color)),
            Span::raw(" ".repeat(pad)),
        )
    };

    let hint_span = if hint_text.is_empty() {
        Span::raw("")
    } else {
        Span::styled(
            hint_text.to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mut spans = vec![mode_span, msg_span, pad_span, hint_span];
    spans.extend(lsp_spans);
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn build_lsp_indicator_spans(statuses: &[(Language, ServerState)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut first = true;

    for (lang, state) in statuses {
        if matches!(state, ServerState::Undetected | ServerState::Stopped) {
            continue;
        }
        let (indicator, color) = match state {
            ServerState::Running => ("\u{25cf}", Color::Green),
            ServerState::Installing | ServerState::Starting | ServerState::Available => {
                ("\u{25cc}", Color::Yellow)
            }
            ServerState::Failed | ServerState::Missing => ("\u{2716}", Color::Red),
            _ => continue,
        };

        if first {
            spans.push(Span::raw(" "));
            first = false;
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("{}:{}", lang.short_name(), indicator),
            Style::default().fg(color),
        ));
    }

    if !spans.is_empty() {
        spans.push(Span::raw(" "));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_state(disk_changed: bool, dirty: bool) -> (NamedTempFile, EditorState) {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello\n").unwrap();
        f.flush().unwrap();
        let mut state = EditorState::for_file(f.path()).unwrap();
        if let Some(buf) = state.current_buffer_mut() {
            buf.disk_changed = disk_changed;
            buf.dirty = dirty;
        }
        (f, state)
    }

    fn render_to_string(state: &EditorState) -> String {
        let backend = TestBackend::new(200, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_inner(frame, frame.area(), state, &[], None);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn clean_buffer_shows_open_newer_version_hint() {
        let (_f, state) = make_state(true, false);
        let rendered = render_to_string(&state);
        assert!(
            rendered.contains("Ctrl-O to open newer version"),
            "hint should say 'open newer version' for clean buffer; got: {rendered:?}"
        );
        assert!(
            !rendered.contains("discard changes"),
            "hint should not mention 'discard changes' for clean buffer; got: {rendered:?}"
        );
    }

    #[test]
    fn dirty_buffer_shows_discard_and_overwrite_hints() {
        let (_f, state) = make_state(true, true);
        let rendered = render_to_string(&state);
        assert!(
            rendered.contains("Ctrl-O to open and discard changes"),
            "hint should say 'open and discard changes' for dirty buffer; got: {rendered:?}"
        );
        assert!(
            rendered.contains("Ctrl-S to overwrite"),
            "hint should mention 'Ctrl-S to overwrite' for dirty buffer; got: {rendered:?}"
        );
    }

    #[test]
    fn disk_change_hint_has_priority_over_selection_hint() {
        let (_f, mut state) = make_state(true, false);
        state.selection = Some(crate::data::state::Selection {
            anchor_line: 0,
            anchor_col: 0,
            head_line: 0,
            head_col: 3,
        });
        let rendered = render_to_string(&state);
        assert!(
            rendered.contains("Ctrl-O to open newer version"),
            "disk-change hint should take priority over selection hint; got: {rendered:?}"
        );
        assert!(
            !rendered.contains("Ctrl-Y: copy"),
            "selection hint should not appear when disk_changed is set; got: {rendered:?}"
        );
    }
}
