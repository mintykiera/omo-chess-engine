use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

pub(crate) fn get_nnue_path() -> std::path::PathBuf {
    if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        path.push("omo.nnue");
        if path.exists() {
            return path;
        }
    }
    std::path::PathBuf::from("omo.nnue")
}

pub(crate) fn get_memory_path() -> PathBuf {
    if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        path.push("omo_memory.bin");
        path
    } else {
        PathBuf::from("omo_memory.bin")
    }
}

pub(crate) fn get_book_path() -> PathBuf {
    if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        path.push("book.bin");
        path
    } else {
        PathBuf::from("book.bin")
    }
}

pub(crate) struct SearchHandle {
    pub threads: Vec<thread::JoinHandle<()>>,
    pub stop_flag: Arc<AtomicBool>,
    pub is_pondering: Arc<AtomicBool>,
    pub time_limit_ms: Arc<AtomicU64>,
}

impl SearchHandle {
    pub fn new() -> Self {
        Self {
            threads: Vec::new(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            is_pondering: Arc::new(AtomicBool::new(false)),
            time_limit_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn stop_and_join(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }

    pub fn is_searching(&self) -> bool {
        self.threads.iter().any(|h| !h.is_finished())
    }
}
