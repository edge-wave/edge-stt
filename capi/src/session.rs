//! Continuous audio input: push samples as they arrive, and the
//! endpointer -- not the caller -- decides where one utterance ends.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::Duration;

use edge_stt_core::{EdgeStt, EndpointConfig, Partial, Transcript};

use crate::convert::{edge_stt_transcript_h, edge_stt_transcript_handle, required_str};
use crate::error::*;
use crate::partials::deliver;
use crate::{core_guard, edge_stt_h, lock, settings};

/// What a handle points to. Opaque on the C side, which only ever
/// names the pointer to this: `edge_stt_session_h`.
#[allow(non_camel_case_types)]
pub struct edge_stt_session_handle {
    parent: edge_stt_h,
    session: Mutex<edge_stt_core::AudioSession<'static>>,
    transcript: Mutex<(edge_stt_transcript_cb, usize)>,
}

/// The handle a C caller holds.
#[allow(non_camel_case_types)]
pub type edge_stt_session_h = *mut edge_stt_session_handle;

/// Owns the transcript it is handed; free it with
/// edge_stt_transcript_free once done with it.
#[allow(non_camel_case_types)]
pub type edge_stt_transcript_cb =
    Option<unsafe extern "C" fn(transcript: edge_stt_transcript_h, user: *mut c_void)>;

/// Run a body against a session handle, or report a null one.
macro_rules! with_session {
    ($handle:expr, $name:ident => $body:expr) => {{
        if $handle.is_null() {
            return fail_with(
                edge_stt_error::EDGE_STT_NULL_ARGUMENT,
                "the session must not be null",
            );
        }
        let $name = unsafe { &*$handle };
        $body
    }};
}

/// @brief Open a continuous session: push samples as they arrive
///        instead of handing over one complete recording.
///
/// The parent handle must stay alive, with a model loaded, for as
/// long as the session stays open. Only one session may be open on a
/// handle at a time.
///
/// @param[in] stt the handle, with a model loaded
/// @param[in] vad_model a ggml VAD file -- a second, separate model
///            from the one edge_stt_load_model loaded
/// @param[in] pause_tolerance_ms how long a pause must last before an
///            utterance is considered finished, or zero for the
///            documented default
/// @return The handle, or NULL on failure --
///         edge_stt_get_last_error() says why.
/// @see edge_stt_session_push, edge_stt_session_close, edge_stt_session_free
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_new(
    stt: edge_stt_h,
    vad_model: *const c_char,
    pause_tolerance_ms: u64,
) -> edge_stt_session_h {
    if stt.is_null() {
        fail_with(
            edge_stt_error::EDGE_STT_NULL_ARGUMENT,
            "the handle must not be null",
        );
        return std::ptr::null_mut();
    }
    let parent = unsafe { &*stt };

    let vad_model = match required_str(vad_model, "vad_model") {
        Ok(path) => path,
        Err(_) => return std::ptr::null_mut(),
    };

    let guard = core_guard(parent);
    let Some(core) = guard.as_ref() else {
        fail_with(
            edge_stt_error::EDGE_STT_NO_MODEL,
            "load a model before opening a session",
        );
        return std::ptr::null_mut();
    };
    // SAFETY: `core` points into the Box `stt` owns, heap-stable for
    // the handle's lifetime. Sound as long as the caller keeps `stt`
    // alive -- and does not call edge_stt_load_model or edge_stt_free
    // on it -- while this session stays open (see edge_stt_session_free).
    let core: &'static EdgeStt = unsafe { &*(core as *const EdgeStt) };
    drop(guard);

    let mut config = EndpointConfig::new(vad_model);
    if pause_tolerance_ms > 0 {
        config = config.with_pause_tolerance(Duration::from_millis(pause_tolerance_ms));
    }

    match core.open_session(config) {
        Ok(session) => Box::into_raw(Box::new(edge_stt_session_handle {
            parent: stt,
            session: Mutex::new(session),
            transcript: Mutex::new((None, 0)),
        })),
        Err(why) => {
            fail(&why);
            std::ptr::null_mut()
        }
    }
}

/// @brief Ask to be told when the endpointer finishes an utterance.
///
/// @param[in] session the handle
/// @param[in] callback called on the thread that called
///            edge_stt_session_push or edge_stt_session_close, never
///            after that call has returned. NULL stops delivery
/// @param[in] user handed back to the callback untouched
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_session_push, edge_stt_session_close
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_on_transcript(
    session: edge_stt_session_h,
    callback: edge_stt_transcript_cb,
    user: *mut c_void,
) -> i32 {
    with_session!(session, handle => {
        *lock(&handle.transcript) = (callback, user as usize);
        ok()
    })
}

/// @brief Feed one piece of newly-captured audio.
///
/// Delivers nothing to the caller directly: an utterance, when the
/// endpointer finishes one, arrives through the callback set with
/// edge_stt_on_transcript instead.
///
/// @param[in] session the handle
/// @param[in] samples 16000 Hz mono 16-bit samples. Borrowed for the call
/// @param[in] count how many samples
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_on_transcript, edge_stt_session_close
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_push(
    session: edge_stt_session_h,
    samples: *const i16,
    count: usize,
) -> i32 {
    with_session!(session, handle => {
        if samples.is_null() {
            return fail_with(
                edge_stt_error::EDGE_STT_NULL_ARGUMENT,
                "samples must not be null",
            );
        }
        let audio = unsafe { std::slice::from_raw_parts(samples, count) };
        let parent = unsafe { &*handle.parent };
        let (partial_cb, partial_user) = settings(parent).partial;

        let mut sink = |p: Partial| deliver(partial_cb, partial_user, &p);
        let on_partial: Option<&mut dyn FnMut(Partial)> = if partial_cb.is_some() {
            Some(&mut sink)
        } else {
            None
        };

        match lock(&handle.session).push(audio, on_partial) {
            Ok(Some(transcript)) => {
                notify(handle, &transcript);
                ok()
            }
            Ok(None) => ok(),
            Err(why) => fail(&why),
        }
    })
}

/// @brief Finalize and deliver whatever utterance was in progress,
///        then close the session. A second call does nothing.
///
/// @param[in] session the handle
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_on_transcript, edge_stt_session_free
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_close(session: edge_stt_session_h) -> i32 {
    with_session!(session, handle => {
        match lock(&handle.session).close() {
            Ok(Some(transcript)) => {
                notify(handle, &transcript);
                ok()
            }
            Ok(None) => ok(),
            Err(why) => fail(&why),
        }
    })
}

/// @brief Release the session and everything it owns. Does not close
///        it first: call edge_stt_session_close beforehand for
///        whatever was in progress to be delivered.
///
/// @param[in] session the handle, or NULL, which does nothing
/// @see edge_stt_session_new, edge_stt_session_close
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_free(session: edge_stt_session_h) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Hands the finished utterance to whatever callback is registered,
/// as an owned transcript the receiver frees like any other.
fn notify(handle: &edge_stt_session_handle, transcript: &Transcript) {
    let (callback, user) = *lock(&handle.transcript);
    let Some(callback) = callback else {
        return;
    };
    let owned = Box::into_raw(Box::new(edge_stt_transcript_handle::from_core(transcript)));
    unsafe { callback(owned, user as *mut c_void) };
}
