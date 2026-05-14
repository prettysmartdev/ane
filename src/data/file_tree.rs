use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
}

impl FileEntry {
    pub fn name(&self) -> &str {
        self.path.file_name().and_then(|n| n.to_str()).unwrap_or("")
    }
}

#[derive(Debug, Clone)]
pub struct FileTree {
    pub root: PathBuf,
    pub entries: Vec<FileEntry>,
}

impl FileTree {
    pub fn from_dir(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let mut entries = Vec::new();

        for entry in WalkDir::new(&root)
            .sort_by_file_name()
            .min_depth(1)
        {
            let entry = entry?;
            let depth = entry.depth() - 1;
            entries.push(FileEntry {
                path: entry.path().to_path_buf(),
                depth,
                is_dir: entry.file_type().is_dir(),
            });
        }

        Ok(Self { root, entries })
    }

    pub fn files_only(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn scan_directory() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("file.txt"), "content").unwrap();
        fs::write(tmp.path().join("sub/nested.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join(".hidden"), "secret").unwrap();

        let tree = FileTree::from_dir(tmp.path()).unwrap();
        let names: Vec<&str> = tree.entries.iter().map(|e| e.name()).collect();
        assert!(names.contains(&"file.txt"));
        assert!(names.contains(&"sub"));
        assert!(names.contains(&"nested.rs"));
        assert!(names.contains(&".hidden"));
    }
}
