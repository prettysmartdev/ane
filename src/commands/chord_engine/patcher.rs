use std::collections::HashMap;

use anyhow::Result;
use similar::TextDiff;

use crate::data::buffer::Buffer;
use crate::data::chord_types::Action;

use super::errors::ChordError;
use super::text::{apply_replacements, apply_single_replacement, extract_range_text};
use super::types::{
    BufferResolution, ChordAction, ChordQuery, DiffHunk, DiffLine, ResolvedChord, TextRange,
    UnifiedDiff,
};

pub fn patch(
    resolved: &ResolvedChord,
    buffers: &HashMap<String, Buffer>,
) -> Result<HashMap<String, ChordAction>> {
    let mut actions = HashMap::new();

    for (name, resolution) in &resolved.resolutions {
        let buffer = buffers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("buffer '{name}' not found"))?;
        let action = build_action(name, buffer, resolution, &resolved.query)?;
        actions.insert(name.clone(), action);
    }

    Ok(actions)
}

fn build_action(
    buffer_name: &str,
    buffer: &Buffer,
    resolution: &BufferResolution,
    query: &ChordQuery,
) -> Result<ChordAction> {
    let original = buffer.content();
    let mut warnings = Vec::new();

    if resolution.target_ranges.is_empty() {
        return Err(ChordError::patch(buffer_name, "no target ranges resolved").into());
    }

    if query.action == Action::Yank {
        let yanked = resolution
            .target_ranges
            .iter()
            .map(|r| extract_range_text(buffer, r))
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(ChordAction {
            buffer_name: buffer_name.to_string(),
            diff: None,
            yanked_content: Some(yanked),
            cursor_destination: resolution.cursor_destination,
            mode_after: resolution.mode_after,
            highlight_ranges: resolution.target_ranges.clone(),
            warnings,
        });
    }

    if query.action == Action::Delete {
        let last = buffer.line_count().saturating_sub(1);
        let covers_buffer = resolution.target_ranges.iter().any(|r| {
            r.start_line == 0
                && r.start_col == 0
                && r.end_line >= last
                && r.end_col
                    >= buffer
                        .lines
                        .get(last)
                        .map(|l| l.chars().count())
                        .unwrap_or(0)
        });
        if covers_buffer {
            warnings.push("this deletes the entire buffer content".to_string());
        }
    }

    let modified = match query.action {
        Action::Delete => apply_to_ranges(buffer, &resolution.target_ranges, ""),
        Action::Change => {
            let replacement = resolution.replacement.as_deref().unwrap_or("");
            apply_to_ranges(buffer, &resolution.target_ranges, replacement)
        }
        Action::Replace => {
            let primary = resolution.target_ranges[0];
            let find = query.args.find.as_deref();
            let replace = query.args.replace.as_deref();
            match (find, replace) {
                (Some(f), Some(r)) => {
                    let old = extract_range_text(buffer, &primary);
                    let new = old.replace(f, r);
                    apply_single_replacement(buffer, &primary, &new)
                }
                _ => {
                    let replacement = resolution.replacement.as_deref().unwrap_or("");
                    apply_to_ranges(buffer, &resolution.target_ranges, replacement)
                }
            }
        }
        Action::Append => {
            let insertion = resolution.replacement.as_deref().unwrap_or("");
            let last = resolution.target_ranges.last().copied().unwrap();
            let point = TextRange::point(last.end_line, last.end_col);
            apply_single_replacement(buffer, &point, insertion)
        }
        Action::Prepend => {
            let insertion = resolution.replacement.as_deref().unwrap_or("");
            let first = resolution.target_ranges[0];
            let point = TextRange::point(first.start_line, first.start_col);
            apply_single_replacement(buffer, &point, insertion)
        }
        Action::Insert => {
            let insertion = resolution.replacement.as_deref().unwrap_or("");
            let cursor = query
                .args
                .cursor_pos
                .map(|(l, c)| TextRange::point(l, c))
                .unwrap_or_else(|| {
                    let first = resolution.target_ranges[0];
                    TextRange::point(first.start_line, first.start_col)
                });
            apply_single_replacement(buffer, &cursor, insertion)
        }
        Action::Yank => unreachable!(),
    };

    let diff = generate_diff(&original, &modified);

    Ok(ChordAction {
        buffer_name: buffer_name.to_string(),
        diff: Some(diff),
        yanked_content: None,
        cursor_destination: resolution.cursor_destination,
        mode_after: resolution.mode_after,
        highlight_ranges: resolution.target_ranges.clone(),
        warnings,
    })
}

