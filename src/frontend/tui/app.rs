use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
};

use crate::commands::chord_engine::{ChordEngine, parens_balanced};
use crate::commands::lsp_engine::{InstallProgress, LspEngine, LspEngineConfig};
use crate::commands::syntax_engine::{SyntaxEngine, SyntaxFrontend};
use crate::data::lsp::types::{InstallLine, Language, LspSharedState, SemanticToken, ServerState};
use crate::data::state::{EditorState, Mode};

use super::chord_box;
use super::editor_pane;
use super::exit_modal;
use super::status_bar;
use super::title_bar;
use super::tree_pane;

fn prev_char_boundary(s: &str, from: usize) -> usize {
    let mut idx = from.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    if idx > 0 {
        idx -= 1;
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
    }
    idx
}

fn next_char_boundary(s: &str, from: usize) -> usize {
    let mut idx = from.min(s.len());
    if idx < s.len() {
        idx += 1;
        while idx < s.len() && !s.is_char_boundary(idx) {
            idx += 1;
        }
    }
    idx
}

fn snap_to_char_boundary(s: &str, pos: usize) -> usize {
    let mut idx = pos.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn compute_text_width(state: &EditorState, term_width: u16) -> usize {
    let total = state.current_buffer().map_or(1, |b| b.line_count());
    let line_num_width = format!("{}", total.saturating_sub(1)).len();
    let has_tree = state.file_tree.is_some() && state.focus_tree;
    let editor_width = if has_tree {
        (term_width as usize) / 2
    } else {
        term_width as usize
    };
    editor_width.saturating_sub(line_num_width + 1)
}

fn move_cursor_left(state: &mut EditorState) {
    let at_start = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line))
        .is_some_and(|line| state.cursor_col.min(line.len()) == 0);
    if at_start && state.cursor_line > 0 {
        state.cursor_line -= 1;
        let end = state
            .current_buffer()
            .and_then(|b| b.lines.get(state.cursor_line))
            .map_or(0, |l| l.len());
        state.cursor_col = end;
    } else if let Some(line) = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line))
    {
        state.cursor_col = prev_char_boundary(line, state.cursor_col.min(line.len()));
    }
}

fn move_cursor_right(state: &mut EditorState) {
    let (at_end, line_count) = {
        let at_end = state
            .current_buffer()
            .and_then(|b| b.lines.get(state.cursor_line))
            .is_none_or(|line| snap_to_char_boundary(line, state.cursor_col) >= line.len());
        let count = state.current_buffer().map_or(0, |b| b.line_count());
        (at_end, count)
    };
    if at_end && state.cursor_line + 1 < line_count {
        state.cursor_line += 1;
        state.cursor_col = 0;
    } else if let Some(line) = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line))
    {
        state.cursor_col = next_char_boundary(line, snap_to_char_boundary(line, state.cursor_col));
    }
}

fn move_cursor_up(state: &mut EditorState, text_width: usize) {
    let cur_info = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line))
        .map(|line| {
            let dc = editor_pane::display_col(line, state.cursor_col);
            let offsets = editor_pane::wrap_offsets(line, text_width);
            let (row, col_in_row) = editor_pane::display_col_to_wrap_pos(&offsets, dc);
            (row, col_in_row, offsets)
        });
    if let Some((visual_row, col_in_row, offsets)) = cur_info {
        if visual_row > 0 {
            let target = editor_pane::wrap_row_start(&offsets, visual_row - 1) + col_in_row;
            if let Some(line) = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
            {
                state.cursor_col = editor_pane::byte_col_from_display(line, target);
            }
        } else if state.cursor_line > 0 {
            state.cursor_line -= 1;
            if let Some(prev_line) = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
            {
                if text_width > 0 {
                    let prev_offsets = editor_pane::wrap_offsets(prev_line, text_width);
                    let last_row = prev_offsets.len() - 1;
                    let target = editor_pane::wrap_row_start(&prev_offsets, last_row) + col_in_row;
                    state.cursor_col = editor_pane::byte_col_from_display(prev_line, target);
                } else {
                    state.cursor_col = state.cursor_col.min(prev_line.len());
                }
            }
        }
    }
}

