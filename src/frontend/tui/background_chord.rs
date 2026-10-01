//! A bounded, cancellable chord worker. UI state is only mutated by `poll`.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};

use crate::commands::chord_engine::ChordEngine;
use crate::commands::chord_engine::types::{ChordAction, ChordQuery};
use crate::commands::lsp_engine::LspProvider;
use crate::data::buffer::Buffer;

pub(super) type ChordResult = Result<HashMap<String, ChordAction>, String>;
pub(super) struct Job {
    pub query: ChordQuery,
    pub buffers: HashMap<String, Buffer>,
    pub provider: Box<dyn LspProvider + Send>,
    pub cancel: Arc<AtomicBool>,
    pub reply: mpsc::SyncSender<ChordResult>,
}
#[derive(Default)]
struct Queue {
    job: Option<Job>,
    shutdown: bool,
}

pub(super) struct ChordWorker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
}
impl ChordWorker {
    pub fn new() -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let worker = Arc::clone(&queue);
        std::thread::spawn(move || {
            loop {
                let (lock, cv) = &*worker;
                let mut state = lock.lock().unwrap();
                while state.job.is_none() && !state.shutdown {
                    state = cv.wait(state).unwrap();
                }
                if state.shutdown {
                    break;
                }
                let mut job = state.job.take().unwrap();
                drop(state);
                if job.cancel.load(Ordering::Acquire) {
                    continue;
                }
                let _timing = crate::commands::diagnostics::Timing::new("chord_resolution");
                let result = ChordEngine::resolve(&job.query, &job.buffers, &mut *job.provider)
                    .map_err(|error| format!("resolve error: {error}"))
                    .and_then(|resolved| {
                        ChordEngine::patch(&resolved, &job.buffers)
                            .map_err(|error| format!("patch error: {error}"))
                    });
                if !job.cancel.load(Ordering::Acquire) {
                    let _ = job.reply.try_send(result);
                }
            }
        });
        Self { queue }
    }
    pub fn submit(&self, job: Job) {
        let (lock, cv) = &*self.queue;
        let mut queue = lock.lock().unwrap();
        if let Some(old) = queue.job.replace(job) {
            old.cancel.store(true, Ordering::Release);
        }
        cv.notify_one();
    }
}
impl Drop for ChordWorker {
    fn drop(&mut self) {
        let (lock, cv) = &*self.queue;
        let mut queue = lock.lock().unwrap();
        queue.shutdown = true;
        if let Some(job) = queue.job.take() {
            job.cancel.store(true, Ordering::Release);
        }
        cv.notify_one();
    }
}
