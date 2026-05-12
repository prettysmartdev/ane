use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::data::lsp::types::{InstallLine, ServerState};
use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState, lsp_status: ServerState) {
    let install_line = state.lsp_state.lock().unwrap().install_line.clone();
    render_inner(frame, area, state, lsp_status, install_line.as_ref());
}

fn render_inner(
    frame: &mut Frame,
    area: Rect,
    state: &EditorState,
    lsp_status: ServerState,
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

    let lsp_color = match lsp_status {
        ServerState::Running => Color::Green,
        ServerState::Failed | ServerState::Missing => Color::Red,
        ServerState::Installing | ServerState::Starting | ServerState::Available => Color::Yellow,
        ServerState::Undetected | ServerState::Stopped => Color::DarkGray,
    };
    let lsp_text = format!(" {} ", lsp_status.display());
    let lsp_span = Span::styled(lsp_text.clone(), Style::default().fg(lsp_color));

    let fixed_len = mode_text.len() + lsp_text.len();
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

    let line = Line::from(vec![mode_span, msg_span, pad_span, lsp_span]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