fn move_cursor_down(state: &mut EditorState, text_width: usize) {
    let cur_info = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line))
        .map(|line| {
            let dc = editor_pane::display_col(line, state.cursor_col);
            let offsets = editor_pane::wrap_offsets(line, text_width);
            let (visual_row, col_in_row) = editor_pane::display_col_to_wrap_pos(&offsets, dc);
            let total_rows = offsets.len();
            (visual_row, col_in_row, total_rows, offsets)
        });
    let has_next = state
        .current_buffer()
        .and_then(|b| b.lines.get(state.cursor_line + 1))
        .is_some();
    if let Some((visual_row, col_in_row, total_rows, offsets)) = cur_info {
        if total_rows > 1 && visual_row < total_rows - 1 {
            let target = editor_pane::wrap_row_start(&offsets, visual_row + 1) + col_in_row;
            if let Some(line) = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
            {
                state.cursor_col = editor_pane::byte_col_from_display(line, target);
            }
        } else if has_next {
            state.cursor_line += 1;
            if let Some(next_line) = state
                .current_buffer()
                .and_then(|b| b.lines.get(state.cursor_line))
            {
                if text_width > 0 {
                    state.cursor_col = editor_pane::byte_col_from_display(next_line, col_in_row);
                } else {
                    state.cursor_col = state.cursor_col.min(next_line.len());
                }
            }
        }
    }
}

use super::tui_frontend::TuiFrontend;

use crate::frontend::traits::ApplyChordAction;

struct TuiInstallProgress {
    shared: Arc<Mutex<LspSharedState>>,
}

impl InstallProgress for TuiInstallProgress {
    fn on_stdout(&self, line: &str) {
        self.shared.lock().unwrap().install_line = Some(InstallLine::Stdout(line.to_string()));
    }
    fn on_stderr(&self, line: &str) {
        self.shared.lock().unwrap().install_line = Some(InstallLine::Stderr(line.to_string()));
    }
    fn on_failed(&self, message: &str) {
        self.shared.lock().unwrap().install_line = Some(InstallLine::Failed(message.to_string()));
    }
    fn on_complete(&self) {
        self.shared.lock().unwrap().install_line = None;
    }
}

/// TUI implementation of SyntaxFrontend — stores tokens per-path for the render loop.
struct TuiSyntaxReceiver {
    tokens: Arc<Mutex<HashMap<PathBuf, Vec<SemanticToken>>>>,
}

impl TuiSyntaxReceiver {
    fn new() -> Self {
        Self {
            tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn tokens_for(&self, path: &Path) -> Vec<SemanticToken> {
        self.tokens
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default()
    }
}

impl SyntaxFrontend for TuiSyntaxReceiver {
    fn set_semantic_tokens(&self, path: &Path, tokens: Vec<SemanticToken>) {
        self.tokens
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), tokens);
    }
}

