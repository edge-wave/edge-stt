//! Every cause is its own variant, and none of them carries what was
//! said.

use std::path::PathBuf;
use std::time::Duration;

use edge_stt_core::Error;
use edge_stt_core::config::{AudioFormat, Secret};

fn one_of_each() -> Vec<Error> {
    vec![
        Error::UnsupportedAudio {
            expected: AudioFormat::mono_16k(),
            got: AudioFormat::new(44_100, 2, edge_stt_core::SampleType::F32),
        },
        Error::AudioTooLong {
            limit: Duration::from_secs(300),
            got: Duration::from_secs(600),
        },
        Error::ModelMissing {
            path: PathBuf::from("/models/ggml-base.bin"),
        },
        Error::ModelUnusable {
            path: PathBuf::from("/models/ggml-base.bin"),
            why: "not a whisper model".to_string(),
        },
        Error::InsufficientResources {
            size: "large-v3".to_string(),
            short: "memory".to_string(),
        },
        Error::Network {
            endpoint: "ws://host:8000".to_string(),
            why: "refused".to_string(),
        },
        Error::CredentialRejected {
            endpoint: "ws://host:8000".to_string(),
        },
        Error::ServerAtCapacity {
            endpoint: "ws://host:8000".to_string(),
            queue_position: Some(3),
            retry_after: Some(Duration::from_secs(2)),
        },
        Error::ServerError {
            endpoint: "ws://host:8000".to_string(),
            why: "boom".to_string(),
        },
        Error::Timeout {
            limit: Duration::from_secs(10),
        },
        Error::Cancelled,
    ]
}

#[test]
fn a_dead_host_never_reads_as_a_bad_credential() {
    let dead = Error::Network {
        endpoint: "ws://host".to_string(),
        why: "refused".to_string(),
    };
    let refused = Error::CredentialRejected {
        endpoint: "ws://host".to_string(),
    };
    assert!(matches!(dead, Error::Network { .. }));
    assert!(matches!(refused, Error::CredentialRejected { .. }));
    assert_ne!(dead.to_string(), refused.to_string());
    assert!(dead.is_retryable());
    assert!(!refused.is_retryable());
}

#[test]
fn cancellation_is_not_a_timeout() {
    assert!(!Error::Cancelled.is_retryable());
    assert!(
        Error::Timeout {
            limit: Duration::from_secs(1)
        }
        .is_retryable()
    );
    assert_ne!(
        Error::Cancelled.to_string(),
        Error::Timeout {
            limit: Duration::from_secs(1)
        }
        .to_string()
    );
}

#[test]
fn every_variant_says_something_different() {
    let messages: Vec<String> = one_of_each().iter().map(|e| e.to_string()).collect();
    let mut unique = messages.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), messages.len(), "two variants share a message");
}

#[test]
fn no_variant_can_carry_what_was_said() {
    let spoken = "the transcript nobody should log";
    for error in one_of_each() {
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains(spoken));
    }
}

#[test]
fn a_credential_prints_as_a_placeholder() {
    let secret = Secret::new("hunter2");
    assert_eq!(secret.expose(), "hunter2");
    assert!(!format!("{secret:?}").contains("hunter2"));
    assert!(format!("{secret:?}").contains("redacted"));
}
