//! Opt-in numeric timings. No paths or file contents are recorded.
use std::io::Write;
use std::sync::{OnceLock, mpsc};
use std::time::Instant;

type Sample = (&'static str, u128, usize);
static SINK: OnceLock<Option<mpsc::SyncSender<Sample>>> = OnceLock::new();
static TIMING_NAME: OnceLock<String> = OnceLock::new();

pub struct Timing {
    operation: &'static str,
    start: Option<Instant>,
    count: usize,
}

impl Timing {
    pub fn new(operation: &'static str) -> Self {
        let sink = SINK.get_or_init(|| {
            std::env::var_os("ANE_TIMINGS")?;
            // Always outside the working tree; writes are handled off-thread.
            let path =
                std::env::temp_dir().join(format!("ane-timings-{}.jsonl", std::process::id()));
            let (tx, rx) = mpsc::sync_channel::<Sample>(1024);
            std::thread::spawn(move || {
                let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return; };
                let mut file = std::io::BufWriter::new(file);
                let _ = writeln!(file, "{}", serde_json::json!({"profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                    "os": std::env::consts::OS, "arch": std::env::consts::ARCH}));
                while let Ok((op, micros, count)) = rx.recv() {
                    let _ = writeln!(file, "{{\"operation\":\"{op}\",\"micros\":{micros},\"count\":{count}}}");
                    let _ = file.flush();
                }
            });
            Some(tx)
        });
        Self {
            operation,
            start: sink.as_ref().map(|_| Instant::now()),
            count: 0,
        }
    }
    pub fn new_count(operation: &'static str, count: usize) -> Self {
        let mut timing = Self::new(operation);
        timing.count = count;
        timing
    }
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
    }
}

/// Prevent self-generated diagnostics events if the watched root includes TMPDIR.
pub fn is_timing_path(path: &std::path::Path) -> bool {
    path.file_name().is_some_and(|name| {
        name == TIMING_NAME
            .get_or_init(|| format!("ane-timings-{}.jsonl", std::process::id()))
            .as_str()
    })
}

impl Drop for Timing {
    fn drop(&mut self) {
        if let Some(start) = self.start
            && let Some(Some(sink)) = SINK.get()
        {
            let _ = sink.try_send((self.operation, start.elapsed().as_micros(), self.count));
        }
    }
}