pub fn run(path: &Path) -> Result<()> {
    let mut state = if path.is_dir() {
        EditorState::for_directory(path)?
    } else {
        EditorState::for_file(path)?
    };

    let engine = Arc::new(Mutex::new(LspEngine::new(LspEngineConfig::default())));
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let files: Vec<&Path> = if path.is_file() { vec![path] } else { vec![] };
    {
        let mut eng = engine.lock().unwrap();
        let progress: Arc<dyn InstallProgress> = Arc::new(TuiInstallProgress {
            shared: Arc::clone(&state.lsp_state),
        });
        eng.set_install_progress(progress);
        let _ = eng.start_for_context(&root, &files);
    }

    // Create the syntax receiver (Layer 2's impl of SyntaxFrontend)
    let syntax_receiver = Arc::new(TuiSyntaxReceiver::new());

    // Create SyntaxEngine (Layer 1) — it owns the LspEngine reference
    // and spawns its own background worker for debounced LSP requests
    let mut syntax_engine = SyntaxEngine::new(
        Arc::clone(&engine),
        Arc::clone(&syntax_receiver) as Arc<dyn SyntaxFrontend>,
    );

    // Initial compute for the opened file
    if let Some(buf) = state.current_buffer() {
        syntax_engine.compute(&buf.path, &buf.content());
    }

    // Status polling thread — polls all server statuses
    {
        let poll_engine = Arc::clone(&engine);
        let poll_shared = Arc::clone(&state.lsp_state);
        std::thread::spawn(move || {
            status_polling_task(poll_engine, poll_shared);
        });
    }

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut frontend = TuiFrontend::new();
    let result = event_loop(
        &mut terminal,
        &mut state,
        &mut frontend,
        &engine,
        &mut syntax_engine,
        &syntax_receiver,
    );

    {
        let mut eng = engine.lock().unwrap();
        eng.shutdown_all();
    }
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn status_polling_task(engine: Arc<Mutex<LspEngine>>, shared: Arc<Mutex<LspSharedState>>) {
    loop {
        let summary = {
            let eng = engine.lock().unwrap();
            eng.status_summary()
        };

        let all_terminal = {
            let mut s = shared.lock().unwrap();
            s.status.clear();
            let mut all_terminal = !summary.is_empty();
            for (lang, state) in &summary {
                s.status.insert(*lang, *state);
                if !state.is_terminal() {
                    all_terminal = false;
                }
            }
            all_terminal
        };

        if all_terminal && !summary.is_empty() {
            break;
        }

        let any_running = summary.iter().any(|(_, s)| *s == ServerState::Running);
        let interval = if any_running {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(1)
        };
        std::thread::sleep(interval);
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    syntax_engine: &mut SyntaxEngine,
    syntax_receiver: &Arc<TuiSyntaxReceiver>,
) -> Result<()> {
    let mut prev_lsp_statuses: Vec<(Language, ServerState)> = Vec::new();

    loop {
        let lsp_statuses: Vec<(Language, ServerState)> = {
            let s = state.lsp_state.lock().unwrap();
            s.status.iter().map(|(l, s)| (*l, *s)).collect()
        };

        // Detect if any LSP server just became Running
        let lsp_became_ready = lsp_statuses.iter().any(|(lang, st)| {
            *st == ServerState::Running
                && !prev_lsp_statuses
                    .iter()
                    .any(|(l, s)| l == lang && *s == ServerState::Running)
        });
        if lsp_became_ready && let Some(buf) = state.current_buffer() {
            syntax_engine.compute(&buf.path, &buf.content());
        }
        prev_lsp_statuses = lsp_statuses.clone();

        // Read tokens from the syntax receiver
        let tokens = if let Some(buf) = state.current_buffer() {
            syntax_receiver.tokens_for(&buf.path)
        } else {
            vec![]
        };

        let term_size = terminal.size()?;
        adjust_scroll_offset(state, term_size.height, term_size.width);

        terminal.draw(|frame| {
            draw(frame, state, &tokens, &lsp_statuses);
        })?;

        if state.should_quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            let buffer_modified = handle_key(
                state,
                frontend,
                engine,
                key.code,
                key.modifiers,
                syntax_engine,
                &lsp_statuses,
                term_size.width,
            );
            if buffer_modified && let Some(buf) = state.current_buffer() {
                syntax_engine.compute(&buf.path, &buf.content());
            }
        }
    }
}

fn adjust_scroll_offset(state: &mut EditorState, term_height: u16, term_width: u16) {
    let total = match state.current_buffer() {
        Some(buf) => buf.line_count(),
        None => return,
    };
    let chord_box_rows: usize = if state.mode == Mode::Chord && !state.focus_tree {
        4
    } else {
        0
    };
    let visible = (term_height as usize).saturating_sub(2 + chord_box_rows);
    if visible == 0 {
        state.scroll_offset = 0;
        return;
    }

    let line_num_width = format!("{}", total.saturating_sub(1)).len();
    let has_tree = state.file_tree.is_some() && state.focus_tree;
    let editor_width = if has_tree {
        (term_width as usize) / 2
    } else {
        term_width as usize
    };
    let text_width = editor_width.saturating_sub(line_num_width + 1);

    if state.cursor_line < state.scroll_offset {
        state.scroll_offset = state.cursor_line;
    }

    let active = state.active_buffer;
    loop {
        let mut visual_rows = 0;
        let cursor = state.cursor_line.min(total.saturating_sub(1));
        for i in state.scroll_offset..=cursor {
            let line = state.buffers[active]
                .lines
                .get(i)
                .map(|s| s.as_str())
                .unwrap_or("");
            if i == cursor {
                let cursor_display_col = editor_pane::display_col(line, state.cursor_col);
                let offsets = editor_pane::wrap_offsets(line, text_width);
                let (cursor_row_in_line, _) =
                    editor_pane::display_col_to_wrap_pos(&offsets, cursor_display_col);
                visual_rows += cursor_row_in_line + 1;
            } else {
                visual_rows += editor_pane::visual_row_count(line, text_width);
            }
        }

        if visual_rows <= visible || state.scroll_offset >= state.cursor_line {
            break;
        }

        state.scroll_offset += 1;
    }

    if state.scroll_offset >= total {
        state.scroll_offset = total.saturating_sub(1);
    }
}

fn draw(
    frame: &mut ratatui::Frame,
    state: &EditorState,
    tokens: &[SemanticToken],
    lsp_statuses: &[(Language, ServerState)],
) {
    let has_tree = state.file_tree.is_some() && state.focus_tree;

    let h_constraints = if has_tree {
        let content_w = tree_pane::content_width(&state.tree_view) as u16;
        let desired = content_w + 3;
        let max_w = frame.area().width / 2;
        let tree_w = desired.min(max_w);
        vec![Constraint::Length(tree_w), Constraint::Min(0)]
    } else {
        vec![Constraint::Length(0), Constraint::Percentage(100)]
    };

    let h_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(h_constraints)
        .split(frame.area());

    let tree_area = h_layout[0];
    let editor_area = h_layout[1];

    if has_tree {
        tree_pane::render(frame, tree_area, state);
    }

    let v_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(editor_area);

    let title_area = v_layout[0];
    let pane_area = v_layout[1];
    let status_area = v_layout[2];

    title_bar::render(frame, title_area, state);

    let chord_box_visible = state.mode == Mode::Chord && !state.focus_tree;
    let editor_render_area = if chord_box_visible {
        Rect::new(
            pane_area.x,
            pane_area.y,
            pane_area.width,
            pane_area.height.saturating_sub(4),
        )
    } else {
        pane_area
    };
    editor_pane::render(frame, editor_render_area, state, tokens);
    status_bar::render(frame, status_area, state, lsp_statuses);

    if chord_box_visible {
        chord_box::render(frame, pane_area, state);
    }

    if state.pending_open_path.is_some() {
        exit_modal::render_open_modal(frame);
    } else if state.show_exit_modal {
        exit_modal::render(frame, state);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    code: KeyCode,
    modifiers: KeyModifiers,
    syntax_engine: &mut SyntaxEngine,
    lsp_statuses: &[(Language, ServerState)],
    term_width: u16,
) -> bool {
    // Priority 1: Exit modal
    if state.show_exit_modal {
        handle_exit_modal(state, code, modifiers);
        return false;
    }

    // Priority 2: Open-confirm modal
    if state.pending_open_path.is_some() {
        handle_open_modal(state, code, modifiers, syntax_engine);
        return false;
    }

    // Priority 3: Ctrl-T always handled
    if code == KeyCode::Char('t') && modifiers.contains(KeyModifiers::CONTROL) {
        toggle_tree(state);
        return false;
    }

    // Priority 3b: Ctrl-S always handled
    if code == KeyCode::Char('s') && modifiers.contains(KeyModifiers::CONTROL) {
        if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
            let _ = buf.write();
        }
        state.status_msg = "saved".into();
        return false;
    }

    // Priority 4: Tree focus
    if state.focus_tree {
        handle_tree_keys(state, code, modifiers, syntax_engine);
        return false;
    }

    // Priority 5: Chord mode
    if state.mode == Mode::Chord {
        return handle_chord_mode(
            state,
            frontend,
            engine,
            code,
            modifiers,
            syntax_engine,
            lsp_statuses,
            term_width,
        );
    }

    // Priority 6: Edit mode
    handle_edit_mode(state, code, modifiers, term_width)
}

fn handle_exit_modal(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.should_quit = true;
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                let _ = buf.write();
            }
            state.should_quit = true;
        }
        KeyCode::Esc => {
            state.show_exit_modal = false;
        }
        _ => {}
    }
}

