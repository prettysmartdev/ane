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

    let fixed_len = mode_text.len() + lsp_char_len;
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

    let mut spans = vec![mode_span, msg_span, pad_span];
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
