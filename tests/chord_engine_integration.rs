use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use ane::commands::chord_engine::types::{ChordAction, DiffLine};
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
    }
}

fn single_buffer(path: &str, content: &str) -> HashMap<String, Buffer> {
    let mut map = HashMap::new();
    map.insert(path.to_string(), make_buffer(path, content));
    map
}

fn default_lsp() -> LspEngine {
    LspEngine::new(LspEngineConfig::default())
}

fn run(chord: &str, path: &str, content: &str) -> ChordAction {
    let buffers = single_buffer(path, content);
    let mut lsp = default_lsp();
    let mut actions = ChordEngine::execute(chord, &buffers, &mut lsp).unwrap();
    actions.remove(path).unwrap()
}

// --- Full pipeline: chord string → ChordAction ---

#[test]
fn full_pipeline_change_entire_line_with_value() {
    let action = run(
        r#"cels(line:1, value:"replacement line")"#,
        "/buf",
        "first line\nsecond line\nthird line",
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("replacement line"),
        "got: {}",
        diff.modified
    );
    assert!(!diff.modified.contains("second line"));
    assert!(diff.modified.contains("first line"));
    assert!(diff.modified.contains("third line"));
}

#[test]
fn full_pipeline_delete_entire_line() {
    let action = run("dels(line:1)", "/buf", "aaa\nbbb\nccc");
    let diff = action.diff.as_ref().unwrap();
    assert!(!diff.modified.contains("bbb"));
    assert!(diff.modified.contains("aaa"));
    assert!(diff.modified.contains("ccc"));
}

#[test]
fn full_pipeline_yank_line_captures_content() {
    let action = run("yels(line:0)", "/buf", "yanked content\nother line");
    assert!(action.diff.is_none());
    assert_eq!(action.yanked_content.as_deref(), Some("yanked content"));
}

