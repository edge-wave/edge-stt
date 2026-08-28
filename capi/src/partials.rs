//! Words as they are decoded, handed to C.

use std::ffi::{CString, c_char, c_void};

use edge_stt_core::{Partial, PartialKind};

/// What a partial callback is handed. Everything in it is borrowed for
/// the length of the call.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct edge_stt_partial {
    /// Counts from zero, one per partial, so a gap is detectable.
    pub seq: u32,
    /// One when this replaces what came before, zero when it extends.
    pub replaces: i32,
    /// The new words. Never the whole transcript unless replaces is one.
    pub text: *const c_char,
    /// Where these words start, in milliseconds from the beginning.
    pub start_ms: u64,
    /// Where they end, in milliseconds from the beginning.
    pub end_ms: u64,
}

/// Called on the thread that called edge_stt_transcribe, never after
/// that call has returned.
#[allow(non_camel_case_types)]
pub type edge_stt_partial_cb =
    Option<unsafe extern "C" fn(partial: *const edge_stt_partial, user: *mut c_void)>;

/// Hand one partial to C, keeping the text alive for the call.
pub fn deliver(callback: edge_stt_partial_cb, user: usize, partial: &Partial) {
    let Some(callback) = callback else {
        return;
    };
    let Ok(text) = CString::new(partial.text.replace('\0', "")) else {
        return;
    };
    let handed = edge_stt_partial {
        seq: partial.seq,
        replaces: i32::from(partial.kind == PartialKind::Replace),
        text: text.as_ptr(),
        start_ms: partial
            .segment
            .as_ref()
            .map_or(0, |s| s.start.as_millis() as u64),
        end_ms: partial
            .segment
            .as_ref()
            .map_or(0, |s| s.end.as_millis() as u64),
    };
    unsafe { callback(&handed, user as *mut c_void) };
}
