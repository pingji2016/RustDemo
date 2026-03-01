use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct AppState {
    pub start: Instant,
    pub hits_root: AtomicU64,
    pub hits_health: AtomicU64,
    pub hits_sum: AtomicU64,
    pub hits_echo: AtomicU64,
    pub hits_parallel: AtomicU64,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            hits_root: AtomicU64::new(0),
            hits_health: AtomicU64::new(0),
            hits_sum: AtomicU64::new(0),
            hits_echo: AtomicU64::new(0),
            hits_parallel: AtomicU64::new(0),
        }
    }

    pub fn uptime(&self) -> u64 {
        self.start.elapsed().as_secs()
    }
}

#[derive(Serialize)]
pub struct MetricsResponse {
    pub uptime_seconds: u64,
    pub hits: Hits,
}

#[derive(Serialize)]
pub struct Hits {
    pub root: u64,
    pub health: u64,
    pub sum: u64,
    pub echo: u64,
    pub parallel: u64,
}

impl From<&AppState> for MetricsResponse {
    fn from(state: &AppState) -> Self {
        Self {
            uptime_seconds: state.uptime(),
            hits: Hits {
                root: state.hits_root.load(Ordering::Relaxed),
                health: state.hits_health.load(Ordering::Relaxed),
                sum: state.hits_sum.load(Ordering::Relaxed),
                echo: state.hits_echo.load(Ordering::Relaxed),
                parallel: state.hits_parallel.load(Ordering::Relaxed),
            },
        }
    }
}
