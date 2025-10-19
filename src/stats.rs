use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone)]
pub struct Stats {
    total_skipped: Arc<AtomicU32>,
    total_added: Arc<AtomicU32>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            total_skipped: Arc::new(AtomicU32::new(0)),
            total_added: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn inc_skipped(&self) {
        self.total_skipped.fetch_add(1, Ordering::SeqCst);
    }

    pub fn inc_added(&self) {
        self.total_added.fetch_add(1, Ordering::SeqCst);
    }

    pub fn skipped(&self) -> u32 {
        self.total_skipped.load(Ordering::SeqCst)
    }

    pub fn added(&self) -> u32 {
        self.total_added.load(Ordering::SeqCst)
    }

    pub fn print_summary(&self, total_in_db: u32) {
        println!();
        println!();
        println!("[+] Total skipped (already exists): {}", self.skipped());
        println!("[+] Total new added: {}", self.added());
        println!("[+] Total in database: {}", total_in_db);
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}
