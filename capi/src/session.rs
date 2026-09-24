//! Continuous audio input: push samples as they arrive, and the
//! endpointer -- not the caller -- decides where one utterance ends.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::Duration;

use edge_stt_core::{EdgeStt, EndpointConfig, Partial, SessionConfig, Transcript};

use crate::convert::{edge_stt_transcript_h, edge_stt_transcript_handle, required_str};
use crate::error::*;
use crate::partials::{deliver, edge_stt_partial_cb};
use crate::{core_guard, edge_stt_h, lock};

/// What a handle points to. Opaque on the C side, which only ever
/// names the pointer to this: `edge_stt_session_h`.
///
/// Its own partial and transcript callbacks -- not the parent
/// `edge_stt_h`'s -- so a one-shot edge_stt_transcribe running
/// against the same handle never shares a sink with this session.
#[allow(non_camel_case_types)]
pub struct edge_stt_session_handle {
    session: Mutex<edge_stt_core::AudioSession<'static>>,
    partial: Mutex<(edge_stt_partial_cb, usize)>,
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

/// The loaded core behind a handle, as a reference a session can hold.
///
/// SAFETY: it points into the Box `stt` owns, heap-stable for the
/// handle's lifetime. Sound as long as the caller keeps `stt` alive --
/// and calls neither edge_stt_load_model nor edge_stt_free on it --
/// while a session stays open (see edge_stt_session_free).
fn borrow_core(parent: &crate::edge_stt_handle) -> Option<&'static EdgeStt> {
    let guard = core_guard(parent);
    let Some(core) = guard.as_ref() else {
        fail_with(
            edge_stt_error::EDGE_STT_NO_MODEL,
            "load a model before opening a session",
        );
        return None;
    };
    Some(unsafe { &*(core as *const EdgeStt) })
}

/// What a session is opened with.
///
/// `struct_size` must be set to `sizeof(edge_stt_session_opts)`. It is
/// what lets fields be added later without breaking a program built
/// against an older header: anything this build does not recognise is
/// ignored, and anything the caller did not supply keeps its default.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy)]
pub struct edge_stt_session_opts {
    /// sizeof(edge_stt_session_opts), as the caller compiled it.
    pub struct_size: usize,
    /// A ggml VAD file for finding boundaries on this device -- a second,
    /// separate model from the one edge_stt_load_model loaded. Never read
    /// by a session a server runs, which uses the server's own.
    pub vad_model: *const c_char,
    /// How long a pause must last before an utterance is considered
    /// finished, or zero for the documented default.
    pub pause_tolerance_ms: u64,
    /// Non-zero to also deliver words while the speaker is still
    /// talking. Those cost repeated recognition, which is why asking
    /// for them is separate from registering a callback.
    pub live_interims: i32,
    /// The shortest gap between two delivered interim results, or zero
    /// for the documented default.
    pub interim_min_interval_ms: u64,
    /// Non-zero when the caller already knows where speech stops: an
    /// utterance then ends only at edge_stt_session_close or at the
    /// maximum duration. Cannot be combined with vad_model.
    pub caller_boundaries: i32,
    /// Non-zero to have boundaries detected without naming vad_model,
    /// which a session a server runs does not need.
    pub detect_boundaries: i32,
}

/// Reads as much of `opts` as both sides know about, leaving the rest
/// at its default. `struct_size` comes first so it can always be read.
unsafe fn read_opts(opts: *const edge_stt_session_opts) -> Option<edge_stt_session_opts> {
    let declared = unsafe { std::ptr::read_unaligned(opts.cast::<usize>()) };
    if declared < std::mem::size_of::<usize>() {
        fail_with(
            edge_stt_error::EDGE_STT_INVALID_VALUE,
            "set struct_size to sizeof(edge_stt_session_opts)",
        );
        return None;
    }

    let mut taken = edge_stt_session_opts {
        struct_size: 0,
        vad_model: std::ptr::null(),
        pause_tolerance_ms: 0,
        live_interims: 0,
        interim_min_interval_ms: 0,
        caller_boundaries: 0,
        detect_boundaries: 0,
    };
    let take = declared.min(std::mem::size_of::<edge_stt_session_opts>());
    unsafe {
        std::ptr::copy_nonoverlapping(opts.cast::<u8>(), (&raw mut taken).cast::<u8>(), take);
    }
    Some(taken)
}

