use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct FsWatcher {
    watcher: RecommendedWatcher,
    pub rx: mpsc::Receiver<notify::Result<notify::Event>>,
    watched_file: Option<PathBuf>,
    watched_tree: Option<PathBuf>,
}

impl FsWatcher {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
        Ok(Self {
            watcher,
            rx,
            watched_file: None,
            watched_tree: None,
        })
    }

    pub fn watch_file(&mut self, path: &Path) -> Result<()> {
        self.unwatch_file();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.watcher
            .watch(&canonical, RecursiveMode::NonRecursive)?;
        self.watched_file = Some(canonical);
        Ok(())
    }

    pub fn unwatch_file(&mut self) {
        if let Some(path) = self.watched_file.take() {
            let _ = self.watcher.unwatch(&path);
        }
    }

    pub fn watch_tree(&mut self, root: &Path) -> Result<()> {
        self.unwatch_tree();
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.watcher.watch(&canonical, RecursiveMode::Recursive)?;
        self.watched_tree = Some(canonical);
        Ok(())
    }

    pub fn unwatch_tree(&mut self) {
        if let Some(path) = self.watched_tree.take() {
            let _ = self.watcher.unwatch(&path);
        }
    }

    pub fn watched_file(&self) -> Option<&Path> {
        self.watched_file.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[test]
    fn watch_file_unwatch_file_round_trip_receives_event() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"initial\n").unwrap();
        f.flush().unwrap();

        let mut watcher = FsWatcher::new().unwrap();
        watcher.watch_file(f.path()).unwrap();

        let expected_canonical = f.path().canonicalize().unwrap();
        assert_eq!(
            watcher.watched_file().map(|p| p.to_path_buf()),
            Some(expected_canonical),
            "watched_file should be set to the canonical path after watch_file"
        );

        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(f.path(), b"modified\n").unwrap();

        let result = watcher.rx.recv_timeout(Duration::from_secs(1));
        assert!(
            result.is_ok(),
            "should receive an FS event within 1 second after writing to watched file"
        );

        watcher.unwatch_file();
        assert!(
            watcher.watched_file().is_none(),
            "watched_file should be None after unwatch_file"
        );
    }
}
