use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    Terminal,
};

use crate::commands::chord_engine::{parens_balanced, ChordEngine};
use crate::commands::lsp_engine::{InstallProgress, LspEngine, LspEngineConfig};
use crate::data::lsp::registry;
use crate::data::lsp::types::{InstallLine, Language, LspSharedState, SemanticToken, ServerState};
use crate::data::state::{EditorState, Mode};

use super::chord_box;
use super::editor_pane;
use super::exit_modal;
use super::status_bar;
use super::title_bar;
use super::tree_pane;
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

    let detected_lang = primary_language(path);
    if let Some(lang) = detected_lang {
        let eng = engine.lock().unwrap();
        let server_state = eng.server_state(lang);
        state.lsp_state.lock().unwrap().status = server_state;
    }

    let lsp_shared = Arc::clone(&state.lsp_state);
    let (token_tx, token_rx) = std::sync::mpsc::sync_channel::<(std::path::PathBuf, String)>(1);

    if let Some(lang) = detected_lang {
        let poll_engine = Arc::clone(&engine);
        let poll_shared = Arc::clone(&lsp_shared);
        std::thread::spawn(move || {
            status_polling_task(poll_engine, poll_shared, lang);
        });

        let tok_engine = Arc::clone(&engine);
        let tok_shared = Arc::clone(&lsp_shared);
        std::thread::spawn(move || {
            token_request_task(tok_engine, tok_shared, token_rx);
        });
    }

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut frontend = TuiFrontend::new();
    let result = event_loop(&mut terminal, &mut state, &mut frontend, &engine, &token_tx);

    {
        let mut eng = engine.lock().unwrap();
        eng.shutdown_all();
    }
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn status_polling_task(
    engine: Arc<Mutex<LspEngine>>,
    shared: Arc<Mutex<LspSharedState>>,
    lang: Language,
) {
    loop {
        let current = {
            let eng = engine.lock().unwrap();
            eng.server_state(lang)
        };

        {
            let mut s = shared.lock().unwrap();
            s.status = current;
        }

        if matches!(current, ServerState::Stopped | ServerState::Failed) {
            break;
        }

        let interval = if current == ServerState::Running {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(1)
        };
        std::thread::sleep(interval);
    }
}

fn token_request_task(
    engine: Arc<Mutex<LspEngine>>,
    shared: Arc<Mutex<LspSharedState>>,
    rx: std::sync::mpsc::Receiver<(std::path::PathBuf, String)>,
) {
    while let Ok((path, content)) = rx.recv() {
        let tokens = {
            let mut eng = engine.lock().unwrap();
            eng.semantic_tokens(&path, &content).unwrap_or_default()
        };
        let mut s = shared.lock().unwrap();
        s.semantic_tokens = tokens;
    }
}

fn primary_language(path: &Path) -> Option<Language> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    registry::detect_language_from_dir(&root).or_else(|| registry::detect_language_from_path(path))
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    token_tx: &std::sync::mpsc::SyncSender<(std::path::PathBuf, String)>,
) -> Result<()> {
    let mut last_edit: Option<Instant> = None;

    loop {
        let (lsp_status, semantic_tokens) = {
            let s = state.lsp_state.lock().unwrap();
            (s.status, s.semantic_tokens.clone())
        };

        adjust_scroll_offset(state, terminal.size()?.height);

        if lsp_status == ServerState::Running && semantic_tokens.is_empty() {
            if let Some(buf) = state.current_buffer() {
                let _ = token_tx.try_send((buf.path.clone(), buf.content()));
            }
        }

        if let Some(last) = last_edit {
            if last.elapsed() >= Duration::from_millis(300) {
                last_edit = None;
                if lsp_status == ServerState::Running {
                    if let Some(buf) = state.current_buffer() {
                        let _ = token_tx.try_send((buf.path.clone(), buf.content()));
                    }
                }
            }
        }

        terminal.draw(|frame| {
            draw(frame, state, &lsp_status, &semantic_tokens);
        })?;

        if state.should_quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let buffer_modified = handle_key(
                    state,
                    frontend,
                    engine,
                    key.code,
                    key.modifiers,
                    token_tx,
                    lsp_status,
                );
                if buffer_modified {
                    last_edit = Some(Instant::now());
                }
            }
        }
    }
}

fn adjust_scroll_offset(state: &mut EditorState, term_height: u16) {
    let Some(buf) = state.current_buffer() else {
        return;
    };
    let total = buf.line_count();
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

    if state.cursor_line < state.scroll_offset {
        state.scroll_offset = state.cursor_line;
    } else if state.cursor_line >= state.scroll_offset + visible {
        state.scroll_offset = state.cursor_line + 1 - visible;
    }
    let max = total.saturating_sub(visible);
    if state.scroll_offset > max {
        state.scroll_offset = max;
    }
}

