//! C API for edge-stt. Binds the core crate directly, adding nothing.
//! One promise covers every call: handles come from `edge_stt_new`
//! unfreed, pointers are good for the call, strings are terminated.
#![allow(clippy::missing_safety_doc)]

mod convert;
mod error;
mod partials;

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::Duration;

use edge_stt_core::{CancelToken, Config, EdgeStt, Language, ModelSpec, Utterance};

use convert::{edge_stt_transcript_handle, out_box, required_str};
use error::*;
use partials::{deliver, edge_stt_partial_cb};

pub use convert::edge_stt_transcript_h;
pub use convert::edge_stt_transcript_handle as edge_stt_transcript_t;
pub use error::edge_stt_error;
pub use partials::{edge_stt_partial, edge_stt_partial_cb as edge_stt_partial_callback};

/// What a handle points to. Opaque on the C side, which only ever
/// names the pointer to this: `edge_stt_h`.
#[allow(non_camel_case_types)]
pub struct edge_stt_handle {
    core: Mutex<Option<EdgeStt>>,
    settings: Mutex<Settings>,
    cancel: CancelToken,
}

/// The handle a C caller holds.
#[allow(non_camel_case_types)]
pub type edge_stt_h = *mut edge_stt_handle;

#[derive(Default)]
struct Settings {
    language: Option<String>,
    timeout: Option<Duration>,
    partial: (edge_stt_partial_cb, usize),
}

/// Run a body against a handle, or report a null one.
macro_rules! with {
    ($handle:expr, $name:ident => $body:expr) => {{
        if $handle.is_null() {
            return fail_with(
                edge_stt_error::EDGE_STT_NULL_ARGUMENT,
                "the handle must not be null",
            );
        }
        let $name = unsafe { &*$handle };
        $body
    }};
}

/// @brief The message behind the last failing call on this thread.
///
/// @return The message, borrowed until the next call on this thread
///         fails. Empty when nothing has failed yet.
#[unsafe(no_mangle)]
pub extern "C" fn edge_stt_get_last_error() -> *const c_char {
    last_message()
}

/// @brief Make a handle. Load a model into it before transcribing.
///
/// @return The handle. Making one cannot fail.
/// @see edge_stt_load_model, edge_stt_free
#[unsafe(no_mangle)]
pub extern "C" fn edge_stt_new() -> edge_stt_h {
    Box::into_raw(Box::new(edge_stt_handle {
        core: Mutex::new(None),
        settings: Mutex::new(Settings::default()),
        cancel: CancelToken::new(),
    }))
}

/// @brief Release the handle and everything it owns.
///
/// @param[in] stt the handle, or NULL, which does nothing
/// @see edge_stt_new
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_free(stt: edge_stt_h) {
    if !stt.is_null() {
        drop(unsafe { Box::from_raw(stt) });
    }
}

/// @brief Set the language before loading a model, or leave it unset
///        to let the model decide.
///
/// @param[in] stt the handle
/// @param[in] language a tag such as "ko", or NULL to detect
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_set_language(stt: edge_stt_h, language: *const c_char) -> i32 {
    with!(stt, handle => {
        let chosen = if language.is_null() {
            None
        } else {
            match required_str(language, "language") {
                Ok(tag) => Some(tag.to_string()),
                Err(code) => return code,
            }
        };
        settings(handle).language = chosen;
        ok()
    })
}

/// @brief Give up on a transcription that takes longer than this.
///
/// @param[in] stt the handle
/// @param[in] milliseconds the limit, or zero for none
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_set_timeout(stt: edge_stt_h, milliseconds: u64) -> i32 {
    with!(stt, handle => {
        settings(handle).timeout =
            (milliseconds > 0).then(|| Duration::from_millis(milliseconds));
        ok()
    })
}

