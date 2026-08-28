//! One variant per cause a caller has to tell apart. No variant
//! carries transcribed text, so an error can be logged freely.

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::config::AudioFormat;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("audio is {got}, and only {expected} can be transcribed")]
    UnsupportedAudio { expected: AudioFormat, got: AudioFormat },

    #[error("audio runs {got:?}, past the {limit:?} this transcriber accepts")]
    AudioTooLong { limit: Duration, got: Duration },

    #[error("no model file at {}", path.display())]
    ModelMissing { path: PathBuf },

    #[error("the model at {} cannot be used: {why}", path.display())]
    ModelUnusable { path: PathBuf, why: String },

    #[error("not enough {short} for the {size} model")]
    InsufficientResources { size: String, short: String },

    #[error("could not reach {endpoint}: {why}")]
    Network { endpoint: String, why: String },

    #[error("{endpoint} rejected the credential")]
    CredentialRejected { endpoint: String },

    #[error("{endpoint} is at capacity")]
    ServerAtCapacity {
        endpoint: String,
        queue_position: Option<u32>,
        retry_after: Option<Duration>,
    },

    #[error("{endpoint} failed: {why}")]
    ServerError { endpoint: String, why: String },

    #[error("gave up after {limit:?}")]
    Timeout { limit: Duration },

    #[error("cancelled")]
    Cancelled,

    #[error("{setting} is {got}, expected {expected}")]
    InvalidValue { setting: &'static str, expected: String, got: String },

    #[error("this build has no {backend} backend; enable the feature")]
    BackendUnavailable { backend: &'static str },
}

impl Error {
    /// Whether trying the same thing again could work. A dead host
    /// might come back; a wrong credential will not.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Network { .. } | Error::ServerAtCapacity { .. } | Error::Timeout { .. }
        )
    }
}