/// @brief Open a continuous session: push samples as they arrive
///        instead of handing over one complete recording.
///
/// The parent handle must stay alive, with a model loaded, for as long
/// as the session stays open. Several sessions may be open on one
/// handle; each has its own state and none can see another's. Zero the
/// options, set `struct_size` and whatever else you need, and anything
/// left alone takes its documented default.
///
/// @param[in] stt the handle, with a model loaded
/// @param[in] opts what to open the session with
/// @return The handle, or NULL on failure --
///         edge_stt_get_last_error() says why, including asking for
///         words mid-utterance from a model that cannot produce them.
/// @see edge_stt_session_push, edge_stt_session_close, edge_stt_session_free
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_new(
    stt: edge_stt_h,
    opts: *const edge_stt_session_opts,
) -> edge_stt_session_h {
    if stt.is_null() {
        fail_with(
            edge_stt_error::EDGE_STT_NULL_ARGUMENT,
            "the handle must not be null",
        );
        return std::ptr::null_mut();
    }
    if opts.is_null() {
        fail_with(
            edge_stt_error::EDGE_STT_NULL_ARGUMENT,
            "the options must not be null",
        );
        return std::ptr::null_mut();
    }
    let parent = unsafe { &*stt };
    let Some(opts) = (unsafe { read_opts(opts) }) else {
        return std::ptr::null_mut();
    };

    let mut config = SessionConfig::new();
    let detects = !opts.vad_model.is_null() || opts.detect_boundaries != 0;
    if opts.caller_boundaries != 0 {
        if detects {
            fail_with(
                edge_stt_error::EDGE_STT_INVALID_VALUE,
                "set caller_boundaries or a detector (vad_model, detect_boundaries), not both",
            );
            return std::ptr::null_mut();
        }
        config = config.with_caller_boundaries();
    } else if detects {
        let mut endpointing = EndpointConfig::new();
        if !opts.vad_model.is_null() {
            match required_str(opts.vad_model, "vad_model") {
                Ok(path) => endpointing = endpointing.with_local_vad_model(path),
                Err(_) => return std::ptr::null_mut(),
            }
        }
        if opts.pause_tolerance_ms > 0 {
            endpointing =
                endpointing.with_pause_tolerance(Duration::from_millis(opts.pause_tolerance_ms));
        }
        config = config.with_endpointing(endpointing);
    }
    if opts.live_interims != 0 {
        config = config.with_live_interims();
    }
    if opts.interim_min_interval_ms > 0 {
        config =
            config.with_interim_min_interval(Duration::from_millis(opts.interim_min_interval_ms));
    }

    let Some(core) = borrow_core(parent) else {
        return std::ptr::null_mut();
    };
    open(core, config)
}

/// Kept apart from the opener so that how a session is built and how
/// a failure is reported stay in one place.
fn open(core: &'static EdgeStt, config: SessionConfig) -> edge_stt_session_h {
    match core.open_session(config) {
        Ok(session) => Box::into_raw(Box::new(edge_stt_session_handle {
            session: Mutex::new(session),
            partial: Mutex::new((None, 0)),
            transcript: Mutex::new((None, 0)),
        })),
        Err(why) => {
            fail(&why);
            std::ptr::null_mut()
        }
    }
}

/// @brief Ask to be told about words as they are decoded, for this
///        session specifically.
///
/// Independent of edge_stt_set_partial_cb on the parent handle: a
/// one-shot edge_stt_transcribe using that one, run alongside this
/// session, delivers to its own sink instead of this one.
///
/// @param[in] session the handle
/// @param[in] callback called on the thread that called
///            edge_stt_session_push, never after that call has
///            returned. NULL turns partials off
/// @param[in] user handed back to the callback untouched
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_session_push
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_set_partial_cb(
    session: edge_stt_session_h,
    callback: edge_stt_partial_cb,
    user: *mut c_void,
) -> i32 {
    with_session!(session, handle => {
        *lock(&handle.partial) = (callback, user as usize);
        ok()
    })
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
pub unsafe extern "C" fn edge_stt_session_set_transcript_cb(
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
/// edge_stt_session_set_transcript_cb instead.
///
/// @param[in] session the handle
/// @param[in] samples 16000 Hz mono 16-bit samples. Borrowed for the call
/// @param[in] count how many samples
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_session_set_partial_cb, edge_stt_session_set_transcript_cb, edge_stt_session_close
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
        let (partial_cb, partial_user) = *lock(&handle.partial);

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
/// The utterance this finalizes decodes like any other: whatever is
/// registered with edge_stt_session_set_partial_cb still runs for it.
/// Every transcript not yet delivered reaches the transcript callback,
/// once each and in order; against a server finding its own boundaries
/// there may be more than one.
///
/// @param[in] session the handle
/// @return #EDGE_STT_OK, or a negative #edge_stt_error.
/// @see edge_stt_session_set_partial_cb, edge_stt_session_set_transcript_cb, edge_stt_session_free
#[unsafe(no_mangle)]
pub unsafe extern "C" fn edge_stt_session_close(session: edge_stt_session_h) -> i32 {
    with_session!(session, handle => {
        let (partial_cb, partial_user) = *lock(&handle.partial);
        let mut sink = |p: Partial| deliver(partial_cb, partial_user, &p);
        let on_partial: Option<&mut dyn FnMut(Partial)> = if partial_cb.is_some() {
            Some(&mut sink)
        } else {
            None
        };

        match lock(&handle.session).close(on_partial) {
            Ok(transcripts) => {
                for transcript in &transcripts {
                    notify(handle, transcript);
                }
                ok()
            }
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
