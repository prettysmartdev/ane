use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::CrosstermBackend,
    Terminal,
};

use crate::commands::chord;
use crate::commands::lsp::client::LspClient;
use crate::data::lsp::registry;
use crate::data::lsp::types::{LspInitParams, LspStatus};
use crate::data::state::{EditorState, Mode};

use super::command_bar;
use super::editor_pane;
use super::exit_modal;
use super::status_bar;
use super::tree_pane;
use super::tui_frontend::TuiFrontend;

use crate::frontend::traits::ChordFrontend;

pub fn run(path: &Path) -> Result<()> {
    let mut state = if path.is_dir() {
        EditorState::for_directory(path)?
    } else {
        EditorState::for_file(path)?
    };

    let lsp_status = start_lsp_background(path);
    if let Some(ref status) = lsp_status {
        state.lsp_status = *status.lock().unwrap();
    }

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut frontend = TuiFrontend::new();
    let result = event_loop(&mut terminal, &mut state, &mut frontend, &lsp_status);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn start_lsp_background(path: &Path) -> Option<Arc<Mutex<LspStatus>>> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };

    let lang = registry::detect_language_from_dir(&root)
        .or_else(|| registry::detect_language_from_path(path))?;

    let status = Arc::new(Mutex::new(LspStatus::Unknown));
    let status_clone = Arc::clone(&status);

    thread::spawn(move || {
        let mut client = LspClient::new(lang);
        let params = LspInitParams {
            root_path: root,
            language: lang,
        };
        match client.start(&params) {
            Ok(()) => {
                *status_clone.lock().unwrap() = LspStatus::Ready;
                // Keep client alive — it holds the LSP process
                loop {
                    thread::sleep(std::time::Duration::from_secs(60));
                }
            }
            Err(_) => {
                let current = client.get_status();
                *status_clone.lock().unwrap() = if current == LspStatus::NotInstalled {
                    LspStatus::NotInstalled
                } else {
                    LspStatus::Failed
                };
            }
        }
    });

    Some(status)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    lsp_status: &Option<Arc<Mutex<LspStatus>>>,
) -> Result<()> {
    loop {
        if let Some(ref status) = lsp_status {
            state.lsp_status = *status.lock().unwrap();
        }

        terminal.draw(|frame| {
            let has_tree = state.file_tree.is_some();

            // Main layout: content area + command bar + status bar
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ])
                .split(frame.area());

            if has_tree {
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                    .split(outer[0]);
                tree_pane::render(frame, panes[0], state);
                editor_pane::render(frame, panes[1], state);
            } else {
                editor_pane::render(frame, outer[0], state);
            }

            command_bar::render(frame, outer[1], state);
            status_bar::render(frame, outer[2], state);

            if state.show_exit_modal {
                exit_modal::render(frame);
            }
        })?;

        if state.should_quit {
            return Ok(());
        }

        if let Event::Key(key) = event::read()? {
            if state.show_exit_modal {
                handle_exit_modal(state, key.code, key.modifiers);
            } else {
                match state.mode {
                    Mode::Chord => handle_chord_mode(state, frontend, key.code, key.modifiers),
                    Mode::Edit => handle_edit_mode(state, key.code, key.modifiers),
                }
            }
        }
    }
}

fn handle_exit_modal(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.should_quit = true;
        }
        KeyCode::Esc => {
            state.show_exit_modal = false;
        }
        _ => {}
    }
}

fn handle_chord_mode(
    state: &mut EditorState,
    frontend: &mut TuiFrontend,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Edit;
            state.status_msg = "-- EDIT --".into();
        }
        KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
            toggle_tree(state);
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Up => {
            if state.focus_tree {
                state.tree_selected = state.tree_selected.saturating_sub(1);
            } else {
                state.cursor_line = state.cursor_line.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if state.focus_tree {
                if let Some(tree) = &state.file_tree {
                    if state.tree_selected + 1 < tree.entries.len() {
                        state.tree_selected += 1;
                    }
                }
            } else if let Some(buf) = state.current_buffer() {
                if state.cursor_line + 1 < buf.line_count() {
                    state.cursor_line += 1;
                }
            }
        }
        KeyCode::Left if !state.focus_tree => {
            state.cursor_col = state.cursor_col.saturating_sub(1);
        }
        KeyCode::Right if !state.focus_tree => {
            state.cursor_col += 1;
        }
        KeyCode::Enter => {
            if state.focus_tree {
                if let Some(tree) = &state.file_tree {
                    if let Some(entry) = tree.entries.get(state.tree_selected) {
                        if !entry.is_dir {
                            let path = entry.path.clone();
                            let _ = state.open_file(&path);
                        }
                    }
                }
            } else if !state.chord_input.is_empty() {
                let input = state.chord_input.clone();
                state.chord_input.clear();
                execute_chord_input(state, frontend, &input);
            }
        }
        KeyCode::Backspace => {
            state.chord_input.pop();
        }
        KeyCode::Esc => {
            state.chord_input.clear();
            state.status_msg.clear();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) && !state.focus_tree => {
            state.chord_input.push(c);
        }
        _ => {}
    }
}

fn handle_edit_mode(state: &mut EditorState, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('e') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.mode = Mode::Chord;
            state.status_msg.clear();
            state.chord_input.clear();
        }
        KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
            toggle_tree(state);
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.show_exit_modal = true;
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(buf) = state.buffers.get_mut(state.active_buffer) {
                let _ = buf.write();
            }
            state.status_msg = "saved".into();
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
            state.cursor_col += 1;
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
                } else if col == 0 && line > 0 {
                    let current_line = buf.lines.remove(line);
                    buf.dirty = true;
                    let prev_line = line - 1;
                    let prev_len = buf.lines[prev_line].len();
                    buf.lines[prev_line].push_str(&current_line);
                    state.cursor_line = prev_line;
                    state.cursor_col = prev_len;
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
                }
            }
        }
        _ => {}
    }
}

fn toggle_tree(state: &mut EditorState) {
    if state.file_tree.is_some() {
        state.focus_tree = !state.focus_tree;
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
        if let Ok(tree) = crate::data::file_tree::FileTree::from_dir(&dir) {
            state.file_tree = Some(tree);
            state.focus_tree = true;
            state.status_msg = "file tree opened".into();
        }
    }
}

fn execute_chord_input(state: &mut EditorState, frontend: &mut TuiFrontend, input: &str) {
    match chord::parse_chord(input) {
        Ok(parsed) => {
            if parsed.spec.requires_lsp && !state.lsp_status.is_available() {
                if state.lsp_status.is_pending() {
                    state.status_msg = format!(
                        "chord {} waiting for LSP ({})",
                        parsed.spec.short_form(),
                        state.lsp_status.display()
                    );
                } else {
                    state.status_msg = format!(
                        "chord {} requires LSP but {}",
                        parsed.spec.short_form(),
                        state.lsp_status.display()
                    );
                }
                return;
            }
            match frontend.dispatch(state, &parsed) {
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
        Err(e) => {
            state.status_msg = format!("parse error: {e}");
        }
    }
}
