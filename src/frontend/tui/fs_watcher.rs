use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};

use crate::commands::diagnostics::Timing;
use crate::data::file_tree::FileTree;
use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const EVENT_CAPACITY: usize = 2048;
enum Command {
    File(Option<PathBuf>),
    Tree(Option<PathBuf>, u64),
    Scan(PathBuf, u64),
}
#[derive(Default)]
struct Desired {
    file: Option<Option<PathBuf>>,
    tree: Option<(Option<PathBuf>, u64)>,
    scan: Option<(PathBuf, u64)>,
    shutdown: bool,
}
struct Commands {
    state: Mutex<Desired>,
    changed: Condvar,
}
impl Commands {
    fn submit(&self, command: Command) {
        let mut state = self.state.lock().unwrap();
        match command {
            Command::File(path) => state.file = Some(path),
            Command::Tree(path, generation) => {
                state.tree = Some((path, generation));
                state.scan = None;
            }
            Command::Scan(root, generation) => state.scan = Some((root, generation)),
        }
        self.changed.notify_one();
    }
    fn take(&self) -> Option<Command> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.shutdown {
                return None;
            }
            if let Some(file) = state.file.take() {
                return Some(Command::File(file));
            }
            if let Some((tree, generation)) = state.tree.take() {
                return Some(Command::Tree(tree, generation));
            }
            if let Some((root, generation)) = state.scan.take() {
                return Some(Command::Scan(root, generation));
            }
            state = self.changed.wait(state).unwrap();
        }
    }
}
fn update_path(event: &notify::Result<notify::Event>) -> Option<&PathBuf> {
    let event = event.as_ref().ok()?;
    if event.paths.len() == 1
        && matches!(
            event.kind,
            EventKind::Modify(
                notify::event::ModifyKind::Data(_)
                    | notify::event::ModifyKind::Metadata(_)
                    | notify::event::ModifyKind::Any
            )
        )
    {
        event.paths.first()
    } else {
        None
    }
}
type TreeResult = Option<(u64, Result<FileTree>)>;

pub struct FsWatcher {
    commands: Arc<Commands>,
    coalesced: Arc<Mutex<HashSet<PathBuf>>>,
    pub rx: mpsc::Receiver<notify::Result<notify::Event>>,
    overflow: Arc<AtomicBool>,
    tree_result: Arc<Mutex<TreeResult>>,
    watched_file: Option<PathBuf>,
    watched_tree: Option<PathBuf>,
    generation: u64,
    scanning: bool,
}

