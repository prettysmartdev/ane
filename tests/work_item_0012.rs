use std::collections::HashMap;
use std::path::PathBuf;

use ane::commands::chord_engine::ChordEngine;
use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::data::buffer::Buffer;
use ane::data::lsp::types::{DocumentSymbol, SymbolKind, SymbolRange};

fn make_buffer(path: &str, content: &str) -> Buffer {
    Buffer {
        path: PathBuf::from(path),
        lines: content.lines().map(String::from).collect(),
        dirty: false,
        trailing_newline: content.ends_with('\n'),
        last_disk_mtime: None,
        disk_changed: false,
        disk_deleted: false,
    }
}

fn single_buffer(path: &str, content: &str) -> HashMap<String, Buffer> {
    let mut map = HashMap::new();
    map.insert(path.to_string(), make_buffer(path, content));
    map
}

fn fn_sym(name: &str, line: usize) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::Function,
        range: SymbolRange {
            start_line: line,
            start_col: 0,
            end_line: line,
            end_col: 9,
        },
        selection_range: None,
        children: vec![],
    }
}

// --- Full pipeline: c3ls changes exactly 3 lines ---

#[test]
fn full_pipeline_c3ls_changes_exactly_3_lines() {
    let path = "/buf";
    let content = "line0\nline1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9";
    let buffers = single_buffer(path, content);
    let mut lsp = LspEngine::new(LspEngineConfig::default());

    let mut actions = ChordEngine::execute(
        r#"c3ls(cursor:"2,0", value:"REPLACED")"#,
        &buffers,
        &mut lsp,
    )
    .unwrap();
    let action = actions.remove(path).unwrap();

    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("line0"),
        "line0 should remain: {}",
        diff.modified
    );
    assert!(
        diff.modified.contains("line1"),
        "line1 should remain: {}",
        diff.modified
    );
    assert!(
        !diff.modified.contains("line2"),
        "line2 should be replaced: {}",
        diff.modified
    );
    assert!(
        !diff.modified.contains("line3"),
        "line3 should be replaced: {}",
        diff.modified
    );
    assert!(
        !diff.modified.contains("line4"),
        "line4 should be replaced: {}",
        diff.modified
    );
    assert!(
        diff.modified.contains("REPLACED"),
        "replacement text should appear: {}",
        diff.modified
    );
    assert!(
        diff.modified.contains("line5"),
        "line5 should remain: {}",
        diff.modified
    );
    assert!(
        diff.modified.contains("line9"),
        "line9 should remain: {}",
        diff.modified
    );
}

// --- Full pipeline: j5lw resolves the 5-word span ---

#[test]
fn full_pipeline_j5lw_resolves_to_correct_word_span() {
    // "one two three four five six" — 6 words; cursor at col 0 (within "one")
    // next words after current: two(4-7) three(8-13) four(14-18) five(19-23) six(24-27)
    // 5th next word ends at col 27, so highlight_ranges[0].end_col == 27
    let path = "/buf";
    let content = "one two three four five six";
    let buffers = single_buffer(path, content);
    let mut lsp = LspEngine::new(LspEngineConfig::default());

    let mut actions = ChordEngine::execute(r#"j5lw(cursor:"0,0")"#, &buffers, &mut lsp).unwrap();
    let action = actions.remove(path).unwrap();

    assert!(action.diff.is_none(), "Jump must produce no diff");
    assert!(
        !action.highlight_ranges.is_empty(),
        "Jump should produce highlight ranges"
    );
    assert_eq!(
        action.highlight_ranges[0].end_col, 27,
        "highlight should end at the 5th word boundary (end of 'six')"
    );
}

// --- CLI exec: l5fd returns at most 5 results ---

#[test]
fn full_pipeline_l5fd_returns_at_most_count_results() {
    // 7 single-line functions at lines 0-6; cursor at (0,0) excludes fn at line 0, col 0.
    // 6 functions remain after filter; Count(5) takes the first 5.
    let path = "/test/file.rs";
    let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\nfn g() {}";
    let buffers = single_buffer(path, content);

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    lsp.inject_test_symbols(
        PathBuf::from(path),
        vec![
            fn_sym("a", 0),
            fn_sym("b", 1),
            fn_sym("c", 2),
            fn_sym("d", 3),
            fn_sym("e", 4),
            fn_sym("f", 5),
            fn_sym("g", 6),
        ],
    );

    let mut actions = ChordEngine::execute(r#"l5fd(cursor:"0,0")"#, &buffers, &mut lsp).unwrap();
    let action = actions.remove(path).unwrap();

    assert!(action.diff.is_none(), "List must produce no diff");
    assert_eq!(
        action.listed_items.len(),
        5,
        "should return exactly 5 results (count capped at 5)"
    );
    for item in &action.listed_items {
        assert!(item.line > 0, "all results should be after the cursor line");
    }
}

#[test]
fn full_pipeline_l5fd_returns_all_when_fewer_than_count_exist() {
    // 4 single-line functions at lines 0-3; cursor at (0,0) excludes line 0, col 0.
    // 3 functions remain after filter; Count(5) takes all 3.
    let path = "/test/file.rs";
    let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}";
    let buffers = single_buffer(path, content);

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    lsp.inject_test_symbols(
        PathBuf::from(path),
        vec![
            fn_sym("a", 0),
            fn_sym("b", 1),
            fn_sym("c", 2),
            fn_sym("d", 3),
        ],
    );

    let mut actions = ChordEngine::execute(r#"l5fd(cursor:"0,0")"#, &buffers, &mut lsp).unwrap();
    let action = actions.remove(path).unwrap();

    assert!(action.diff.is_none(), "List must produce no diff");
    assert_eq!(
        action.listed_items.len(),
        3,
        "should return all 3 available results when fewer than 5 exist"
    );
}