fn draw(
    frame: &mut ratatui::Frame,
    state: &EditorState,
    lsp_status: &ServerState,
    semantic_tokens: &[SemanticToken],
) {
    let has_tree = state.file_tree.is_some() && state.focus_tree;

    let h_constraints = if has_tree {
        vec![Constraint::Percentage(25), Constraint::Percentage(75)]
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
    editor_pane::render(
        frame,
        editor_render_area,
        state,
        *lsp_status,
        semantic_tokens,
    );
    status_bar::render(frame, status_area, state, *lsp_status);

    if chord_box_visible {
        chord_box::render(frame, pane_area, state);
    }

    if state.pending_open_path.is_some() {
        exit_modal::render_open_modal(frame);
    } else if state.show_exit_modal {
        exit_modal::render(frame, state);
    }
}

fn handle_key(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    code: KeyCode,
    modifiers: KeyModifiers,
    token_tx: &std::sync::mpsc::SyncSender<(std::path::PathBuf, String)>,
    lsp_status: ServerState,
) -> bool {
    // Priority 1: Exit modal
    if state.show_exit_modal {
        handle_exit_modal(state, code, modifiers);
        return false;
    }

    // Priority 2: Open-confirm modal
    if state.pending_open_path.is_some() {
        handle_open_modal(state, code, modifiers, token_tx);
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
        handle_tree_keys(state, code, modifiers);
        return false;
    }

    // Priority 5: Chord mode
    if state.mode == Mode::Chord {
        return handle_chord_mode(
            state, frontend, engine, code, modifiers, token_tx, lsp_status,
        );
    }

    // Priority 6: Edit mode
    handle_edit_mode(state, code, modifiers)
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
    token_tx: &std::sync::mpsc::SyncSender<(std::path::PathBuf, String)>,
) {
    match code {
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                let _ = buf.write();
            }
            if let Some(path) = state.pending_open_path.take() {
                let _ = state.open_file(&path);
                state.lsp_state.lock().unwrap().semantic_tokens.clear();
                if let Some(buf) = state.current_buffer() {
                    let _ = token_tx.try_send((buf.path.clone(), buf.content()));
                }
            }
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(path) = state.pending_open_path.take() {
                if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                    buf.dirty = false;
                }
                let _ = state.open_file(&path);
                state.lsp_state.lock().unwrap().semantic_tokens.clear();
                if let Some(buf) = state.current_buffer() {
                    let _ = token_tx.try_send((buf.path.clone(), buf.content()));
                }
            }
        }
        KeyCode::Esc => {
            state.pending_open_path = None;
        }
        _ => {}
    }
}

fn handle_tree_keys(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) {
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
            if let Some(entry) = state
                .tree_view
                .get(state.tree_selected)
                .filter(|e| e.is_dir)
            {
                let _ = entry;
                tree_pane::collapse(state, state.tree_selected);
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
                    state.lsp_state.lock().unwrap().semantic_tokens.clear();
                }
            }
        }
        _ => {}
    }
}