impl FsWatcher {
    pub fn new() -> Result<Self> {
        let (events, rx) = mpsc::sync_channel(EVENT_CAPACITY);
        let commands = Arc::new(Commands {
            state: Mutex::new(Desired::default()),
            changed: Condvar::new(),
        });
        let command_rx = Arc::clone(&commands);
        let coalesced = Arc::new(Mutex::new(HashSet::new()));
        let pending_updates = Arc::clone(&coalesced);
        let overflow = Arc::new(AtomicBool::new(false));
        let tree_result = Arc::new(Mutex::new(None));
        let worker_overflow = Arc::clone(&overflow);
        let worker_result = Arc::clone(&tree_result);
        std::thread::spawn(move || {
            let changes = Arc::new(AtomicU64::new(0));
            let observed = Arc::clone(&changes);
            let event_overflow = Arc::clone(&worker_overflow);
            let notices = events.clone();
            let mut watcher = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    if let Ok(event) = &event {
                        if event
                            .paths
                            .iter()
                            .any(|path| crate::commands::diagnostics::is_timing_path(path))
                        {
                            return;
                        }
                        if matches!(event.kind, EventKind::Access(_)) {
                            return;
                        }
                        if matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Remove(_)
                                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                        ) {
                            observed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    if let Some(path) = update_path(&event)
                        && !pending_updates.lock().unwrap().insert(path.clone())
                    {
                        return;
                    }
                    if let Err(error) = events.try_send(event) {
                        let event = match error {
                            mpsc::TrySendError::Full(event)
                            | mpsc::TrySendError::Disconnected(event) => event,
                        };
                        if let Some(path) = update_path(&event) {
                            pending_updates.lock().unwrap().remove(path);
                        }
                        event_overflow.store(true, Ordering::Release);
                    }
                },
                notify::Config::default(),
            )
            .ok();
            let mut file: Option<PathBuf> = None;
            let mut tree: Option<PathBuf> = None;
            while let Some(command) = command_rx.take() {
                match command {
                    Command::File(path) => {
                        if let Some(watcher) = watcher.as_mut() {
                            if let Some(old) = file.take() {
                                let _ = watcher.unwatch(&old);
                            }
                            if let Some(path) = path {
                                let canonical = path.canonicalize().unwrap_or(path);
                                if watcher
                                    .watch(&canonical, RecursiveMode::NonRecursive)
                                    .is_ok()
                                {
                                    file = Some(canonical.clone());
                                }
                                // Close the gap between recording the buffer's
                                // mtime and installing its asynchronous watch.
                                let kind = if canonical.exists() {
                                    EventKind::Modify(notify::event::ModifyKind::Any)
                                } else {
                                    EventKind::Remove(notify::event::RemoveKind::Any)
                                };
                                if notices
                                    .try_send(Ok(notify::Event::new(kind).add_path(canonical)))
                                    .is_err()
                                {
                                    worker_overflow.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                    Command::Tree(path, generation) => {
                        if let Some(watcher) = watcher.as_mut()
                            && let Some(old) = tree.take()
                        {
                            let _ = watcher.unwatch(&old);
                        }
                        if let Some(root) = path {
                            let root = root.canonicalize().unwrap_or(root);
                            {
                                let _timing = Timing::new("tree_watch_setup");
                                if let Some(watcher) = watcher.as_mut() {
                                    let _ = watcher.watch(&root, RecursiveMode::Recursive);
                                }
                            }
                            tree = Some(root.clone());
                            scan_tree(
                                &root,
                                generation,
                                &changes,
                                &worker_overflow,
                                &worker_result,
                            );
                        }
                    }
                    Command::Scan(root, generation) => {
                        scan_tree(
                            &root,
                            generation,
                            &changes,
                            &worker_overflow,
                            &worker_result,
                        );
                    }
                }
            }
            // Backend teardown may traverse thousands of watches; it stays here.
        });
        Ok(Self {
            commands,
            coalesced,
            rx,
            overflow,
            tree_result,
            watched_file: None,
            watched_tree: None,
            generation: 0,
            scanning: false,
        })
    }

    fn enqueue(&self, command: Command) -> Result<()> {
        self.commands.submit(command);
        Ok(())
    }
    pub fn next_event(&self) -> Option<notify::Result<notify::Event>> {
        let event = self.rx.try_recv().ok()?;
        if let Some(path) = update_path(&event) {
            self.coalesced.lock().unwrap().remove(path);
        }
        Some(event)
    }
    pub fn watch_file(&mut self, path: &Path) -> Result<()> {
        self.enqueue(Command::File(Some(path.to_path_buf())))?;
        self.watched_file = Some(path.to_path_buf());
        Ok(())
    }
    pub fn unwatch_file(&mut self) {
        let _ = self.enqueue(Command::File(None));
        self.watched_file = None;
    }
    pub fn watch_tree(&mut self, root: &Path) -> Result<()> {
        self.generation = self.generation.wrapping_add(1);
        self.enqueue(Command::Tree(Some(root.to_path_buf()), self.generation))?;
        self.watched_tree = Some(root.to_path_buf());
        self.scanning = true;
        Ok(())
    }
    pub fn unwatch_tree(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let _ = self.enqueue(Command::Tree(None, self.generation));
        self.watched_tree = None;
        self.scanning = false;
    }
    pub fn watched_tree(&self) -> Option<&Path> {
        self.watched_tree.as_deref()
    }
    pub fn watched_file(&self) -> Option<&Path> {
        self.watched_file.as_deref()
    }
    pub fn take_tree(&mut self) -> Option<Result<FileTree>> {
        let (generation, result) = self.tree_result.lock().unwrap().take()?;
        if generation != self.generation {
            return None;
        }
        self.scanning = false;
        Some(result)
    }
    pub fn take_overflow(&self) -> bool {
        let overflow = self.overflow.swap(false, Ordering::AcqRel);
        if overflow {
            self.coalesced.lock().unwrap().clear();
            for _ in 0..EVENT_CAPACITY {
                if self.rx.try_recv().is_err() {
                    break;
                }
            }
        }
        overflow
    }
    pub fn reconcile_tree(&mut self) -> Result<()> {
        if !self.scanning
            && let Some(root) = &self.watched_tree
        {
            self.enqueue(Command::Scan(root.clone(), self.generation))?;
            self.scanning = true;
        } else if self.scanning {
            self.overflow.store(true, Ordering::Release);
        }
        Ok(())
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        self.commands.state.lock().unwrap().shutdown = true;
        self.commands.changed.notify_one();
    }
}

fn scan_tree(
    root: &Path,
    generation: u64,
    changes: &AtomicU64,
    overflow: &AtomicBool,
    result: &Mutex<TreeResult>,
) {
    let mut timing = Timing::new("tree_scan");
    let before = changes.load(Ordering::Acquire);
    let snapshot = FileTree::from_dir(root);
    timing.set_count(snapshot.as_ref().map_or(0, |tree| tree.entries.len()));
    if changes.load(Ordering::Acquire) != before {
        overflow.store(true, Ordering::Release);
    }
    *result.lock().unwrap() = Some((generation, snapshot));
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
    fn await_snapshot(watcher: &mut FsWatcher) -> crate::data::file_tree::FileTree {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = watcher.take_tree() {
                return result.unwrap();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "background scan did not finish"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn watch_scan_filters_access_events_and_overflow_reconciles_final_tree() {
        let root = tempfile::tempdir().unwrap();
        let mut watcher = FsWatcher::new().unwrap();
        watcher.watch_tree(root.path()).unwrap();
        assert!(await_snapshot(&mut watcher).entries.is_empty());
        for i in 0..3000 {
            std::fs::write(root.path().join(format!("{i}.txt")), "x").unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !watcher.overflow.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            watcher.take_overflow(),
            "bounded event queue must report overflow"
        );
        watcher.reconcile_tree().unwrap();
        let snapshot = await_snapshot(&mut watcher);
        assert_eq!(snapshot.files_only().count(), 3000);
        while let Some(Ok(event)) = watcher.next_event() {
            assert!(!matches!(event.kind, EventKind::Access(_)));
        }
    }

    #[test]
    fn watch_commands_coalesce_to_latest_desired_paths() {
        let commands = Commands {
            state: Mutex::new(Desired::default()),
            changed: Condvar::new(),
        };
        for i in 0..10_000 {
            commands.submit(Command::File(Some(PathBuf::from(i.to_string()))));
        }
        assert!(
            matches!(commands.take(), Some(Command::File(Some(path))) if path == Path::new("9999"))
        );
        commands.submit(Command::Scan(PathBuf::from("old"), 1));
        commands.submit(Command::Tree(Some(PathBuf::from("new")), 2));
        assert!(commands.state.lock().unwrap().scan.is_none());
        assert!(
            matches!(commands.take(), Some(Command::Tree(Some(path), 2)) if path == Path::new("new"))
        );
    }
}
