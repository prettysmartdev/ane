use similar::{ChangeTag, TextDiff};

pub fn unified_diff(path: &str, original: &str, modified: &str) -> String {
    if original == modified {
        return String::new();
    }

    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();

    let clean = path.strip_prefix('/').unwrap_or(path);
    output.push_str(&format!("--- a/{clean}\n"));
    output.push_str(&format!("+++ b/{clean}\n"));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{hunk}"));
    }

    output
}

pub fn colored_unified_diff(path: &str, original: &str, modified: &str) -> String {
    if original == modified {
        return String::new();
    }

    const BOLD: &str = "\x1b[1m";
    const CYAN: &str = "\x1b[36m";
    const RED_BG: &str = "\x1b[41;97m";
    const GREEN_BG: &str = "\x1b[42;30m";
    const RESET: &str = "\x1b[0m";
    const DIM: &str = "\x1b[2m";

    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();

    let clean = path.strip_prefix('/').unwrap_or(path);
    output.push_str(&format!("{BOLD}--- a/{clean}{RESET}\n"));
    output.push_str(&format!("{BOLD}+++ b/{clean}{RESET}\n"));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{CYAN}{}{RESET}\n", hunk.header()));
        for change in hunk.iter_changes() {
            let line = change.value();
            let line_no_newline = line.strip_suffix('\n').unwrap_or(line);
            match change.tag() {
                ChangeTag::Delete => {
                    output.push_str(&format!("{RED_BG}-{line_no_newline}{RESET}\n"));
                }
                ChangeTag::Insert => {
                    output.push_str(&format!("{GREEN_BG}+{line_no_newline}{RESET}\n"));
                }
                ChangeTag::Equal => {
                    output.push_str(&format!("{DIM} {line_no_newline}{RESET}\n"));
                }
            }
        }
    }

    output
}

pub fn collect_diffs(changes: Vec<(String, String, String)>) -> String {
    let mut output = String::new();
    for (path, original, modified) in changes {
        let diff = unified_diff(&path, &original, &modified);
        if !diff.is_empty() {
            output.push_str(&diff);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_diff_when_identical() {
        let result = unified_diff("test.rs", "hello\n", "hello\n");
        assert!(result.is_empty());
    }

    #[test]
    fn produces_unified_diff() {
        let result = unified_diff("test.rs", "aaa\nbbb\nccc\n", "aaa\nxxx\nccc\n");
        assert!(result.contains("--- a/test.rs"));
        assert!(result.contains("+++ b/test.rs"));
        assert!(result.contains("-bbb"));
        assert!(result.contains("+xxx"));
    }

    #[test]
    fn absolute_path_does_not_double_slash() {
        let result = unified_diff("/tmp/foo.rs", "old\n", "new\n");
        assert!(result.contains("--- a/tmp/foo.rs"), "got:\n{result}");
        assert!(!result.contains("a//tmp/foo.rs"), "got:\n{result}");
    }

    #[test]
    fn collect_multiple_diffs() {
        let changes = vec![
            ("a.rs".into(), "old\n".into(), "new\n".into()),
            ("b.rs".into(), "same\n".into(), "same\n".into()),
        ];
        let result = collect_diffs(changes);
        assert!(result.contains("a.rs"));
        assert!(!result.contains("b.rs"));
    }

    #[test]
    fn colored_diff_empty_when_identical() {
        let result = colored_unified_diff("test.rs", "hello\n", "hello\n");
        assert!(result.is_empty());
    }

    #[test]
    fn colored_diff_has_ansi_codes() {
        let result = colored_unified_diff("test.rs", "aaa\nbbb\nccc\n", "aaa\nxxx\nccc\n");
        assert!(
            result.contains("\x1b[1m--- a/test.rs"),
            "missing bold header"
        );
        assert!(result.contains("\x1b[36m@@ "), "missing cyan hunk header");
        assert!(
            result.contains("\x1b[41;97m-bbb"),
            "missing red deleted line"
        );
        assert!(
            result.contains("\x1b[42;30m+xxx"),
            "missing green added line"
        );
        assert!(result.contains("\x1b[2m aaa"), "missing dim context line");
    }
}