fn handle_open_modal(
    state: &mut EditorState,
    code: KeyCode,
    modifiers: KeyModifiers,
    syntax_engine: &mut SyntaxEngine,
) {
    match code {
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                let _ = buf.write();
            }
            if let Some(path) = state.pending_open_path.take() {
                let _ = state.open_file(&path);
                if let Some(buf) = state.current_buffer() {
                    syntax_engine.compute(&buf.path, &buf.content());
                }
            }
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(path) = state.pending_open_path.take() {
                if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                    buf.dirty = false;
                }
                let _ = state.open_file(&path);
                if let Some(buf) = state.current_buffer() {
                    syntax_engine.compute(&buf.path, &buf.content());
                }
            }
        }
        KeyCode::Esc => {
            state.pending_open_path = None;
        }
        _ => {}
    }
}

fn handle_tree_keys(
    state: &mut EditorState,
    code: KeyCode,
    modifiers: KeyModifiers,
    syntax_engine: &mut SyntaxEngine,
) {
    match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.focus_tree = false;
            state.mode = Mode::Edit;
            state.status_msg = "-- EDIT --".into();
        }
        KeyCode::Up => {
            state.tree_selected = state.tree_selected.saturating_sub(1);
        }
        KeyCode::Down if !state.tree_view.is_empty() => {
            let max = state.tree_view.len().saturating_sub(1);
            if state.tree_selected < max {
                state.tree_selected += 1;
            }
        }
        KeyCode::Right => {
            if let Some(entry) = state
                .tree_view
                .get(state.tree_selected)
                .filter(|e| e.is_dir)
            {
                let _ = entry;
                tree_pane::expand(state, state.tree_selected);
            }
        }
        KeyCode::Left => {
            let selected = state.tree_selected;
            let is_expanded_dir = state.tree_view.get(selected).is_some_and(|e| {
                e.is_dir
                    && state
                        .tree_view
                        .get(selected + 1)
                        .is_some_and(|next| next.depth > e.depth)
            });
            if is_expanded_dir {
                tree_pane::collapse(state, selected);
            } else if let Some(depth) = state.tree_view.get(selected).map(|e| e.depth)
                && depth > 0
            {
                let parent = (0..selected)
                    .rev()
                    .find(|&j| state.tree_view[j].is_dir && state.tree_view[j].depth < depth);
                if let Some(idx) = parent {
                    state.tree_selected = idx;
                    tree_pane::collapse(state, idx);
                }
            }
        }
        KeyCode::Enter => {
            let selected = state.tree_selected;
            if let Some(entry) = state.tree_view.get(selected).filter(|e| !e.is_dir) {
                let path = entry.path.clone();
                let is_dirty = state.current_buffer().is_some_and(|b| b.dirty);
                if is_dirty {
                    state.pending_open_path = Some(path);
                } else {
                    let _ = state.open_file(&path);
                    if let Some(buf) = state.current_buffer() {
                        syntax_engine.compute(&buf.path, &buf.content());
                    }
                }
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_chord_mode(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    code: KeyCode,
    modifiers: KeyModifiers,
    syntax_engine: &mut SyntaxEngine,
    lsp_statuses: &[(Language, ServerState)],
    term_width: u16,
) -> bool {
    match code {
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Edit;
            state.status_msg = "-- EDIT --".into();
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Char('r')
            if modifiers.contains(KeyModifiers::CONTROL) && !state.chord_history.is_empty() =>
        {
            let idx = match state.chord_history_index {
                Some(i) if i > 0 => i - 1,
                Some(_) => 0,
                None => state.chord_history.len() - 1,
            };
            state.chord_history_index = Some(idx);
            state.chord_input = state.chord_history[idx].clone();
            state.chord_cursor_col = state.chord_input.len();
            state.chord_error = false;
        }
        KeyCode::Up if state.chord_input.is_empty() => {
            let tw = compute_text_width(state, term_width);
            move_cursor_up(state, tw);
        }
        KeyCode::Down if state.chord_input.is_empty() => {
            let tw = compute_text_width(state, term_width);
            move_cursor_down(state, tw);
        }
        KeyCode::Left if state.chord_input.is_empty() => {
            move_cursor_left(state);
        }
        KeyCode::Right if state.chord_input.is_empty() => {
            move_cursor_right(state);
        }
        KeyCode::Left => {
            state.chord_cursor_col = state.chord_cursor_col.saturating_sub(1);
        }
        KeyCode::Right if state.chord_cursor_col < state.chord_input.len() => {
            state.chord_cursor_col += 1;
        }
        KeyCode::Enter if !state.chord_input.is_empty() => {
            let input = state.chord_input.clone();
            match ChordEngine::parse(&input) {
                Ok(_) => {
                    clear_chord(state);
                    execute_chord_input(state, frontend, engine, &input, lsp_statuses);
                    if !state.status_msg.starts_with("error:")
                        && !state.status_msg.starts_with("resolve error:")
                        && !state.status_msg.starts_with("patch error:")
                        && !state.status_msg.starts_with("parse error:")
                    {
                        state.chord_history.push(input.clone());
                    }
                    if let Some(buf) = state.current_buffer() {
                        syntax_engine.compute(&buf.path, &buf.content());
                    }
                }
                Err(_) => {
                    state.chord_error = true;
                    state.status_msg = "invalid chord".into();
                }
            }
        }
        KeyCode::Backspace if state.chord_cursor_col > 0 && !state.chord_input.is_empty() => {
            let col = state.chord_cursor_col.min(state.chord_input.len());
            state.chord_input.remove(col - 1);
            state.chord_cursor_col = col - 1;
            state.chord_error = false;
            state.chord_history_index = None;
            try_auto_submit(state, frontend, engine, syntax_engine, lsp_statuses);
        }
        KeyCode::Esc => {
            clear_chord(state);
            state.status_msg.clear();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let col = state.chord_cursor_col.min(state.chord_input.len());
            state.chord_input.insert(col, c);
            state.chord_cursor_col = col + 1;
            state.chord_error = false;
            state.chord_history_index = None;
            try_auto_submit(state, frontend, engine, syntax_engine, lsp_statuses);
        }
        _ => {}
    }
    false
}

fn try_auto_submit(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    syntax_engine: &mut SyntaxEngine,
    lsp_statuses: &[(Language, ServerState)],
) {
    let input = &state.chord_input;

    if input.len() == 4 && input.chars().next().is_some_and(|c| c.is_lowercase()) {
        if let Some(_query) =
            ChordEngine::try_auto_submit_short(input, state.cursor_line, state.cursor_col)
        {
            let input_clone = state.chord_input.clone();
            state.chord_running = true;
            clear_chord(state);
            execute_chord_input(state, frontend, engine, &input_clone, lsp_statuses);
            if !state.status_msg.starts_with("error:")
                && !state.status_msg.starts_with("resolve error:")
                && !state.status_msg.starts_with("patch error:")
                && !state.status_msg.starts_with("parse error:")
            {
                state.chord_history.push(input_clone);
            }
            state.chord_running = false;
            if let Some(buf) = state.current_buffer() {
                syntax_engine.compute(&buf.path, &buf.content());
            }
        }
    } else if input.ends_with(')')
        && input.chars().next().is_some_and(|c| c.is_uppercase())
        && parens_balanced(input)
        && ChordEngine::parse(input).is_ok()
    {
        let input_clone = state.chord_input.clone();
        state.chord_running = true;
        clear_chord(state);
        execute_chord_input(state, frontend, engine, &input_clone, lsp_statuses);
        if !state.status_msg.starts_with("error:")
            && !state.status_msg.starts_with("resolve error:")
            && !state.status_msg.starts_with("patch error:")
            && !state.status_msg.starts_with("parse error:")
        {
            state.chord_history.push(input_clone);
        }
        state.chord_running = false;
        if let Some(buf) = state.current_buffer() {
            syntax_engine.compute(&buf.path, &buf.content());
        }
    }
}

fn clear_chord(state: &mut EditorState) {
    state.chord_input.clear();
    state.chord_cursor_col = 0;
    state.chord_error = false;
    state.chord_running = false;
    state.chord_history_index = None;
}

fn handle_edit_mode(
    state: &mut EditorState,
    code: KeyCode,
    modifiers: KeyModifiers,
    term_width: u16,
) -> bool {
    let mut modified = false;
    match code {
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Chord;
            state.status_msg.clear();
            clear_chord(state);
        }
        KeyCode::Esc => {
            state.mode = Mode::Chord;
            state.status_msg.clear();
            clear_chord(state);
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Tab => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer)
                && line < buf.lines.len()
            {
                let col = snap_to_char_boundary(&buf.lines[line], col);
                buf.lines[line].insert(col, '\t');
                buf.dirty = true;
                state.cursor_col = col + 1;
                modified = true;
            }
        }
        KeyCode::Up => {
            let tw = compute_text_width(state, term_width);
            move_cursor_up(state, tw);
        }
        KeyCode::Down => {
            let tw = compute_text_width(state, term_width);
            move_cursor_down(state, tw);
        }
        KeyCode::Left => {
            move_cursor_left(state);
        }
        KeyCode::Right => {
            move_cursor_right(state);
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer)
                && line < buf.lines.len()
            {
                let col = snap_to_char_boundary(&buf.lines[line], col);
                buf.lines[line].insert(col, c);
                buf.dirty = true;
                state.cursor_col = col + c.len_utf8();
                modified = true;
            }
        }
        KeyCode::Backspace => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                let snapped = if line < buf.lines.len() {
                    snap_to_char_boundary(&buf.lines[line], col)
                } else {
                    0
                };
                if snapped > 0 && line < buf.lines.len() {
                    let prev = prev_char_boundary(&buf.lines[line], snapped);
                    buf.lines[line].drain(prev..snapped);
                    buf.dirty = true;
                    state.cursor_col = prev;
                    modified = true;
                } else if snapped == 0 && line > 0 {
                    let current_line = buf.lines.remove(line);
                    buf.dirty = true;
                    let prev_line = line - 1;
                    let prev_len = buf.lines[prev_line].len();
                    buf.lines[prev_line].push_str(&current_line);
                    state.cursor_line = prev_line;
                    state.cursor_col = prev_len;
                    modified = true;
                }
            }
        }
        KeyCode::Enter => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer)
                && line < buf.lines.len()
            {
                let current = buf.lines[line].clone();
                let col = snap_to_char_boundary(&current, col);
                let remainder = current[col..].to_string();
                buf.lines[line] = current[..col].to_string();
                buf.insert_line(line + 1, remainder);
                state.cursor_line += 1;
                state.cursor_col = 0;
                modified = true;
            }
        }
        _ => {}
    }
    modified
}

