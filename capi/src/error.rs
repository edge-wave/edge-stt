//! Turning core errors into the codes a C caller sees.

use std::cell::RefCell;
use std::ffi::{CString, c_char};

use edge_stt_core::Error;

/// @brief What went wrong. Zero is success; everything else is
///        negative, one value for each failure the core reports.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(non_camel_case_types)]
pub enum edge_stt_error {
    /// It worked.
    EDGE_STT_OK = 0,
    /// A pointer that must not be null was null.
    EDGE_STT_NULL_ARGUMENT = -1,
    /// The audio is not 16000 Hz mono 16-bit.
    EDGE_STT_UNSUPPORTED_AUDIO = -2,
    /// The recording is longer than this transcriber accepts.
    EDGE_STT_AUDIO_TOO_LONG = -3,
    /// No model file at that path.
    EDGE_STT_MODEL_MISSING = -4,
    /// The file is not a model this backend can use.
    EDGE_STT_MODEL_UNUSABLE = -5,
    /// Not enough memory or compute for the model chosen.
    EDGE_STT_INSUFFICIENT_RESOURCES = -6,
    /// The server could not be reached.
    EDGE_STT_NETWORK = -7,
    /// The server refused the credential.
    EDGE_STT_CREDENTIAL_REJECTED = -8,
    /// The server has no room right now.
    EDGE_STT_SERVER_AT_CAPACITY = -9,
    /// The server failed on its own account.
    EDGE_STT_SERVER_ERROR = -10,
    /// The time limit ran out.
    EDGE_STT_TIMEOUT = -11,
    /// The caller cancelled it.
    EDGE_STT_CANCELLED = -12,
    /// A setting is outside what it allows.
    EDGE_STT_INVALID_VALUE = -13,
    /// This build does not carry that backend.
    EDGE_STT_BACKEND_UNAVAILABLE = -14,
    /// No model has been loaded into this handle yet.
    EDGE_STT_NO_MODEL = -15,
    /// Something in the library gave way.
    EDGE_STT_INTERNAL = -99,
}

thread_local! {
    static LAST: RefCell<CString> = RefCell::new(CString::default());
}

pub fn code_of(error: &Error) -> edge_stt_error {
    use edge_stt_error::*;
    match error {
        Error::UnsupportedAudio { .. } => EDGE_STT_UNSUPPORTED_AUDIO,
        Error::AudioTooLong { .. } => EDGE_STT_AUDIO_TOO_LONG,
        Error::ModelMissing { .. } => EDGE_STT_MODEL_MISSING,
        Error::ModelUnusable { .. } => EDGE_STT_MODEL_UNUSABLE,
        Error::InsufficientResources { .. } => EDGE_STT_INSUFFICIENT_RESOURCES,
        Error::Network { .. } => EDGE_STT_NETWORK,
        Error::CredentialRejected { .. } => EDGE_STT_CREDENTIAL_REJECTED,
        Error::ServerAtCapacity { .. } => EDGE_STT_SERVER_AT_CAPACITY,
        Error::ServerError { .. } => EDGE_STT_SERVER_ERROR,
        Error::Timeout { .. } => EDGE_STT_TIMEOUT,
        Error::Cancelled => EDGE_STT_CANCELLED,
        Error::InvalidValue { .. } => EDGE_STT_INVALID_VALUE,
        Error::BackendUnavailable { .. } => EDGE_STT_BACKEND_UNAVAILABLE,
    }
}

/// Remember why, and hand back the code. The message keeps the detail
/// the Rust error carried, so nothing is lost at the boundary.
pub fn fail(error: &Error) -> i32 {
    remember(&error.to_string());
    code_of(error) as i32
}

pub fn fail_with(code: edge_stt_error, message: &str) -> i32 {
    remember(message);
    code as i32
}

pub fn ok() -> i32 {
    remember("");
    edge_stt_error::EDGE_STT_OK as i32
}

fn remember(message: &str) {
    let text = CString::new(message).unwrap_or_default();
    LAST.with(|last| *last.borrow_mut() = text);
}

pub fn last_message() -> *const c_char {
    LAST.with(|last| last.borrow().as_ptr())
}
