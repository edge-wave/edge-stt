//! What travels between a client and the server. One place, shared by
//! both halves, so the two cannot drift apart.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{BackendKind, Language};
use crate::transcript::{Partial, PartialKind, Segment, Transcript};

/// The audio's shape, sent before the samples so the server can refuse
/// what it cannot take without reading any of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_type: String,
}

impl WireFormat {
    pub fn mono_16k() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            sample_type: "i16".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Start {
        request_id: String,
        format: WireFormat,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        want_partials: bool,
    },
    End {
        request_id: String,
    },
    Cancel {
        request_id: String,
    },
    /// Opens a continuous session: audio arrives as binary frames with
    /// no predetermined end, and the server decides utterance
    /// boundaries itself, sending `final` once per detected one.
    OpenStream {
        request_id: String,
        format: WireFormat,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        want_partials: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pause_tolerance_ms: Option<u64>,
    },
    /// Clean shutdown of a continuous session: finalizes whatever
    /// utterance was in progress before the connection may close.
    CloseStream {
        request_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSegment {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Accepted {
        request_id: String,
        queue_position: u32,
    },
    Partial {
        request_id: String,
        seq: u32,
        kind: String,
        text: String,
        start_ms: u64,
        end_ms: u64,
    },
    Final {
        request_id: String,
        text: String,
        language: String,
        confidence: f32,
        audio_duration_ms: u64,
        processing_time_ms: u64,
        segments: Vec<WireSegment>,
    },
    Cancelled {
        request_id: String,
    },
    Error {
        request_id: String,
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        queue_position: Option<u32>,
    },
}

impl ServerMessage {
    pub fn from_transcript(request_id: &str, transcript: &Transcript) -> Self {
        ServerMessage::Final {
            request_id: request_id.to_string(),
            text: transcript.text.clone(),
            language: transcript.language.to_string(),
            confidence: transcript.confidence,
            audio_duration_ms: transcript.audio_duration.as_millis() as u64,
            processing_time_ms: transcript.processing_time.as_millis() as u64,
            segments: transcript
                .segments
                .iter()
                .map(|s| WireSegment {
                    text: s.text.clone(),
                    start_ms: s.start.as_millis() as u64,
                    end_ms: s.end.as_millis() as u64,
                    confidence: s.confidence,
                })
                .collect(),
        }
    }

    pub fn from_partial(request_id: &str, partial: &Partial) -> Self {
        let segment = partial.segment.as_ref();
        ServerMessage::Partial {
            request_id: request_id.to_string(),
            seq: partial.seq,
            kind: match partial.kind {
                PartialKind::Append => "append".to_string(),
                PartialKind::Replace => "replace".to_string(),
            },
            text: partial.text.clone(),
            start_ms: segment.map_or(0, |s| s.start.as_millis() as u64),
            end_ms: segment.map_or(0, |s| s.end.as_millis() as u64),
        }
    }
}

/// Turn a final message back into the transcript the caller expects,
/// which is what makes the two backends interchangeable.
pub fn transcript_from(message: &ServerMessage) -> Option<Transcript> {
    let ServerMessage::Final {
        text,
        language,
        confidence,
        audio_duration_ms,
        processing_time_ms,
        segments,
        ..
    } = message
    else {
        return None;
    };
    Some(Transcript {
        text: text.clone(),
        segments: segments
            .iter()
            .map(|s| Segment {
                text: s.text.clone(),
                start: Duration::from_millis(s.start_ms),
                end: Duration::from_millis(s.end_ms),
                confidence: s.confidence,
            })
            .collect(),
        language: Language::new(language),
        confidence: *confidence,
        audio_duration: Duration::from_millis(*audio_duration_ms),
        processing_time: Duration::from_millis(*processing_time_ms),
        backend: BackendKind::Remote,
    })
}

pub fn partial_from(message: &ServerMessage) -> Option<Partial> {
    let ServerMessage::Partial {
        seq,
        kind,
        text,
        start_ms,
        end_ms,
        ..
    } = message
    else {
        return None;
    };
    Some(Partial {
        seq: *seq,
        kind: if kind == "replace" {
            PartialKind::Replace
        } else {
            PartialKind::Append
        },
        text: text.clone(),
        segment: Some(Segment {
            text: text.clone(),
            start: Duration::from_millis(*start_ms),
            end: Duration::from_millis(*end_ms),
            confidence: 1.0,
        }),
    })
}
