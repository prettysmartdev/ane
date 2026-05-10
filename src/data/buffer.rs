use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Buffer {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub dirty: bool,
}

impl Buffer {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let lines: Vec<String> = content.lines().map(String::from).collect();
        Ok(Self {
            path: path.to_path_buf(),
            lines,
            dirty: false,
        })
    }

    pub fn empty(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            lines: vec![String::new()],
            dirty: false,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_line(&mut self, index: usize, text: String) {
        if index < self.lines.len() {
            self.lines[index] = text;
            self.dirty = true;
        }
    }

    pub fn insert_line(&mut self, index: usize, text: String) {
        let idx = index.min(self.lines.len());
        self.lines.insert(idx, text);
        self.dirty = true;
    }

    pub fn remove_line(&mut self, index: usize) -> Option<String> {
        if index < self.lines.len() && self.lines.len() > 1 {
            self.dirty = true;
            Some(self.lines.remove(index))
        } else {
            None
        }
    }

    pub fn replace_range(&mut self, start: usize, end: usize, replacement: Vec<String>) {
        let start = start.min(self.lines.len());
        let end = end.min(self.lines.len());
        if start <= end {
            self.lines.splice(start..end, replacement);
            self.dirty = true;
        }
    }

    pub fn write(&mut self) -> Result<()> {
        let content = self.content();
        std::fs::write(&self.path, content)
            .with_context(|| format!("writing {}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn round_trip_read_write() {
        let f = make_temp("hello\nworld");
        let mut buf = Buffer::from_file(f.path()).unwrap();
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.lines[0], "hello");

        buf.set_line(0, "goodbye".into());
        assert!(buf.dirty);

        buf.write().unwrap();
        let reloaded = Buffer::from_file(f.path()).unwrap();
        assert_eq!(reloaded.lines[0], "goodbye");
    }

    #[test]
    fn insert_and_remove_lines() {
        let mut buf = Buffer::empty(Path::new("/tmp/test"));
        buf.insert_line(0, "first".into());
        buf.insert_line(1, "second".into());
        assert_eq!(buf.line_count(), 3); // empty initial line + 2 inserted
        buf.remove_line(0);
        assert_eq!(buf.lines[0], "second");
    }

    #[test]
    fn replace_range() {
        let f = make_temp("a\nb\nc\nd");
        let mut buf = Buffer::from_file(f.path()).unwrap();
        buf.replace_range(1, 3, vec!["x".into(), "y".into(), "z".into()]);
        assert_eq!(buf.lines, vec!["a", "x", "y", "z", "d"]);
    }
}
