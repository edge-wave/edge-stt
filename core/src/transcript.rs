//! What was said, and enough about how it was produced to act on it.

use std::time::Duration;

use crate::config::{BackendKind, Language};

/// One stretch of speech, timed from the start of the utterance.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    pub start: Duration,
    pub end: Duration,
    pub confidence: f32,
}

/// A finished transcript. `text` is exactly the segments joined, so a
/// caller can use either without the two disagreeing.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
    pub language: Language,
    pub confidence: f32,
    pub audio_duration: Duration,
    pub processing_time: Duration,
    pub backend: BackendKind,
}

impl Transcript {
    /// Silence is a transcript with nothing in it, never an error.
    pub fn empty(
        language: Language,
        audio_duration: Duration,
        processing_time: Duration,
        backend: BackendKind,
    ) -> Self {
        Self {
            text: String::new(),
            segments: Vec::new(),
            language,
            confidence: 0.0,
            audio_duration,
            processing_time,
            backend,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// How the decoding time compares to the audio. Under one means the
    /// hardware is keeping up with someone speaking.
    pub fn real_time_factor(&self) -> f32 {
        let audio = self.audio_duration.as_secs_f32();
        if audio <= 0.0 {
            return 0.0;
        }
        self.processing_time.as_secs_f32() / audio
    }
}

/// Whether a partial adds to what came before or replaces it. Whisper
/// only ever appends; the other variant is for a runtime that revises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialKind {
    Append,
    Replace,
}

/// An interim view, delivered while decoding continues. Never final,
/// and the type says so rather than the caller having to remember.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    pub seq: u32,
    pub kind: PartialKind,
    pub text: String,
    pub segment: Option<Segment>,
}

impl Partial {
    pub fn append(seq: u32, text: impl Into<String>, segment: Option<Segment>) -> Self {
        Self {
            seq,
            kind: PartialKind::Append,
            text: text.into(),
            segment,
        }
    }
}