fn handle_chord_mode(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    code: KeyCode,
    modifiers: KeyModifiers,
    token_tx: &std::sync::mpsc::SyncSender<(std::path::PathBuf, String)>,
    lsp_status: ServerState,
) -> bool {
    match code {
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Edit;
            state.status_msg = "-- EDIT --".into();
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Up if state.chord_input.is_empty() => {
            state.cursor_line = state.cursor_line.saturating_sub(1);
        }
        KeyCode::Down if state.chord_input.is_empty() => {
            if let Some(buf) = state.current_buffer() {
                if state.cursor_line + 1 < buf.line_count() {
                    state.cursor_line += 1;
                }
            }
        }
        KeyCode::Left if state.chord_input.is_empty() => {
            state.cursor_col = state.cursor_col.saturating_sub(1);
        }
        KeyCode::Right if state.chord_input.is_empty() => {
            if let Some(buf) = state.current_buffer() {
                let max = buf
                    .lines
                    .get(state.cursor_line)
                    .map(|l| l.len())
                    .unwrap_or(0);
                if state.cursor_col < max {
                    state.cursor_col += 1;
                }
            }
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
                    execute_chord_input(state, frontend, engine, &input, lsp_status);
                    if lsp_status == ServerState::Running {
                        if let Some(buf) = state.current_buffer() {
                            let _ = token_tx.try_send((buf.path.clone(), buf.content()));
                        }
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
            try_auto_submit(state, frontend, engine, token_tx, lsp_status);
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
            try_auto_submit(state, frontend, engine, token_tx, lsp_status);
        }
        _ => {}
    }
    false
}

fn try_auto_submit(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    engine: &Arc<Mutex<LspEngine>>,
    token_tx: &std::sync::mpsc::SyncSender<(std::path::PathBuf, String)>,
    lsp_status: ServerState,
) {
    let input = &state.chord_input;

    if input.len() == 4 && input.chars().next().is_some_and(|c| c.is_lowercase()) {
        if let Some(_query) =
            ChordEngine::try_auto_submit_short(input, state.cursor_line, state.cursor_col)
        {
            let input_clone = state.chord_input.clone();
            state.chord_running = true;
            clear_chord(state);
            execute_chord_input(state, frontend, engine, &input_clone, lsp_status);
            state.chord_running = false;
            if lsp_status == ServerState::Running {
                if let Some(buf) = state.current_buffer() {
                    let _ = token_tx.try_send((buf.path.clone(), buf.content()));
                }
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
        execute_chord_input(state, frontend, engine, &input_clone, lsp_status);
        state.chord_running = false;
        if lsp_status == ServerState::Running {
            if let Some(buf) = state.current_buffer() {
                let _ = token_tx.try_send((buf.path.clone(), buf.content()));
            }
        }
    }
}

fn clear_chord(state: &mut EditorState) {
    state.chord_input.clear();
    state.chord_cursor_col = 0;
    state.chord_error = false;
    state.chord_running = false;
}

fn handle_edit_mode(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) -> bool {
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
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                if line < buf.lines.len() {
                    let col = col.min(buf.lines[line].len());
                    buf.lines[line].insert(col, '\t');
                    buf.dirty = true;
                    state.cursor_col = col + 1;
                    modified = true;
                }
            }
        }
        KeyCode::Up => {
            state.cursor_line = state.cursor_line.saturating_sub(1);
        }
        KeyCode::Down => {
            if let Some(buf) = state.current_buffer() {
                if state.cursor_line + 1 < buf.line_count() {
                    state.cursor_line += 1;
                }
            }
        }
        KeyCode::Left => {
            state.cursor_col = state.cursor_col.saturating_sub(1);
        }
        KeyCode::Right => {
            if let Some(buf) = state.current_buffer() {
                let max = buf
                    .lines
                    .get(state.cursor_line)
                    .map(|l| l.len())
                    .unwrap_or(0);
                if state.cursor_col < max {
                    state.cursor_col += 1;
                }
            }
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                if line < buf.lines.len() {
                    let col = col.min(buf.lines[line].len());
                    buf.lines[line].insert(col, c);
                    buf.dirty = true;
                    state.cursor_col = col + 1;
                    modified = true;
                }
            }
        }
        KeyCode::Backspace => {
            let line = state.cursor_line;
            let col = state.cursor_col;
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                if col > 0 && line < buf.lines.len() {
                    buf.lines[line].remove(col - 1);
                    buf.dirty = true;
                    state.cursor_col -= 1;
                    modified = true;
                } else if col == 0 && line > 0 {
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
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                if line < buf.lines.len() {
                    let current = buf.lines[line].clone();
                    let col = col.min(current.len());
                    let remainder = current[col..].to_string();
                    buf.lines[line] = current[..col].to_string();
                    buf.insert_line(line + 1, remainder);
                    state.cursor_line += 1;
                    state.cursor_col = 0;
                    modified = true;
                }
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
    lsp_status: ServerState,
) {
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::data::state::EditorState;

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
        // Chord mode, no tree: chord_box_rows=4, visible=20-2-4=14
        // cursor_line=15 >= scroll_offset(0)+14 → scroll_offset = 15+1-14 = 2
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 0;
        state.cursor_line = 15;
        adjust_scroll_offset(&mut state, 20);
        assert_eq!(state.scroll_offset, 2);
    }

    #[test]
    fn adjust_scroll_offset_scrolls_up_when_cursor_above_scroll() {
        // cursor_line=3 < scroll_offset=10 → scroll_offset = 3
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 10;
        state.cursor_line = 3;
        adjust_scroll_offset(&mut state, 20);
        assert_eq!(state.scroll_offset, 3);
    }

    #[test]
    fn adjust_scroll_offset_no_change_when_cursor_in_viewport() {
        // cursor_line=5, scroll_offset=0, visible=14 → 5 in [0,14) → no change
        let lines: Vec<&str> = (0..30).map(|_| "line").collect();
        let (_f, mut state) = make_state_with_lines(&lines);
        state.scroll_offset = 0;
        state.cursor_line = 5;
        adjust_scroll_offset(&mut state, 20);
        assert_eq!(state.scroll_offset, 0);
    }
}
