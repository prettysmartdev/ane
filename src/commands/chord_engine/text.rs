use crate::data::buffer::Buffer;

use super::types::TextRange;

pub fn line_char_count(line: &str) -> usize {
    line.chars().count()
}

pub fn char_to_byte(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map(|(i, _)| i)
        .unwrap_or(line.len())
}

pub fn slice_chars(line: &str, start: usize, end: usize) -> &str {
    let start = start.min(line_char_count(line));
    let end = end.min(line_char_count(line)).max(start);
    let start_byte = char_to_byte(line, start);
    let end_byte = char_to_byte(line, end);
    &line[start_byte..end_byte]
}

pub fn split_line_at_char(line: &str, col: usize) -> (&str, &str) {
    let col = col.min(line_char_count(line));
    line.split_at(char_to_byte(line, col))
}

pub fn extract_range_text(buffer: &Buffer, range: &TextRange) -> String {
    if buffer.lines.is_empty() {
        return String::new();
    }

    let last = buffer.line_count().saturating_sub(1);
    let start_line = range.start_line.min(last);
    let end_line = range.end_line.min(last);

    if start_line == end_line {
        return slice_chars(&buffer.lines[start_line], range.start_col, range.end_col).to_string();
    }

    let mut result = String::new();
    for line_idx in start_line..=end_line {
        let line = &buffer.lines[line_idx];
        if line_idx == start_line {
            let from = range.start_col.min(line_char_count(line));
            result.push_str(slice_chars(line, from, line_char_count(line)));
        } else if line_idx == end_line {
            result.push('\n');
            result.push_str(slice_chars(line, 0, range.end_col));
        } else {
            result.push('\n');
            result.push_str(line);
        }
    }
    result
}

/// Splice `replacement` into `lines` at `range` (character-indexed). Returns
/// the resulting Vec<String>, splitting on '\n' (preserves trailing empty
/// lines, unlike str::lines).
fn splice_lines(lines: &[String], range: &TextRange, replacement: &str) -> Vec<String> {
    if lines.is_empty() {
        let mut out: Vec<String> = replacement.split('\n').map(String::from).collect();
        if out.is_empty() {
            out.push(String::new());
        }
        return out;
    }

    let mut lines = lines.to_vec();
    let last = lines.len().saturating_sub(1);
    let start_line = range.start_line.min(last);
    let end_line = range.end_line.min(last);

    let (prefix, _) = split_line_at_char(&lines[start_line], range.start_col);
    let prefix = prefix.to_string();
    let (_, suffix) = split_line_at_char(&lines[end_line], range.end_col);
    let suffix = suffix.to_string();

    let new_content = format!("{prefix}{replacement}{suffix}");
    let mut new_lines: Vec<String> = new_content.split('\n').map(String::from).collect();
    if new_lines.is_empty() {
        new_lines.push(String::new());
    }

    lines.splice(start_line..=end_line, new_lines);
    lines
}

fn lines_to_content(lines: &[String], buffer: &Buffer) -> String {
    let mut s = lines.join("\n");
    if buffer.trailing_newline {
        s.push('\n');
    }
    s
}

/// Apply a single text replacement at `range`, splitting on '\n' (preserving
/// trailing empty lines, unlike str::lines). The result reproduces the
/// buffer's trailing-newline behavior.
pub fn apply_single_replacement(buffer: &Buffer, range: &TextRange, replacement: &str) -> String {
    let new_lines = splice_lines(&buffer.lines, range, replacement);
    lines_to_content(&new_lines, buffer)
}

/// Apply many replacements (range, replacement) to the buffer. Ranges must be
/// non-overlapping; they are applied in reverse document order so earlier
/// offsets remain valid as later edits are made.
pub fn apply_replacements(buffer: &Buffer, edits: &[(TextRange, String)]) -> String {
    if edits.is_empty() {
        return buffer.content();
    }
    let mut sorted = edits.to_vec();
    sorted.sort_by(|a, b| {
        b.0.start_line
            .cmp(&a.0.start_line)
            .then(b.0.start_col.cmp(&a.0.start_col))
    });
    let mut lines = buffer.lines.clone();
    for (range, replacement) in &sorted {
        lines = splice_lines(&lines, range, replacement);
    }
    lines_to_content(&lines, buffer)
}

pub fn apply_insertion(buffer: &Buffer, line: usize, col: usize, insertion: &str) -> String {
    let point = TextRange::point(line, col);
    apply_single_replacement(buffer, &point, insertion)
}
