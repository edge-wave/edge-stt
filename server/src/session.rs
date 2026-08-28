//! One client's request, and how far it has got.

use edge_stt_core::CancelToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Received,
    Queued,
    Decoding,
    Completed,
    Failed,
    Cancelled,
}

impl SessionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
        )
    }
}

pub struct Session {
    pub request_id: String,
    pub state: SessionState,
    pub cancel: CancelToken,
    pub language: Option<String>,
    pub want_partials: bool,
    pub audio: Vec<i16>,
}

impl Session {
    pub fn new(request_id: String, language: Option<String>, want_partials: bool) -> Self {
        Self {
            request_id,
            state: SessionState::Received,
            cancel: CancelToken::new(),
            language,
            want_partials,
            audio: Vec::new(),
        }
    }

    /// A client that walked away is a client whose work should stop.
    pub fn abandon(&mut self) {
        if !self.state.is_terminal() {
            self.state = SessionState::Cancelled;
            self.cancel.cancel();
        }
    }
}

/// Samples arrive as little-endian bytes, which is what the shape says.
pub fn samples_from(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}
