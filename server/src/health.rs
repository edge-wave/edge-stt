//! Alive, and separately, able to transcribe. Loading a large model
//! takes long enough that one answer for both would get a working
//! server killed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;

#[derive(Clone, Default)]
pub struct Readiness(Arc<AtomicBool>);

impl Readiness {
    pub fn mark_ready(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub async fn alive() -> StatusCode {
    StatusCode::OK
}

pub async fn ready(readiness: Readiness) -> StatusCode {
    if readiness.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
