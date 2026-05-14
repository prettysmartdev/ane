#![cfg(feature = "frontends")]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ane::commands::chord::{execute_chord, parse_chord};
use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::frontend::cli_frontend::CliFrontend;

const ANE_BIN: &str = env!("CARGO_BIN_EXE_ane");
const MOCK_SERVER: &str = env!("CARGO_BIN_EXE_mock_lsp_server");

fn temp_rs_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

// --- Integration: Line-scope exec without LSP ---

#[test]
fn line_scope_exec_updates_file_and_does_not_start_lsp() {
    // Use a config that would fail immediately if LSP were accidentally started:
    // the binary doesn't exist, so spawning it would produce ENOENT → Failed state → error.
    // Because the chord is line-scope (requires_lsp=false), the LSP block is never entered
    // and the function succeeds even with this poisoned config.
    let failing_if_lsp_started = LspEngineConfig::default().with_server_override(
        "nonexistent-binary-must-not-be-started-xyzzy",
        vec![],
        "true",
    );

    let f = temp_rs_file("first line\nsecond line\nthird line");

    let mut chord = parse_chord("cels").unwrap();
    chord.args.target_line = Some(1);
    chord.args.value = Some("replaced".to_string());

    let mut lsp = LspEngine::new(failing_if_lsp_started);
    let result = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp).unwrap();

    assert!(
        result.modified.contains("replaced"),
        "modified: {}",
        result.modified
    );
    assert!(!result.modified.contains("second line"));

    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(on_disk.contains("replaced"));
    assert!(!on_disk.contains("second line"));
}

// --- Integration: LSP-scope exec with mock server ---

#[test]
fn lsp_scope_exec_with_mock_server_modifies_file() {
    // The mock LSP server always reports a "main" Function symbol at lines 0–10.
    // We create an 11-line file so the range is in-bounds, then run cefs
    // (Change Entire Function Self) targeting "main".  The full pipeline runs:
    // parse → LSP start → await ready → resolve symbols → patch → write to disk.
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

    let mut chord = parse_chord("cefs").unwrap();
    chord.args.target_name = Some("main".to_string());
    chord.args.value = Some("// replaced by test".to_string());

    let mut lsp = LspEngine::new(config);
    let result = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp).unwrap();

    assert!(
        result.modified.contains("// replaced by test"),
        "modified: {}",
        result.modified
    );

    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(
        on_disk.contains("// replaced by test"),
        "on disk: {on_disk}"
    );
}

// --- Integration: LSP-scope exec with startup failure ---

#[test]
fn lsp_scope_exec_with_nonexistent_binary_returns_error_promptly() {
    // check_command="true" (server appears installed) but the binary doesn't exist.
    // The startup thread skips the installer and immediately tries to spawn the binary,
    // which fails with ENOENT → Failed state.  execute_chord should return
    // an error containing "failed to start" well within the 5-second budget.
    let config = LspEngineConfig::default()
        .with_startup_timeout(Duration::from_secs(5))
        .with_server_override("nonexistent-binary-xyzzy-67890", vec![], "true");

    let f = temp_rs_file("fn main() {}");

    let mut chord = parse_chord("cefs").unwrap();
    chord.args.target_name = Some("main".to_string());
    chord.args.value = Some("// new".to_string());

    let mut lsp = LspEngine::new(config);
    let started = Instant::now();
    let result = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp);
    let elapsed = started.elapsed();

    assert!(result.is_err(), "expected an error, got Ok");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("failed to start"), "error: {msg}");
    assert!(
        elapsed < Duration::from_secs(8),
        "should return promptly, took {:?}",
        elapsed
    );
}

// --- Integration: no-change idempotent exec (via binary) ---

#[test]
fn no_change_chord_produces_empty_stdout_and_zero_exit() {
    let f = temp_rs_file("hello world");

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            r#"cels(target:0, value:"hello world")"#,
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "expected empty stdout for no-change chord, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

// --- Integration: binary file rejection ---

#[test]
fn binary_file_rejected_with_nonzero_exit_and_stderr() {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    // Write bytes that are not valid UTF-8.
    f.write_all(&[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90]).unwrap();
    f.flush().unwrap();

    let mut chord = parse_chord("cels").unwrap();
    chord.args.target_line = Some(0);
    chord.args.value = Some("x".to_string());

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    let result = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp);
    assert!(result.is_err(), "expected error for binary file");
    // Buffer::from_file uses read_to_string which fails on non-UTF-8 content.
    // anyhow wraps the IO error with a "reading <path>" context message.
}

#[test]
fn binary_file_error_message_matches_spec() {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(&[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90]).unwrap();
    f.flush().unwrap();

    let mut chord = parse_chord("cels").unwrap();
    chord.args.target_line = Some(0);
    chord.args.value = Some("x".to_string());

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    let err = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp).unwrap_err();
    let msg = err.to_string();
    assert!(msg.starts_with("file is not valid UTF-8: "), "got: {msg}");
    assert!(msg.contains(&f.path().display().to_string()), "got: {msg}");
}

#[test]
fn binary_file_via_binary_exits_nonzero() {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(&[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90]).unwrap();
    f.flush().unwrap();

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            "cels(target:0, value:x)",
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");

    assert!(
        !output.status.success(),
        "expected non-zero exit for binary file"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "expected error on stderr for binary file"
    );
}

