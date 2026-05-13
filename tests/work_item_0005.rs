use std::collections::HashMap;
use std::io::Write;

use ane::commands::chord_engine::ChordEngine;
use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::data::buffer::Buffer;
use ane::data::state::EditorState;
use ane::frontend::traits::ApplyChordAction;
use ane::frontend::tui::tui_frontend::TuiFrontend;

fn temp_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

// --- TUI cursor update from Jump chord ---

#[test]
fn tui_cursor_update_from_jump_delimiter_contents() {
    // "fn f(abc)" — '(' at col 4, ')' at col 8, cursor at (0,6) inside
    // jedc = Jump Entire Delimiter Contents → no LSP needed
    // Jump places cursor at start of target range (Contents starts at col 5)
    let content = "fn f(abc)";
    let f = temp_file(content);

    let mut state = EditorState::for_file(f.path()).unwrap();
    let query =
        ChordEngine::try_auto_submit_short("jedc", 0, 6).expect("jedc is a valid 4-char chord");
    // cursor_pos already set to (0,6) by try_auto_submit_short

    let abs_path = std::fs::canonicalize(f.path()).unwrap();
    let path_str = abs_path.to_string_lossy().to_string();
    let buf = Buffer::from_file(&abs_path).unwrap();
    let mut buffers = HashMap::new();
    buffers.insert(path_str.clone(), buf);

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    let resolved = ChordEngine::resolve(&query, &buffers, &mut lsp).unwrap();
    let actions = ChordEngine::patch(&resolved, &buffers).unwrap();

    let action = actions.get(&path_str).unwrap();
    assert!(action.diff.is_none(), "Jump must produce no diff");

    let mut frontend = TuiFrontend::new();
    frontend.apply(&mut state, action).unwrap();

    // Jump to Contents: target range starts at start_col+1=5 (after the '(')
    assert_eq!(state.cursor_line, 0, "cursor line should be 0");
    assert_eq!(
        state.cursor_col, 5,
        "cursor col should point to start of contents"
    );
}

#[test]
fn tui_cursor_update_from_jump_delimiter_self_goes_to_open_delimiter() {
    // "foo(bar)" — '(' at col 3, cursor at (0,4) inside
    // jeds = Jump Entire Delimiter Self — target is full delimiter range starting at '('
    let content = "foo(bar)";
    let f = temp_file(content);

    let mut state = EditorState::for_file(f.path()).unwrap();
    let query =
        ChordEngine::try_auto_submit_short("jeds", 0, 4).expect("jeds is a valid 4-char chord");

    let abs_path = std::fs::canonicalize(f.path()).unwrap();
    let path_str = abs_path.to_string_lossy().to_string();
    let buf = Buffer::from_file(&abs_path).unwrap();
    let mut buffers = HashMap::new();
    buffers.insert(path_str.clone(), buf);

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    let resolved = ChordEngine::resolve(&query, &buffers, &mut lsp).unwrap();
    let actions = ChordEngine::patch(&resolved, &buffers).unwrap();

    let action = actions.get(&path_str).unwrap();
    assert!(action.diff.is_none(), "Jump must produce no diff");

    let mut frontend = TuiFrontend::new();
    frontend.apply(&mut state, action).unwrap();

    // Jump to Self: target range starts at '(' which is col 3
    assert_eq!(state.cursor_line, 0);
    assert_eq!(state.cursor_col, 3);
}
