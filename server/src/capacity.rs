//! How many utterances this machine will decode at once, and what a
//! client is told when it asks for one too many.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How long a queued client waits before being told to come back.
const PATIENCE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct Capacity {
    permits: Arc<Semaphore>,
    limit: usize,
}

pub enum Admission {
    Started(OwnedSemaphorePermit),
    Queued(OwnedSemaphorePermit, u32),
    Full,
}

impl Capacity {
    pub fn new(limit: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(limit.max(1))),
            limit: limit.max(1),
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Never leaves a client without an answer: it starts, or it is
    /// told where it stands, or it is told to come back.
    pub async fn admit(&self) -> Admission {
        if let Ok(permit) = self.permits.clone().try_acquire_owned() {
            return Admission::Started(permit);
        }
        let waiting = self.limit as u32;
        match tokio::time::timeout(PATIENCE, self.permits.clone().acquire_owned()).await {
            Ok(Ok(permit)) => Admission::Queued(permit, waiting),
            _ => Admission::Full,
        }
    }

    pub fn retry_after_ms() -> u64 {
        PATIENCE.as_millis() as u64
    }
}