/// @brief Load a model, which is when a missing or unusable file is
///        found out rather than on the first spoken word.
///
/// @param[in] stt the handle
/// @param[in] path the ggml file. You supply it; nothing is downloaded
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
///         edge_stt_get_last_error() says which path was searched.
/// @see edge_stt_new
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_load_model(stt: edge_stt_h, path: *const c_char) -> i32 {
    with!(stt, handle => {
        let path = match required_str(path, "path") {
            Ok(path) => path,
            Err(code) => return code,
        };
        let held = settings(handle);
        let mut config = Config::local(ModelSpec::at(path));
        if let Some(tag) = &held.language {
            config = config.with_language(Language::new(tag));
        }
        if let Some(limit) = held.timeout {
            config = config.with_timeout(limit);
        }
        drop(held);

        match EdgeStt::new(config) {
            Ok(built) => {
                *lock(&handle.core) = Some(built);
                ok()
            }
            Err(why) => fail(&why),
        }
    })
}

/// @brief Ask to be told about words as they are decoded.
///
/// @param[in] stt the handle
/// @param[in] callback called on the transcribing thread, never after
///            edge_stt_transcribe returns. NULL turns partials off
/// @param[in] user handed back to the callback untouched
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_transcribe
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_on_partial(
    stt: edge_stt_h,
    callback: edge_stt_partial_cb,
    user: *mut c_void,
) -> i32 {
    with!(stt, handle => {
        settings(handle).partial = (callback, user as usize);
        ok()
    })
}

/// @brief Turn a recording into text.
///
/// @param[in] stt the handle, with a model loaded
/// @param[in] samples 16000 Hz mono 16-bit samples. Borrowed for the
///            call
/// @param[in] count how many samples
/// @param[in] sample_rate must be 16000; anything else is refused
/// @param[out] out where the transcript is put. Free it with
///             edge_stt_transcript_free
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_transcript_free, edge_stt_cancel
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcribe(
    stt: edge_stt_h,
    samples: *const i16,
    count: usize,
    sample_rate: u32,
    out: *mut edge_stt_transcript_h,
) -> i32 {
    with!(stt, handle => {
        if samples.is_null() {
            return fail_with(
                edge_stt_error::EDGE_STT_NULL_ARGUMENT,
                "samples must not be null",
            );
        }
        if sample_rate != 16_000 {
            return fail_with(
                edge_stt_error::EDGE_STT_UNSUPPORTED_AUDIO,
                &format!("audio is {sample_rate} Hz, and only 16000 Hz mono 16-bit is taken"),
            );
        }

        let held = lock(&handle.core);
        let Some(core) = held.as_ref() else {
            return fail_with(
                edge_stt_error::EDGE_STT_NO_MODEL,
                "load a model before transcribing",
            );
        };

        let audio = unsafe { std::slice::from_raw_parts(samples, count) };
        let utterance = Utterance::mono_16k(audio);
        let (callback, user) = settings(handle).partial;

        let outcome = if callback.is_some() {
            core.transcribe_with(&utterance, |p| deliver(callback, user, &p), &handle.cancel)
        } else {
            core.transcribe(&utterance)
        };

        match outcome {
            Ok(transcript) => {
                match unsafe { out_box(out, edge_stt_transcript_handle::from_core(&transcript), "out") } {
                    Ok(()) => ok(),
                    Err(code) => code,
                }
            }
            Err(why) => fail(&why),
        }
    })
}

/// @brief Stop a transcription that is running, from any thread.
///
/// @param[in] stt the handle
/// @return #EDGE_STT_OK, or a negative #edge_stt_error. The
///         transcribing call returns #EDGE_STT_CANCELLED.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_cancel(stt: edge_stt_h) -> i32 {
    with!(stt, handle => {
        handle.cancel.cancel();
        ok()
    })
}

/// @brief The words that were said.
///
/// @param[in] transcript the transcript
/// @return The text, owned by the transcript, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_text(
    transcript: edge_stt_transcript_h,
) -> *const c_char {
    match unsafe { transcript.as_ref() } {
        Some(held) => held.text.as_ptr(),
        None => std::ptr::null(),
    }
}

