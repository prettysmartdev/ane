use std::collections::HashMap;
use std::io::{IsTerminal, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::data::buffer::Buffer;
use crate::data::lsp::registry;
use crate::data::lsp::types::ServerState;

use super::chord_engine::types::{ChordArgs, ChordQuery};
use super::chord_engine::ChordEngine;
use super::lsp_engine::LspEngine;

pub trait FrontendCapabilities {
    fn is_interactive(&self) -> bool;
}

#[cfg(test)]
struct HeadlessContext;
#[cfg(test)]
impl FrontendCapabilities for HeadlessContext {
    fn is_interactive(&self) -> bool {
        false
    }
}

pub fn parse_chord(input: &str) -> Result<ChordQuery> {
    ChordEngine::parse(input)
}

fn args_are_empty(args: &ChordArgs) -> bool {
    args.target_name.is_none()
        && args.parent_name.is_none()
        && args.target_line.is_none()
        && args.cursor_pos.is_none()
        && args.value.is_none()
        && args.find.is_none()
        && args.replace.is_none()
}

#[derive(Debug)]
pub struct ChordResult {
    pub original: String,
    pub modified: String,
    pub warnings: Vec<String>,
    pub yanked: Option<String>,
}

pub fn execute_chord(
    frontend: &dyn FrontendCapabilities,
    path: &Path,
    chord: &ChordQuery,
    lsp: &mut LspEngine,
) -> Result<ChordResult> {
    if chord.action.requires_interactive() && !frontend.is_interactive() {
        bail!("Jump action requires an interactive frontend; use ane in TUI mode");
    }

    if !path.exists() {
        bail!("file not found: {}", path.display());
    }

    let abs_path = std::fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize path: {}", path.display()))?;

    let buffer = Buffer::from_file(&abs_path)?;
    let original = buffer.content();
    let path_str = abs_path.to_string_lossy().to_string();
    let mut buffers = HashMap::new();
    buffers.insert(path_str.clone(), buffer);

    let mut chord = chord.clone();
    resolve_stdin_sentinels(&mut chord)?;

    if args_are_empty(&chord.args) {
        bail!(
            "exec mode requires explicit parameters, e.g. {}(fn_name, \"body\")",
            chord.short_form()
        );
    }

    if chord.requires_lsp {
        let lsp_timeout = lsp.startup_timeout();
        let lang = registry::detect_language_from_path(&abs_path).ok_or_else(|| {
            anyhow::anyhow!("no language server available for {}", path.display())
        })?;

        let root_path = abs_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot determine parent directory"))?;

        lsp.start_for_context(root_path, &[&abs_path])?;

        let state = lsp.await_ready(lang, lsp_timeout)?;

        match state {
            ServerState::Running => {}
            ServerState::Failed => {
                bail!("LSP server for {} failed to start", lang.name());
            }
            _ => {
                bail!(
                    "LSP server for {} did not become ready within 30s",
                    lang.name()
                );
            }
        }
    }

    let resolved = ChordEngine::resolve(&chord, &buffers, lsp)?;
    let actions = ChordEngine::patch(&resolved, &buffers)?;

    if let Some(action) = actions.get(&path_str) {
        let modified = if let Some(ref diff) = action.diff {
            diff.modified.clone()
        } else {
            original.clone()
        };

        if modified != original {
            std::fs::write(&abs_path, &modified)?;
        }

        Ok(ChordResult {
            original,
            modified,
            warnings: action.warnings.clone(),
            yanked: action.yanked_content.clone(),
        })
    } else {
        Ok(ChordResult {
            original: original.clone(),
            modified: original,
            warnings: Vec::new(),
            yanked: None,
        })
    }
}

fn strip_trailing_newline(s: &mut String) {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
}

fn resolve_stdin_sentinels(chord: &mut ChordQuery) -> Result<()> {
    let has_sentinel = [
        chord.args.value.as_deref(),
        chord.args.target_name.as_deref(),
        chord.args.find.as_deref(),
        chord.args.replace.as_deref(),
    ]
    .contains(&Some("-"));

    if !has_sentinel {
        return Ok(());
    }

    if std::io::stdin().is_terminal() {
        bail!("chord parameter '-' requires piped input on stdin");
    }

    let mut stdin_content = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_content)
        .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;

    // Strip exactly one trailing newline so `echo "foo" | ane exec ...` matches a literal "foo"
    strip_trailing_newline(&mut stdin_content);

    // All `-` sentinels share the same stdin read
    if chord.args.value.as_deref() == Some("-") {
        chord.args.value = Some(stdin_content.clone());
    }
    if chord.args.target_name.as_deref() == Some("-") {
        chord.args.target_name = Some(stdin_content.clone());
    }
    if chord.args.find.as_deref() == Some("-") {
        chord.args.find = Some(stdin_content.clone());
    }
    if chord.args.replace.as_deref() == Some("-") {
        chord.args.replace = Some(stdin_content);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::lsp_engine::LspEngineConfig;
    use crate::data::chord_types::{Action, Component, Positional, Scope};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    fn default_engine() -> LspEngine {
        LspEngine::new(LspEngineConfig::default())
    }

    // --- strip_trailing_newline ---

    #[test]
    fn trailing_newline_strip_removes_lf() {
        let mut s = "foo\n".to_string();
        strip_trailing_newline(&mut s);
        assert_eq!(s, "foo");
    }

    #[test]
    fn trailing_newline_strip_no_change_without_newline() {
        let mut s = "foo".to_string();
        strip_trailing_newline(&mut s);
        assert_eq!(s, "foo");
    }

    #[test]
    fn trailing_newline_strip_removes_crlf() {
        let mut s = "foo\r\n".to_string();
        strip_trailing_newline(&mut s);
        assert_eq!(s, "foo");
    }

    // --- stdin-is-TTY rejection ---

    #[test]
    fn stdin_sentinel_tty_rejection_exact_message() {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            // Can only assert TTY rejection when stdin is an interactive terminal.
            // When piped (e.g. in CI), this path is exercised by binary tests instead.
            return;
        }
        let mut chord = parse_chord("cels").unwrap();
        chord.args.target_line = Some(0);
        chord.args.value = Some("-".to_string());

        let result = resolve_stdin_sentinels(&mut chord);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "chord parameter '-' requires piped input on stdin"
        );
    }

    // --- error message formats ---

    #[test]
    fn error_message_file_not_found_exact_format() {
        let chord = parse_chord("cels").unwrap();
        let result = execute_chord(
            &HeadlessContext,
            Path::new("/nonexistent/path/does-not-exist.rs"),
            &chord,
            &mut default_engine(),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("file not found: /nonexistent/path/does-not-exist.rs"),
            "got: {msg}"
        );
    }

    #[test]
    fn error_message_exec_requires_explicit_params() {
        // LSP-scoped chord with no target_name or cursor_pos triggers this error before
        // any LSP server is started.
        let mut f_rs = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
        f_rs.write_all(b"fn main() {}").unwrap();
        f_rs.flush().unwrap();

        let chord = parse_chord("cifc").unwrap(); // ChangeInsideFunctionContents, requires_lsp=true
                                                  // No target_name or cursor_pos set → error before LSP starts
        let result = execute_chord(&HeadlessContext, f_rs.path(), &chord, &mut default_engine());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exec mode requires explicit parameters"),
            "got: {msg}"
        );
    }

    #[test]
    fn error_message_bare_chord_rejected_for_line_scope() {
        // Non-LSP scope without args must also be rejected.
        let f = temp_file("hello\n");
        let chord = parse_chord("cels").unwrap();
        let result = execute_chord(&HeadlessContext, f.path(), &chord, &mut default_engine());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exec mode requires explicit parameters"),
            "got: {msg}"
        );
    }

    #[test]
    fn error_message_no_language_server_for_non_rust_file() {
        // Non-.rs file with an LSP-scoped chord that has a target_name set.
        // Fails with "no language server available" after the param check.
        let mut f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        f.write_all(b"some content").unwrap();
        f.flush().unwrap();

        let mut chord = parse_chord("cifc").unwrap();
        chord.args.target_name = Some("some_fn".to_string());

        let result = execute_chord(&HeadlessContext, f.path(), &chord, &mut default_engine());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no language server available"), "got: {msg}");
    }

    #[test]
    fn parse_short_form_cifc() {
        let parsed = parse_chord("cifc").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Inside);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Contents);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_long_form() {
        let parsed = parse_chord("ChangeInsideFunctionContents").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Inside);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Contents);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_change_entire_line_self() {
        let parsed = parse_chord("cels").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
        assert!(!parsed.requires_lsp);
    }

    #[test]
    fn parse_delete_entire_line_self() {
        let parsed = parse_chord("dels").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
    }

    #[test]
    fn parse_delete_entire_line_self_long() {
        let parsed = parse_chord("DeleteEntireLineSelf").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::Self_);
    }

    #[test]
    fn parse_yank_entire_function_contents() {
        let parsed = parse_chord("yefc").unwrap();
        assert_eq!(parsed.action, Action::Yank);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Contents);
        assert!(parsed.requires_lsp);
    }

    #[test]
    fn parse_append_after_line_end() {
        let parsed = parse_chord("aale").unwrap();
        assert_eq!(parsed.action, Action::Append);
        assert_eq!(parsed.positional, Positional::After);
        assert_eq!(parsed.scope, Scope::Line);
        assert_eq!(parsed.component, Component::End);
        assert!(!parsed.requires_lsp);
    }

    #[test]
    fn parse_with_args() {
        let parsed = parse_chord("cifp(function:getData, value:\"(x: i32)\")").unwrap();
        assert_eq!(parsed.action, Action::Change);
        assert_eq!(parsed.scope, Scope::Function);
        assert_eq!(parsed.component, Component::Parameters);
        assert_eq!(parsed.args.target_name.as_deref(), Some("getData"));
        assert_eq!(parsed.args.value.as_deref(), Some("(x: i32)"));
    }

    #[test]
    fn parse_invalid_combination() {
        let result = parse_chord("cilp");
        assert!(result.is_err());
    }

    #[test]
    fn execute_change_line() {
        let f = temp_file("aaa\nbbb\nccc");
        let mut chord = parse_chord("cels").unwrap();
        chord.args.target_line = Some(1);
        chord.args.value = Some("xxx".to_string());
        let result =
            execute_chord(&HeadlessContext, f.path(), &chord, &mut default_engine()).unwrap();
        assert!(result.modified.contains("xxx"));
        assert!(!result.modified.contains("bbb"));
    }

    #[test]
    fn execute_delete_line() {
        let f = temp_file("aaa\nbbb\nccc");
        let mut chord = parse_chord("dels").unwrap();
        chord.args.target_line = Some(1);
        let result =
            execute_chord(&HeadlessContext, f.path(), &chord, &mut default_engine()).unwrap();
        assert!(!result.modified.contains("bbb"));
        assert!(result.modified.contains("aaa"));
        assert!(result.modified.contains("ccc"));
    }

    #[test]
    fn parse_unknown_chord() {
        let result = parse_chord("zzzz");
        assert!(result.is_err());
    }

    #[test]
    fn parse_delete_entire_buffer_self() {
        let parsed = parse_chord("debs").unwrap();
        assert_eq!(parsed.action, Action::Delete);
        assert_eq!(parsed.positional, Positional::Entire);
        assert_eq!(parsed.scope, Scope::Buffer);
        assert_eq!(parsed.component, Component::Self_);
        assert!(!parsed.requires_lsp);
    }

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn execute_chord_jump_rejects_before_file_io_check() {
        // Jump on a non-interactive frontend must fail with the interactive error
        // BEFORE the file-not-found check fires — even for a nonexistent path.
        let chord = parse_chord("jefc").unwrap();
        let result = execute_chord(
            &HeadlessContext,
            Path::new("/nonexistent/path/does-not-exist.rs"),
            &chord,
            &mut default_engine(),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("interactive") || msg.contains("Jump"),
            "expected interactive error before file-not-found, got: {msg}"
        );
        assert!(
            !msg.contains("file not found"),
            "file-not-found must not fire before interactive check, got: {msg}"
        );
    }
}
