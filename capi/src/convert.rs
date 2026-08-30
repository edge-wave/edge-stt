//! Moving values across the boundary without losing what they said.

use std::ffi::{CStr, CString, c_char};

use edge_stt_core::Transcript;

use crate::error::{edge_stt_error, fail_with};

/// A borrowed C string, or a reason it could not be read.
pub fn required_str<'a>(value: *const c_char, name: &str) -> Result<&'a str, i32> {
    if value.is_null() {
        return Err(fail_with(
            edge_stt_error::EDGE_STT_NULL_ARGUMENT,
            &format!("{name} must not be null"),
        ));
    }
    unsafe { CStr::from_ptr(value) }.to_str().map_err(|_| {
        fail_with(
            edge_stt_error::EDGE_STT_INVALID_VALUE,
            &format!("{name} must be UTF-8"),
        )
    })
}

/// Allocate an owned value behind a caller's out pointer.
///
/// # Safety
/// `slot` must be valid for writes for the length of the call.
pub unsafe fn out_box<T>(slot: *mut *mut T, value: T, name: &str) -> Result<(), i32> {
    if slot.is_null() {
        return Err(fail_with(
            edge_stt_error::EDGE_STT_NULL_ARGUMENT,
            &format!("{name} must not be null"),
        ));
    }
    unsafe { *slot = Box::into_raw(Box::new(value)) };
    Ok(())
}

/// What a transcript handle points to. Opaque on the C side, which
/// only ever names the pointer to this: `edge_stt_transcript_h`. Owns
/// its strings so the caller never has to free one separately.
#[allow(non_camel_case_types)]
pub struct edge_stt_transcript_handle {
    pub(crate) text: CString,
    pub(crate) language: CString,
    pub(crate) confidence: f32,
    pub(crate) audio_duration_ms: u64,
    pub(crate) processing_time_ms: u64,
    pub(crate) segments: Vec<Segment>,
}

/// The handle a C caller holds for one transcript.
#[allow(non_camel_case_types)]
pub type edge_stt_transcript_h = *mut edge_stt_transcript_handle;

pub struct Segment {
    pub(crate) text: CString,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    pub(crate) confidence: f32,
}

impl edge_stt_transcript_handle {
    pub fn from_core(transcript: &Transcript) -> Self {
        Self {
            text: cstring(&transcript.text),
            language: cstring(transcript.language.as_str()),
            confidence: transcript.confidence,
            audio_duration_ms: transcript.audio_duration.as_millis() as u64,
            processing_time_ms: transcript.processing_time.as_millis() as u64,
            segments: transcript
                .segments
                .iter()
                .map(|s| Segment {
                    text: cstring(&s.text),
                    start_ms: s.start.as_millis() as u64,
                    end_ms: s.end.as_millis() as u64,
                    confidence: s.confidence,
                })
                .collect(),
        }
    }
}

/// A NUL inside the text would truncate it silently, so it is dropped.
fn cstring(value: &str) -> CString {
    CString::new(value.replace('\0', "")).unwrap_or_default()
}