// --- Integration: out-of-range target ---

#[test]
fn out_of_range_line_returns_error() {
    let f = temp_rs_file("line 0\nline 1\nline 2");

    let mut chord = parse_chord("cels").unwrap();
    chord.args.target_line = Some(100); // file has 3 lines (0-2)
    chord.args.value = Some("x".to_string());

    let mut lsp = LspEngine::new(LspEngineConfig::default());
    let result = execute_chord(&CliFrontend, f.path(), &chord, &mut lsp);
    assert!(result.is_err(), "expected error for out-of-range line");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("out of range"), "error: {msg}");
}

#[test]
fn out_of_range_line_via_binary_exits_nonzero() {
    let f = temp_rs_file("line 0\nline 1\nline 2");

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            "cels(target:100, value:x)",
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");

    assert!(
        !output.status.success(),
        "expected non-zero exit for out-of-range line"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("out of range"),
        "stderr should mention out of range, got: {stderr}"
    );
}

// --- Integration: Yank action ---

#[test]
fn yank_writes_content_to_stdout_not_to_file() {
    let content = "the content to yank\nsecond line";
    let f = temp_rs_file(content);

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            "yels(target:0)",
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("the content to yank"), "stdout: {stdout}");

    // File must be unchanged.
    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert_eq!(on_disk, content, "yank must not modify the file");
}

// --- End-to-end smoke test ---

#[test]
fn end_to_end_smoke_test_diff_output_and_file_update() {
    let f = temp_rs_file("first line\nsecond line\nthird line");

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            r#"cels(target:0, value:"replaced first line")"#,
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Unified diff must show the removed and added lines.
    assert!(
        stdout.contains("-first line"),
        "diff should show removed line, stdout: {stdout}"
    );
    assert!(
        stdout.contains("+replaced first line"),
        "diff should show added line, stdout: {stdout}"
    );

    // File on disk must be updated.
    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(
        on_disk.contains("replaced first line"),
        "file not updated: {on_disk}"
    );
    // The original un-replaced text "first line" was on line 0; after replacement
    // it becomes "replaced first line". Check the old standalone line is gone by
    // verifying the file no longer starts with "first line".
    assert!(
        !on_disk.starts_with("first line"),
        "old first line should be gone: {on_disk}"
    );
}

// --- Integration: trailing newline preservation ---

#[test]
fn binary_preserves_trailing_newline_when_present() {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(b"fn main() {}\n").unwrap();
    f.flush().unwrap();

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            r#"cels(target:0, value:"fn other() {}")"#,
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");
    assert!(output.status.success());

    let bytes = std::fs::read(f.path()).unwrap();
    assert_eq!(bytes, b"fn other() {}\n");
}

#[test]
fn binary_preserves_absence_of_trailing_newline() {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(b"fn main() {}").unwrap();
    f.flush().unwrap();

    let output = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            r#"cels(target:0, value:"fn other() {}")"#,
            &f.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");
    assert!(output.status.success());

    let bytes = std::fs::read(f.path()).unwrap();
    assert_eq!(bytes, b"fn other() {}");
}

// --- Integration: stdin sentinel ---

#[test]
fn stdin_with_trailing_newline_matches_inline_value() {
    // `echo "piped value"` (stdin "piped value\n") must produce the same
    // file content as passing `value:"piped value"` inline.
    let mut from_stdin = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    from_stdin.write_all(b"first line\n").unwrap();
    from_stdin.flush().unwrap();

    let mut from_inline = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    from_inline.write_all(b"first line\n").unwrap();
    from_inline.flush().unwrap();

    let mut child = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            "cels(target:0, value:-)",
            &from_stdin.path().to_string_lossy(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ane binary");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"piped value\n").unwrap();
    }
    let out_stdin = child.wait_with_output().expect("wait failed");
    assert!(
        out_stdin.status.success(),
        "stdin run failed: {}",
        String::from_utf8_lossy(&out_stdin.stderr)
    );

    let out_inline = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            r#"cels(target:0, value:"piped value")"#,
            &from_inline.path().to_string_lossy(),
        ])
        .output()
        .expect("failed to run ane binary");
    assert!(
        out_inline.status.success(),
        "inline run failed: {}",
        String::from_utf8_lossy(&out_inline.stderr)
    );

    let bytes_stdin = std::fs::read(from_stdin.path()).unwrap();
    let bytes_inline = std::fs::read(from_inline.path()).unwrap();
    assert_eq!(
        bytes_stdin, bytes_inline,
        "stdin and inline must produce identical bytes"
    );
}

#[test]
fn stdin_sentinel_piped_content_replaces_value_parameter() {
    let f = temp_rs_file("original content\nsecond line");

    let mut child = Command::new(ANE_BIN)
        .args([
            "exec",
            "--chord",
            "cels(target:0, value:-)",
            &f.path().to_string_lossy(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ane binary");

    // Write to the child's stdin then close it so the child sees EOF.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"piped replacement\n").unwrap();
    }

    let output = child.wait_with_output().expect("failed to wait on child");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(on_disk.contains("piped replacement"), "file: {on_disk}");
    assert!(
        !on_disk.contains("original content"),
        "old content should be gone: {on_disk}"
    );
}