fn apply_to_ranges(buffer: &Buffer, ranges: &[TextRange], replacement: &str) -> String {
    let edits: Vec<(TextRange, String)> = ranges
        .iter()
        .map(|r| (*r, replacement.to_string()))
        .collect();
    apply_replacements(buffer, &edits)
}

fn generate_diff(original: &str, modified: &str) -> UnifiedDiff {
    let text_diff = TextDiff::from_lines(original, modified);
    let mut hunks = Vec::new();

    for hunk in text_diff.unified_diff().context_radius(3).iter_hunks() {
        let mut lines = Vec::new();
        let mut old_start = 0;
        let mut old_count = 0;
        let mut new_start = 0;
        let mut new_count = 0;

        for change in hunk.iter_changes() {
            match change.tag() {
                similar::ChangeTag::Equal => {
                    lines.push(DiffLine::Context(change.value().to_string()));
                    old_count += 1;
                    new_count += 1;
                    if old_start == 0 {
                        old_start = change.old_index().unwrap_or(0) + 1;
                    }
                    if new_start == 0 {
                        new_start = change.new_index().unwrap_or(0) + 1;
                    }
                }
                similar::ChangeTag::Delete => {
                    lines.push(DiffLine::Removed(change.value().to_string()));
                    old_count += 1;
                    if old_start == 0 {
                        old_start = change.old_index().unwrap_or(0) + 1;
                    }
                    if new_start == 0 {
                        new_start = change.new_index().unwrap_or(0) + 1;
                    }
                }
                similar::ChangeTag::Insert => {
                    lines.push(DiffLine::Added(change.value().to_string()));
                    new_count += 1;
                    if old_start == 0 {
                        old_start = change.old_index().unwrap_or(0) + 1;
                    }
                    if new_start == 0 {
                        new_start = change.new_index().unwrap_or(0) + 1;
                    }
                }
            }
        }

        hunks.push(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        });
    }

    UnifiedDiff {
        original: original.to_string(),
        modified: modified.to_string(),
        hunks,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::commands::chord_engine::types::{
        BufferResolution, ChordArgs, ChordQuery, CursorPosition, EditorMode, ResolvedChord,
        TextRange,
    };
    use crate::data::buffer::Buffer;
    use crate::data::chord_types::{Action, Component, Positional, Scope};

    use super::patch;

    fn buf(lines: &[&str]) -> Buffer {
        Buffer {
            path: PathBuf::from("/test/file.rs"),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            dirty: false,
        }
    }

    fn make_query(action: Action) -> ChordQuery {
        ChordQuery {
            action,
            positional: Positional::Entire,
            scope: Scope::Line,
            component: Component::Self_,
            args: ChordArgs::default(),
            requires_lsp: false,
        }
    }

    fn make_resolution(
        target: TextRange,
        replacement: Option<&str>,
        cursor: Option<CursorPosition>,
        mode: Option<EditorMode>,
    ) -> BufferResolution {
        BufferResolution {
            target_ranges: vec![target],
            scope_range: target,
            component_range: target,
            replacement: replacement.map(String::from),
            cursor_destination: cursor,
            mode_after: mode,
        }
    }

    fn make_multi_resolution(
        targets: Vec<TextRange>,
        scope: TextRange,
        replacement: Option<&str>,
    ) -> BufferResolution {
        BufferResolution {
            target_ranges: targets,
            scope_range: scope,
            component_range: scope,
            replacement: replacement.map(String::from),
            cursor_destination: None,
            mode_after: None,
        }
    }

    fn run_patch(
        action: Action,
        lines: &[&str],
        target: TextRange,
        replacement: Option<&str>,
        cursor: Option<CursorPosition>,
        mode: Option<EditorMode>,
    ) -> crate::commands::chord_engine::types::ChordAction {
        let name = "test_buf";
        let buffer = buf(lines);
        let query = make_query(action);
        let resolution = make_resolution(target, replacement, cursor, mode);
        let mut resolutions = HashMap::new();
        resolutions.insert(name.to_string(), resolution);
        let resolved = ResolvedChord { query, resolutions };
        let mut buffers = HashMap::new();
        buffers.insert(name.to_string(), buffer);
        patch(&resolved, &buffers).unwrap().remove(name).unwrap()
    }

    #[test]
    fn change_action_replaces_entire_line() {
        let target = TextRange {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 6,
        };
        let action = run_patch(
            Action::Change,
            &["first", "second", "third"],
            target,
            Some("replaced"),
            Some(CursorPosition { line: 1, col: 0 }),
            Some(EditorMode::Chord),
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("replaced"));
        assert!(!diff.modified.contains("second"));
        assert!(diff.modified.contains("first"));
        assert!(diff.modified.contains("third"));
    }

    #[test]
    fn change_action_partial_line() {
        let target = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 11,
        };
        let action = run_patch(
            Action::Change,
            &["hello world"],
            target,
            Some("rust"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(
            diff.modified.contains("hello rust"),
            "got: {}",
            diff.modified
        );
    }

    #[test]
    fn change_with_unicode_replacement_preserved() {
        let target = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 11,
        };
        let action = run_patch(
            Action::Change,
            &["hello world"],
            target,
            Some("wörld"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("wörld"));
    }

    #[test]
    fn change_with_unicode_buffer_byte_safe() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let action = run_patch(Action::Change, &["héllo"], target, Some("hi"), None, None);
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("hi"));
        assert!(!diff.modified.contains("héllo"));
    }

    #[test]
    fn change_replacement_with_trailing_newline_preserved() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let action = run_patch(Action::Change, &["hello"], target, Some("a\n"), None, None);
        let diff = action.diff.as_ref().unwrap();
        let lines: Vec<&str> = diff.modified.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "a");
        assert_eq!(lines[1], "");
    }

    #[test]
    fn delete_action_removes_range() {
        let target = TextRange {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 3,
        };
        let action = run_patch(
            Action::Delete,
            &["aaa", "bbb", "ccc"],
            target,
            None,
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(!diff.modified.contains("bbb"));
    }

    #[test]
    fn delete_entire_buffer_emits_warning() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 2,
            end_col: 5,
        };
        let action = run_patch(
            Action::Delete,
            &["aaa", "bbb", "ccccc"],
            target,
            None,
            None,
            None,
        );
        assert!(!action.warnings.is_empty());
    }

    #[test]
    fn yank_action_no_diff_captures_content() {
        let target = TextRange {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 6,
        };
        let action = run_patch(
            Action::Yank,
            &["first", "second", "third"],
            target,
            None,
            None,
            None,
        );
        assert!(action.diff.is_none());
        assert_eq!(action.yanked_content.as_deref(), Some("second"));
    }

    #[test]
    fn yank_multi_range_joins_with_newline() {
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let r1 = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let r2 = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 11,
        };
        let name = "test_buf";
        let buffer = buf(&["hello world"]);
        let query = make_query(Action::Yank);
        let resolution = make_multi_resolution(vec![r1, r2], scope, None);
        let mut resolutions = HashMap::new();
        resolutions.insert(name.to_string(), resolution);
        let resolved = ResolvedChord { query, resolutions };
        let mut buffers = HashMap::new();
        buffers.insert(name.to_string(), buffer);
        let action = patch(&resolved, &buffers).unwrap().remove(name).unwrap();
        let yanked = action.yanked_content.unwrap();
        assert!(yanked.contains("hello") && yanked.contains("world"));
    }

    #[test]
    fn append_action_inserts_after_range_end() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let action = run_patch(
            Action::Append,
            &["hello world"],
            target,
            Some("!"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("hello! world"));
    }

    #[test]
    fn prepend_action_inserts_before_range_start() {
        let target = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 11,
        };
        let action = run_patch(
            Action::Prepend,
            &["hello world"],
            target,
            Some("dear "),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("hello dear world"));
    }

    #[test]
    fn insert_action_at_cursor_position() {
        let mut query = make_query(Action::Insert);
        query.args.cursor_pos = Some((0, 5));
        let name = "test_buf";
        let buffer = buf(&["hello world"]);
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let resolution = make_resolution(target, Some("!"), None, None);
        let mut resolutions = HashMap::new();
        resolutions.insert(name.to_string(), resolution);
        let resolved = ResolvedChord { query, resolutions };
        let mut buffers = HashMap::new();
        buffers.insert(name.to_string(), buffer);
        let action = patch(&resolved, &buffers).unwrap().remove(name).unwrap();
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("hello! world"));
    }

    #[test]
    fn replace_action_with_find_and_replace() {
        let mut query = make_query(Action::Replace);
        query.args.find = Some("foo".to_string());
        query.args.replace = Some("bar".to_string());
        let name = "test_buf";
        let buffer = buf(&["foo and foo"]);
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let resolution = make_resolution(target, None, None, None);
        let mut resolutions = HashMap::new();
        resolutions.insert(name.to_string(), resolution);
        let resolved = ResolvedChord { query, resolutions };
        let mut buffers = HashMap::new();
        buffers.insert(name.to_string(), buffer);
        let action = patch(&resolved, &buffers).unwrap().remove(name).unwrap();
        let diff = action.diff.as_ref().unwrap();
        assert_eq!(diff.modified, "bar and bar");
    }

    #[test]
    fn replace_without_find_falls_back_to_change() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let action = run_patch(
            Action::Replace,
            &["hello"],
            target,
            Some("world"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.modified.contains("world"));
    }

    #[test]
    fn outside_change_replaces_two_ranges() {
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let r1 = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let r2 = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 11,
        };
        let name = "test_buf";
        let buffer = buf(&["hello world"]);
        let query = make_query(Action::Change);
        let resolution = make_multi_resolution(vec![r1, r2], scope, Some("X"));
        let mut resolutions = HashMap::new();
        resolutions.insert(name.to_string(), resolution);
        let resolved = ResolvedChord { query, resolutions };
        let mut buffers = HashMap::new();
        buffers.insert(name.to_string(), buffer);
        let action = patch(&resolved, &buffers).unwrap().remove(name).unwrap();
        let diff = action.diff.as_ref().unwrap();
        assert_eq!(diff.modified, "X X");
    }

    #[test]
    fn cursor_destination_passed_through() {
        let target = TextRange {
            start_line: 3,
            start_col: 7,
            end_line: 3,
            end_col: 12,
        };
        let expected = CursorPosition { line: 3, col: 7 };
        let action = run_patch(
            Action::Change,
            &["a", "b", "c", "hello world", "e"],
            target,
            Some("x"),
            Some(expected),
            Some(EditorMode::Edit),
        );
        assert_eq!(action.cursor_destination, Some(expected));
    }

    #[test]
    fn highlight_range_equals_target_ranges() {
        let target = TextRange {
            start_line: 1,
            start_col: 2,
            end_line: 1,
            end_col: 8,
        };
        let action = run_patch(
            Action::Change,
            &["aaa", "bbbbbb", "ccc"],
            target,
            Some("x"),
            None,
            None,
        );
        assert_eq!(action.highlight_ranges, vec![target]);
    }

    #[test]
    fn diff_hunks_contain_added_and_removed_lines() {
        let target = TextRange {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 3,
        };
        let action = run_patch(
            Action::Change,
            &["aaa", "bbb", "ccc"],
            target,
            Some("xxx"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(!diff.hunks.is_empty());
        use crate::commands::chord_engine::types::DiffLine;
        let all_lines: Vec<&DiffLine> = diff.hunks.iter().flat_map(|h| h.lines.iter()).collect();
        assert!(all_lines.iter().any(|l| matches!(l, DiffLine::Removed(_))));
        assert!(all_lines.iter().any(|l| matches!(l, DiffLine::Added(_))));
    }

    #[test]
    fn no_change_produces_empty_hunks() {
        let target = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let action = run_patch(
            Action::Change,
            &["hello"],
            target,
            Some("hello"),
            None,
            None,
        );
        let diff = action.diff.as_ref().unwrap();
        assert!(diff.hunks.is_empty());
    }
}