fn toggle_tree(state: &mut EditorState) {
    if state.file_tree.is_some() {
        if state.focus_tree {
            state.focus_tree = false;
            state.mode = state.pre_tree_mode;
        } else {
            state.pre_tree_mode = state.mode;
            state.focus_tree = true;
            if let Some(buf) = state.current_buffer() {
                let buf_path = buf.path.clone();
                if let Some(idx) = state.tree_view.iter().position(|e| e.path == buf_path) {
                    state.tree_selected = idx;
                }
            }
        }
    } else {
        let dir = if state.opened_path.is_dir() {
            state.opened_path.clone()
        } else {
            state
                .opened_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf()
        };
        match crate::data::file_tree::FileTree::from_dir(&dir) {
            Ok(tree) => {
                let tree_view: Vec<_> = tree
                    .entries
                    .iter()
                    .filter(|e| e.depth == 0)
                    .cloned()
                    .collect();
                state.file_tree = Some(tree);
                state.tree_view = tree_view;
                state.pre_tree_mode = state.mode;
                state.focus_tree = true;
                if let Some(buf) = state.current_buffer() {
                    let buf_path = buf.path.clone();
                    if let Some(idx) = state.tree_view.iter().position(|e| e.path == buf_path) {
                        state.tree_selected = idx;
                    }
                }
                state.status_msg = "file tree opened".into();
            }
            Err(e) => {
                state.status_msg = format!("tree error: {e}");
            }
        }
    }
}

