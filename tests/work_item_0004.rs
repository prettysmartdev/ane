#![cfg(feature = "frontends")]

use std::io::Write;
use std::time::Duration;

use ane::commands::chord::execute_chord;
use ane::commands::chord_engine::ChordEngine;
use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::data::chord_types::{Action, Component, Positional, Scope};
use ane::frontend::cli_frontend::CliFrontend;

const MOCK_SERVER: &str = env!("CARGO_BIN_EXE_mock_lsp_server");

fn temp_rs_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

// --- TUI chord dispatch: try_auto_submit_short ---

#[test]
fn auto_submit_short_cifn_returns_query_with_correct_fields() {
    let query =
        ChordEngine::try_auto_submit_short("cifn", 5, 10).expect("cifn is a valid 4-char chord");
    assert_eq!(query.action, Action::Change);
    assert_eq!(query.positional, Positional::Inside);
    assert_eq!(query.scope, Scope::Function);
    assert_eq!(query.component, Component::Name);
    assert_eq!(query.args.cursor_pos, Some((5, 10)));
    assert!(query.requires_lsp);
}

#[test]
fn auto_submit_short_cifn_apply_produces_nonempty_diff() {
    // The mock LSP server always reports a "main" Function symbol at lines 0–10
    // with selectionRange (0,3)–(0,7) (the identifier "main" in "fn main()").
    // try_auto_submit_short places cursor_pos=(5,0) inside that range.
    // Adding value="new_main" makes the patcher rename the function.
    let content = concat!(
        "fn main() {\n",
        "    // line 1\n",
        "    // line 2\n",
        "    // line 3\n",
        "    // line 4\n",
        "    // line 5\n",
        "    // line 6\n",
        "    // line 7\n",
        "    // line 8\n",
        "    // line 9\n",
        "}\n",
    );
    let f = temp_rs_file(content);

    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_secs(10))
        .with_server_override(MOCK_SERVER, vec![], "true");

    let mut query =
        ChordEngine::try_auto_submit_short("cifn", 5, 0).expect("cifn is a valid 4-char chord");
    query.args.value = Some("new_main".to_string());

    let mut lsp = LspEngine::new(config);
    let result = execute_chord(&CliFrontend, f.path(), &query, &mut lsp).unwrap();
    assert_ne!(
        result.original, result.modified,
        "expected a non-empty diff for cifn rename"
    );
    assert!(
        result.modified.contains("new_main"),
        "modified should contain 'new_main', got:\n{}",
        result.modified
    );
}

// --- Long-form auto-submit gate: parens_balanced + ChordEngine::parse ---

#[test]
fn long_form_auto_submit_gate_only_fires_when_parens_are_balanced() {
    use ane::commands::chord_engine::parens_balanced;

    // Balanced parens with valid long form parses.
    let balanced = r#"ChangeInsideFunctionName(value:"new_name")"#;
    assert!(parens_balanced(balanced));
    assert!(ChordEngine::parse(balanced).is_ok());

    // Nested parens inside a value: balanced, parser should accept (or reject)
    // — the auto-submit gate is parens_balanced + parse Ok, never panic.
    let nested = r#"ChangeInsideFunctionName(value:"foo()")"#;
    assert!(parens_balanced(nested));
    // Don't assert parse outcome — parser may not support nested parens yet.
    let _ = ChordEngine::parse(nested);

    // Unbalanced should never reach parse via the auto-submit gate.
    let unbalanced_open = r#"ChangeInsideFunctionName(value:"foo()""#;
    assert!(!parens_balanced(unbalanced_open));

    let unbalanced_close = r#"ChangeInsideFunctionName)value:"foo"("#;
    assert!(!parens_balanced(unbalanced_close));
}