#[test]
fn full_pipeline_append_after_line_end() {
    let action = run(
        r#"aale(line:0, value:" appended")"#,
        "/buf",
        "hello world\nline two",
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("hello world appended"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn full_pipeline_prepend_before_line_beginning() {
    let action = run(
        r#"pels(line:0, value:">>> ")"#,
        "/buf",
        "original text\nline two",
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains(">>> original text"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn full_pipeline_change_entire_buffer_self() {
    let action = run(
        r#"cebs(value:"brand new content")"#,
        "/buf",
        "old line 1\nold line 2",
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(diff.modified.contains("brand new content"));
    assert!(!diff.modified.contains("old line"));
}

// --- Round-trip: apply diff modified back to original ---

#[test]
fn round_trip_change_line_matches_expected() {
    let original = "line one\nline two\nline three";
    let action = run(r#"cels(line:1, value:"line REPLACED")"#, "/buf", original);
    let diff = action.diff.as_ref().unwrap();
    let result = &diff.modified;

    assert!(result.contains("line one"));
    assert!(result.contains("line REPLACED"));
    assert!(result.contains("line three"));
    assert!(!result.contains("line two"));
}

#[test]
fn round_trip_delete_clears_line_content() {
    // dels clears the line's content (sets it to "") rather than removing the line entirely
    let original = "a\nb\nc\nd\ne";
    let action = run("dels(line:2)", "/buf", original);
    let diff = action.diff.as_ref().unwrap();
    let result_lines: Vec<&str> = diff.modified.lines().collect();
    assert!(
        !diff.modified.contains('c'),
        "deleted content should be gone"
    );
    assert_eq!(result_lines[2], "", "deleted line should be empty");
    assert_eq!(result_lines[0], "a");
    assert_eq!(result_lines[4], "e");
}

#[test]
fn round_trip_append_increases_line_length() {
    let original = "hello";
    let action = run(r#"aale(line:0, value:" world")"#, "/buf", original);
    let diff = action.diff.as_ref().unwrap();
    assert_eq!(diff.modified.trim(), "hello world");
}

// --- Real Rust source file content ---

const RUST_SOURCE: &str = r#"struct Point {
    x: f64,
    y: f64,
}

fn distance(a: &Point, b: &Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn main() {
    let p1 = Point { x: 0.0, y: 0.0 };
    let p2 = Point { x: 3.0, y: 4.0 };
    println!("{}", distance(&p1, &p2));
}
"#;

#[test]
fn real_rust_source_delete_line() {
    let action = run("dels(line:7)", "/src/main.rs", RUST_SOURCE);
    let diff = action.diff.as_ref().unwrap();
    assert!(
        !diff.modified.contains("let dy ="),
        "got: {}",
        diff.modified
    );
    assert!(diff.modified.contains("let dx ="), "got: {}", diff.modified);
}

#[test]
fn real_rust_source_change_line_with_value() {
    let action = run(
        r#"cels(line:12, value:"    let p1 = Point { x: 1.0, y: 2.0 };")"#,
        "/src/main.rs",
        RUST_SOURCE,
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("x: 1.0, y: 2.0"),
        "got: {}",
        diff.modified
    );
    assert!(
        !diff.modified.contains("x: 0.0, y: 0.0"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn real_rust_source_yank_function_line() {
    let action = run("yels(line:5)", "/src/main.rs", RUST_SOURCE);
    assert!(action.diff.is_none());
    let yanked = action.yanked_content.as_ref().unwrap();
    assert!(yanked.contains("fn distance"), "yanked: {yanked}");
}

#[test]
fn real_rust_source_diff_hunks_nonempty_on_change() {
    let action = run(
        r#"cels(line:0, value:"struct Vector {")"#,
        "/src/main.rs",
        RUST_SOURCE,
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(!diff.hunks.is_empty());

    let all_lines: Vec<&DiffLine> = diff.hunks.iter().flat_map(|h| h.lines.iter()).collect();
    let has_removed = all_lines.iter().any(|l| matches!(l, DiffLine::Removed(_)));
    let has_added = all_lines.iter().any(|l| matches!(l, DiffLine::Added(_)));
    assert!(has_removed);
    assert!(has_added);
}

// --- Multi-buffer: chord applies to each independently ---

#[test]
fn multi_buffer_same_chord_applied_to_each() {
    let path_a = "/test/a.rs";
    let path_b = "/test/b.rs";
    let content_a = "fn alpha() {}\nfn beta() {}";
    let content_b = "mod foo;\nmod bar;";

    let mut buffers = HashMap::new();
    buffers.insert(path_a.to_string(), make_buffer(path_a, content_a));
    buffers.insert(path_b.to_string(), make_buffer(path_b, content_b));

    let mut lsp = default_lsp();
    let mut actions = ChordEngine::execute("dels(line:0)", &buffers, &mut lsp).unwrap();

    let action_a = actions.remove(path_a).unwrap();
    let action_b = actions.remove(path_b).unwrap();

    let diff_a = action_a.diff.as_ref().unwrap();
    let diff_b = action_b.diff.as_ref().unwrap();

    assert!(
        !diff_a.modified.contains("fn alpha"),
        "a: {}",
        diff_a.modified
    );
    assert!(
        diff_a.modified.contains("fn beta"),
        "a: {}",
        diff_a.modified
    );

    assert!(
        !diff_b.modified.contains("mod foo"),
        "b: {}",
        diff_b.modified
    );
    assert!(
        diff_b.modified.contains("mod bar"),
        "b: {}",
        diff_b.modified
    );
}

// --- CLI: execute_chord applies diff to disk ---

#[test]
fn cli_execute_chord_modifies_file_on_disk() {
    use ane::commands::chord::execute_chord;

    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    write!(f, "line one\nline two\nline three").unwrap();
    f.flush().unwrap();

    let mut chord = ane::commands::chord::parse_chord("cels").unwrap();
    chord.args.target_line = Some(1);
    chord.args.value = Some("CHANGED".to_string());

    let result = execute_chord(f.path(), &chord).unwrap();
    assert!(result.modified.contains("CHANGED"));
    assert!(!result.modified.contains("line two"));

    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(on_disk.contains("CHANGED"));
    assert!(!on_disk.contains("line two"));
}

#[test]
fn cli_execute_chord_yank_does_not_modify_file() {
    use ane::commands::chord::execute_chord;

    let content = "original content\nsecond line";
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    write!(f, "{content}").unwrap();
    f.flush().unwrap();

    let mut chord = ane::commands::chord::parse_chord("yels").unwrap();
    chord.args.target_line = Some(0);

    let result = execute_chord(f.path(), &chord).unwrap();
    assert_eq!(result.original, result.modified);

    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert_eq!(on_disk, content);
}

// --- Function-scoped chords with mock LSP ---

fn run_with_lsp(
    chord: &str,
    path: &str,
    content: &str,
    symbols: Vec<DocumentSymbol>,
) -> ChordAction {
    let buffers = single_buffer(path, content);
    let mut lsp = LspEngine::new(LspEngineConfig::default());
    lsp.inject_test_symbols(PathBuf::from(path), symbols);
    let mut actions = ChordEngine::execute(chord, &buffers, &mut lsp).unwrap();
    actions.remove(path).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn fn_sym(
    name: &str,
    sl: usize,
    sc: usize,
    el: usize,
    ec: usize,
    sel_sl: usize,
    sel_sc: usize,
    sel_el: usize,
    sel_ec: usize,
) -> DocumentSymbol {
    DocumentSymbol {
        name: name.to_string(),
        kind: SymbolKind::Function,
        range: SymbolRange {
            start_line: sl,
            start_col: sc,
            end_line: el,
            end_col: ec,
        },
        selection_range: Some(SymbolRange {
            start_line: sel_sl,
            start_col: sel_sc,
            end_line: sel_el,
            end_col: sel_ec,
        }),
        children: vec![],
    }
}

#[test]
fn cifn_renames_function() {
    let action = run_with_lsp(
        r#"cifn(function:foo, value:"bar")"#,
        "/test.rs",
        "fn foo() { 42 }",
        vec![fn_sym("foo", 0, 0, 0, 15, 0, 3, 0, 6)],
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("fn bar() { 42 }"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn cifc_replaces_function_contents() {
    let action = run_with_lsp(
        r#"cifc(function:foo, value:" 99 ")"#,
        "/test.rs",
        "fn foo() { 42 }",
        vec![fn_sym("foo", 0, 0, 0, 15, 0, 3, 0, 6)],
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("fn foo() { 99 }"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn cifc_long_form_replaces_function_contents() {
    let action = run_with_lsp(
        r#"ChangeInsideFunctionContents(function:foo, value:" 99 ")"#,
        "/test.rs",
        "fn foo() { 42 }",
        vec![fn_sym("foo", 0, 0, 0, 15, 0, 3, 0, 6)],
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("fn foo() { 99 }"),
        "got: {}",
        diff.modified
    );
}

#[test]
fn cbfs_replaces_text_before_function() {
    let action = run_with_lsp(
        r#"cbfs(function:foo, value:"// header ")"#,
        "/test.rs",
        "use std::io;\n\nfn foo() { 42 }",
        vec![fn_sym("foo", 2, 0, 2, 15, 2, 3, 2, 6)],
    );
    let diff = action.diff.as_ref().unwrap();
    assert!(
        diff.modified.contains("// header "),
        "got: {}",
        diff.modified
    );
    assert!(
        !diff.modified.contains("use std::io;"),
        "got: {}",
        diff.modified
    );
    assert!(
        diff.modified.contains("fn foo() { 42 }"),
        "got: {}",
        diff.modified
    );
}
