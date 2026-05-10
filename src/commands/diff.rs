use similar::TextDiff;

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
}