/// @brief The language the model settled on.
///
/// @param[in] transcript the transcript
/// @return The tag, owned by the transcript, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_language(
    transcript: edge_stt_transcript_h,
) -> *const c_char {
    match unsafe { transcript.as_ref() } {
        Some(held) => held.language.as_ptr(),
        None => std::ptr::null(),
    }
}

/// @brief How long the audio ran, in milliseconds.
///
/// @param[in] transcript the transcript
/// @return The duration, or zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_audio_ms(
    transcript: edge_stt_transcript_h,
) -> u64 {
    unsafe { transcript.as_ref() }.map_or(0, |held| held.audio_duration_ms)
}

/// @brief How long transcribing took, in milliseconds. With the audio
///        duration, this is whether the hardware is keeping up.
///
/// @param[in] transcript the transcript
/// @return The time taken, or zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_processing_ms(
    transcript: edge_stt_transcript_h,
) -> u64 {
    unsafe { transcript.as_ref() }.map_or(0, |held| held.processing_time_ms)
}

/// @brief How sure the model is, between zero and one.
///
/// @param[in] transcript the transcript
/// @return The confidence, or zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_confidence(
    transcript: edge_stt_transcript_h,
) -> f32 {
    unsafe { transcript.as_ref() }.map_or(0.0, |held| held.confidence)
}

/// @brief How many timed segments the transcript holds.
///
/// @param[in] transcript the transcript
/// @return The count, or zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_segment_count(
    transcript: edge_stt_transcript_h,
) -> usize {
    unsafe { transcript.as_ref() }.map_or(0, |held| held.segments.len())
}

/// @brief One segment's words.
///
/// @param[in] transcript the transcript
/// @param[in] index below edge_stt_transcript_get_segment_count
/// @return The text, owned by the transcript, or NULL when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_segment_text(
    transcript: edge_stt_transcript_h,
    index: usize,
) -> *const c_char {
    match unsafe { transcript.as_ref() }.and_then(|held| held.segments.get(index)) {
        Some(segment) => segment.text.as_ptr(),
        None => std::ptr::null(),
    }
}

/// @brief Where one segment starts, in milliseconds from the start.
///
/// @param[in] transcript the transcript
/// @param[in] index below edge_stt_transcript_get_segment_count
/// @return The offset, or zero when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_segment_start_ms(
    transcript: edge_stt_transcript_h,
    index: usize,
) -> u64 {
    unsafe { transcript.as_ref() }
        .and_then(|held| held.segments.get(index))
        .map_or(0, |segment| segment.start_ms)
}

/// @brief Where one segment ends, in milliseconds from the start.
///
/// @param[in] transcript the transcript
/// @param[in] index below edge_stt_transcript_get_segment_count
/// @return The offset, or zero when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_segment_end_ms(
    transcript: edge_stt_transcript_h,
    index: usize,
) -> u64 {
    unsafe { transcript.as_ref() }
        .and_then(|held| held.segments.get(index))
        .map_or(0, |segment| segment.end_ms)
}

/// @brief How sure the model is about one segment.
///
/// @param[in] transcript the transcript
/// @param[in] index below edge_stt_transcript_get_segment_count
/// @return Between zero and one, or zero when out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_get_segment_confidence(
    transcript: edge_stt_transcript_h,
    index: usize,
) -> f32 {
    unsafe { transcript.as_ref() }
        .and_then(|held| held.segments.get(index))
        .map_or(0.0, |segment| segment.confidence)
}

/// @brief Release a transcript.
///
/// @param[in] transcript the transcript, or NULL, which does nothing
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_transcript_free(transcript: edge_stt_transcript_h) {
    if !transcript.is_null() {
        drop(unsafe { Box::from_raw(transcript) });
    }
}

fn settings(handle: &edge_stt_handle) -> std::sync::MutexGuard<'_, Settings> {
    lock(&handle.settings)
}

/// A poisoned lock means another thread panicked, which is not a
/// reason to refuse this caller.
fn lock<T>(held: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
