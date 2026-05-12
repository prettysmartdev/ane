use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::data::lsp::types::ServerState;
use crate::data::state::EditorState;

pub fn render(frame: &mut Frame, area: Rect, state: &EditorState, lsp_status: ServerState) {
    let lsp_color = match lsp_status {
        ServerState::Running => Color::Green,
        ServerState::Failed | ServerState::Missing => Color::Red,
        ServerState::Installing | ServerState::Starting | ServerState::Available => Color::Yellow,
        ServerState::Undetected | ServerState::Stopped => Color::DarkGray,
    };

    let lsp_span = Span::styled(
        format!(" {} ", lsp_status.display()),
        Style::default().fg(lsp_color),
    );

    let mode_str = match state.mode {
        crate::data::state::Mode::Edit => "EDIT",
        crate::data::state::Mode::Chord => "CHORD",
    };
    let mode_span = Span::styled(
        format!(" {mode_str} "),
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let file_span = match state.current_buffer() {
        Some(buf) => {
            let name = buf
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("untitled");
            let dirty = if buf.dirty { " [+]" } else { "" };
            Span::styled(
                format!(" {name}{dirty} "),
                Style::default().fg(Color::White),
            )
        }
        None => Span::styled(" no file ", Style::default().fg(Color::DarkGray)),
    };

    let pos_span = Span::styled(
        format!(" {}:{} ", state.cursor_line + 1, state.cursor_col + 1),
        Style::default().fg(Color::DarkGray),
    );

    let msg_span = if state.status_msg.is_empty() {
        Span::raw("")
    } else {
        Span::styled(
            format!(" {} ", state.status_msg),
            Style::default().fg(Color::Yellow),
        )
    };

    let line = Line::from(vec![mode_span, file_span, pos_span, msg_span, lsp_span]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