fn execute_chord_input(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    input: &str,
    lsp_statuses: &[(Language, ServerState)],
) {
    // Determine the effective LSP status for the current file's language
    let lsp_status = current_file_lsp_status(state, lsp_statuses);

    match ChordEngine::parse(input) {
        Ok(mut query) => {
            if query.requires_lsp && !lsp_status.is_available() {
                if lsp_status.is_pending() {
                    state.status_msg = format!(
                        "chord {} waiting for LSP ({})",
                        query.short_form(),
                        lsp_status.display()
                    );
                } else {
                    state.status_msg = format!(
                        "chord {} requires LSP but {}",
                        query.short_form(),
                        lsp_status.display()
                    );
                }
                return;
            }

            query.args.cursor_pos = Some((state.cursor_line, state.cursor_col));

            let mut buffers = HashMap::new();
            if let Some(buf) = state.current_buffer() {
                let path_str = buf.path.to_string_lossy().to_string();
                buffers.insert(path_str, buf.clone());
            }

            let resolve_result = {
                let mut eng = engine.lock().unwrap();
                ChordEngine::resolve(&query, &buffers, &mut eng)
            };

            match resolve_result {
                Ok(resolved) => match ChordEngine::patch(&resolved, &buffers) {
                    Ok(actions) => {
                        for action in actions.values() {
                            match frontend.apply(state, action) {
                                Ok(msg) => {
                                    if !msg.is_empty() && state.status_msg.is_empty() {
                                        state.status_msg = msg;
                                    }
                                }
                                Err(e) => {
                                    state.status_msg = format!("error: {e}");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        state.status_msg = format!("patch error: {e}");
                    }
                },
                Err(e) => {
                    state.status_msg = format!("resolve error: {e}");
                }
            }
        }
        Err(e) => {
            state.status_msg = format!("parse error: {e}");
        }
    }
}

fn current_file_lsp_status(
    state: &EditorState,
    lsp_statuses: &[(Language, ServerState)],
) -> ServerState {
    if let Some(buf) = state.current_buffer()
        && let Some(lang) = Language::from_path(&buf.path)
    {
        for (l, s) in lsp_statuses {
            if *l == lang {
                return *s;
            }
        }
    }
    ServerState::Undetected
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
    use crate::commands::syntax_engine::SyntaxEngine;
    use crate::data::lsp::types::SemanticToken;
    use crate::data::state::EditorState;

    fn tok(line: usize, start_col: usize, length: usize, token_type: &str) -> SemanticToken {
        SemanticToken {
            line,
            start_col,
            length,
            token_type: token_type.to_string(),
        }
    }

    #[test]
    fn tui_syntax_receiver_set_and_get_tokens() {
        let receiver = TuiSyntaxReceiver::new();
        let path = std::path::Path::new("/tmp/test.rs");
        let tokens = vec![tok(0, 0, 2, "keyword"), tok(0, 3, 4, "function")];
        receiver.set_semantic_tokens(path, tokens);
        let got = receiver.tokens_for(path);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].token_type, "keyword");
        assert_eq!(got[1].token_type, "function");
    }

    #[test]
    fn tui_syntax_receiver_per_path_isolation() {
        let receiver = TuiSyntaxReceiver::new();
        let path_a = std::path::Path::new("/tmp/a.rs");
        let path_b = std::path::Path::new("/tmp/b.ts");

        receiver.set_semantic_tokens(path_a, vec![tok(0, 0, 3, "keyword")]);
        receiver.set_semantic_tokens(path_b, vec![tok(1, 5, 4, "string")]);

        let got_a = receiver.tokens_for(path_a);
        let got_b = receiver.tokens_for(path_b);

        assert_eq!(got_a.len(), 1);
        assert_eq!(got_a[0].token_type, "keyword");
        assert_eq!(got_b.len(), 1);
        assert_eq!(got_b[0].token_type, "string");
        // cross-check isolation
        assert!(
            receiver
                .tokens_for(std::path::Path::new("/tmp/c.go"))
                .is_empty()
        );
    }

    #[test]
    fn lsp_readiness_triggers_merged_token_delivery() {
        // Simulate: first compute delivers ts-only; after LSP "becomes ready"
        // (worker caches tokens), second compute delivers merged ts+LSP tokens.
        let receiver = Arc::new(TuiSyntaxReceiver::new());
        let path = PathBuf::from("lsp_ready_integration.rs");

        let mut lsp = LspEngine::new(LspEngineConfig::default());
        // Inject a fake LSP token that partially overlaps with the ts "fn" keyword
        lsp.inject_test_semantic_tokens(path.clone(), vec![tok(0, 0, 2, "keyword")]);
        let engine = Arc::new(Mutex::new(lsp));

        let mut syntax = SyntaxEngine::new(
            Arc::clone(&engine),
            Arc::clone(&receiver) as Arc<dyn SyntaxFrontend>,
        );

        // First compute: LSP cache is empty — delivers tree-sitter tokens only
        syntax.compute(path.as_path(), "fn main() {}");
        let first = receiver.tokens_for(path.as_path());
        assert!(
            !first.is_empty(),
            "tree-sitter tokens should be delivered on first compute"
        );

        // Wait for the background worker to run, call semantic_tokens (returns
        // injected tokens), and populate lsp_cache
        std::thread::sleep(std::time::Duration::from_millis(600));

        // Second compute: LSP cache is now populated — merged delivery fires synchronously
        syntax.compute(path.as_path(), "fn main() {}");

        let second = receiver.tokens_for(path.as_path());
        assert!(
            second
                .iter()
                .any(|t| t.line == 0 && t.start_col == 0 && t.length == 2),
            "merged delivery should include LSP token at (0,0,2); got: {:?}",
            second
        );
    }

    fn make_state_with_lines(lines: &[&str]) -> (tempfile::NamedTempFile, EditorState) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                f.write_all(b"\n").unwrap();
            }
            f.write_all(line.as_bytes()).unwrap();
        }
        f.flush().unwrap();
        let state = EditorState::for_file(f.path()).unwrap();
        (f, state)
    }

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn adjust_scroll_offset_scrolls_down_when_cursor_below_viewport() {
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 0;
        state.cursor_line = 15;
        adjust_scroll_offset(&mut state, 20, 80);
        assert_eq!(state.scroll_offset, 2);
    }

    #[test]
    fn adjust_scroll_offset_scrolls_up_when_cursor_above_scroll() {
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 10;
        state.cursor_line = 3;
        adjust_scroll_offset(&mut state, 20, 80);
        assert_eq!(state.scroll_offset, 3);
    }

    #[test]
    fn adjust_scroll_offset_no_change_when_cursor_in_viewport() {
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 0;
        state.cursor_line = 5;
        adjust_scroll_offset(&mut state, 20, 80);
        assert_eq!(state.scroll_offset, 0);
    }
}
