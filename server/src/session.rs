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

/// One piece of audio, or the clean-shutdown signal, sent from the
/// async socket-reading side to the blocking decode side.
pub enum AudioInput {
    Chunk(Vec<i16>),
    CleanClose,
}

/// What the blocking decode side reports back, one event at a time.
pub enum ContinuousEvent {
    Partial(edge_stt_core::Partial),
    Final(edge_stt_core::Transcript),
    Error(edge_stt_core::Error),
}

/// Runs one continuous session's endpointing and decoding on a
/// blocking thread, fed by `AudioInput` and reporting `ContinuousEvent`
/// back -- the bridge a synchronous `AudioSession` needs in an async
/// server. Once nobody is reading the events (the client vanished),
/// `session` is dropped without `close()` and any audio still queued is
/// discarded. `permit` is held until then, so capacity counts decoding
/// that is really happening rather than sockets that are still open.
pub fn spawn_continuous(
    stt: std::sync::Arc<edge_stt_core::EdgeStt>,
    config: edge_stt_core::SessionConfig,
    want_partials: bool,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> (
    tokio::sync::mpsc::UnboundedSender<AudioInput>,
    tokio::sync::mpsc::UnboundedReceiver<ContinuousEvent>,
) {
    use edge_stt_core::Partial;

    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::unbounded_channel::<AudioInput>();
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel::<ContinuousEvent>();

    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut session = match stt.open_session(config) {
            Ok(session) => session,
            Err(why) => {
                let _ = events_tx.send(ContinuousEvent::Error(why));
                return;
            }
        };

        while let Some(first) = audio_rx.blocking_recv() {
            // Words for audio that newer audio is already waiting behind
            // are stale before they arrive, so only the newest gets a pass.
            let mut waiting = vec![first];
            while let Ok(next) = audio_rx.try_recv() {
                waiting.push(next);
            }
            let newest = waiting
                .iter()
                .rposition(|input| matches!(input, AudioInput::Chunk(_)));

            for (index, input) in waiting.into_iter().enumerate() {
                if events_tx.is_closed() {
                    return;
                }
                let mut sink = |p: Partial| {
                    let _ = events_tx.send(ContinuousEvent::Partial(p));
                };
                match input {
                    AudioInput::Chunk(samples) => {
                        let on_partial = (want_partials && Some(index) == newest)
                            .then_some(&mut sink as &mut dyn FnMut(Partial));
                        match session.push(&samples, on_partial) {
                            Ok(Some(transcript)) => {
                                let _ = events_tx.send(ContinuousEvent::Final(transcript));
                            }
                            Ok(None) => {}
                            Err(why) => {
                                let _ = events_tx.send(ContinuousEvent::Error(why));
                            }
                        }
                    }
                    AudioInput::CleanClose => {
                        let on_partial =
                            want_partials.then_some(&mut sink as &mut dyn FnMut(Partial));
                        if let Ok(transcripts) = session.close(on_partial) {
                            for transcript in transcripts {
                                let _ = events_tx.send(ContinuousEvent::Final(transcript));
                            }
                        }
                        return;
                    }
                }
            }
        }
    });

    (audio_tx, events_rx)
}
