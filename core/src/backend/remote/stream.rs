//! One session a server runs whole. A thread of its own owns the
//! socket, so audio keeps moving between the caller's calls and a slow
//! link never blocks one; what comes back waits to be handed over.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc as channel;
use tokio_tungstenite::tungstenite::Message;

use super::{Socket, connect, decode, encode, error_for, network, new_request_id};
use crate::backend::HostedStream;
use crate::config::RemoteConfig;
use crate::error::{Error, Result};
use crate::transcript::{Partial, Transcript};
use crate::wire::{ClientMessage, ServerMessage, WireFormat, partial_from, transcript_from};

/// Everything a stream is opened with.
pub(crate) struct Request {
    pub remote: RemoteConfig,
    pub language: Option<String>,
    /// `None` leaves every boundary to the caller.
    pub pause_tolerance: Option<Duration>,
    pub live_interims: bool,
    pub interim_min_interval: Duration,
    pub max_duration: Duration,
    pub timeout: Option<Duration>,
}

/// From the caller's thread to the socket's.
enum Up {
    Audio(Vec<i16>),
    Close,
}

/// From the socket's thread back to the caller's.
enum Down {
    Partial(Partial),
    Final(Transcript),
    Failed(Error),
    Ended,
}

pub(crate) struct RemoteStream {
    endpoint: String,
    up: channel::UnboundedSender<Up>,
    down: mpsc::Receiver<Down>,
    finals: VecDeque<Transcript>,
    /// Samples handed over and not yet written to the socket.
    unsent: Arc<AtomicUsize>,
    limit: usize,
    timeout: Option<Duration>,
    failed: bool,
}

impl RemoteStream {
    /// Returns once the server has accepted the stream, or with why not.
    pub(crate) fn open(request: Request) -> Result<Self> {
        let endpoint = request.remote.endpoint.clone();
        let limit = (request.max_duration.as_secs_f64() * 16_000.0) as usize;
        let timeout = request.timeout;
        let (up, from_caller) = channel::unbounded_channel();
        let (to_caller, down) = mpsc::channel();
        let (ready, opened) = mpsc::channel();
        let unsent = Arc::new(AtomicUsize::new(0));
        let written = Arc::clone(&unsent);

        let named = endpoint.clone();
        std::thread::Builder::new()
            .name("edge-stt-stream".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(why) => {
                        let _ = ready.send(Err(network(&named, why)));
                        return;
                    }
                };
                runtime.block_on(run(request, from_caller, to_caller, ready, written));
            })
            .map_err(|why| network(&endpoint, why))?;

        match opened.recv() {
            Ok(Ok(())) => Ok(Self {
                endpoint,
                up,
                down,
                finals: VecDeque::new(),
                unsent,
                limit,
                timeout,
                failed: false,
            }),
            Ok(Err(why)) => Err(why),
            Err(_) => Err(network(&endpoint, "the stream stopped before it opened")),
        }
    }

    fn fail(&mut self, why: Error) -> Error {
        self.failed = true;
        why
    }

    /// Hands over everything already back, without waiting for more.
    fn drain(&mut self, on_partial: &mut Option<&mut dyn FnMut(Partial)>) -> Result<()> {
        loop {
            match self.down.try_recv() {
                Ok(Down::Partial(partial)) => deliver(on_partial, partial),
                Ok(Down::Final(transcript)) => self.finals.push_back(transcript),
                Ok(Down::Failed(why)) => return Err(self.fail(why)),
                Ok(Down::Ended) | Err(mpsc::TryRecvError::Disconnected) => {
                    let why = network(&self.endpoint, "the server ended the stream");
                    return Err(self.fail(why));
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
            }
        }
    }
}

impl HostedStream for RemoteStream {
    fn push(
        &mut self,
        samples: &[i16],
        mut on_partial: Option<&mut dyn FnMut(Partial)>,
    ) -> Result<Option<Transcript>> {
        if self.failed {
            return Err(network(&self.endpoint, "the stream had already failed"));
        }
        if !samples.is_empty() {
            let waiting = self.unsent.fetch_add(samples.len(), Ordering::Relaxed) + samples.len();
            if waiting > self.limit {
                let why = network(&self.endpoint, "the link cannot keep up with the audio");
                return Err(self.fail(why));
            }
            // A closed channel means the thread has stopped; drain says why.
            let _ = self.up.send(Up::Audio(samples.to_vec()));
        }
        self.drain(&mut on_partial)?;
        Ok(self.finals.pop_front())
    }

    fn close(
        &mut self,
        mut on_partial: Option<&mut dyn FnMut(Partial)>,
    ) -> Result<Vec<Transcript>> {
        if self.failed {
            return Err(network(&self.endpoint, "the stream had already failed"));
        }
        let _ = self.up.send(Up::Close);
        let deadline = self.timeout.map(|limit| (limit, Instant::now() + limit));
        loop {
            let next = match deadline {
                Some((limit, until)) => {
                    let left = until.saturating_duration_since(Instant::now());
                    match self.down.recv_timeout(left) {
                        Ok(next) => next,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            return Err(self.fail(Error::Timeout { limit }));
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => Down::Failed(network(
                            &self.endpoint,
                            "the stream stopped without ending",
                        )),
                    }
                }
                None => self.down.recv().unwrap_or_else(|_| {
                    Down::Failed(network(&self.endpoint, "the stream stopped without ending"))
                }),
            };
            match next {
                Down::Partial(partial) => deliver(&mut on_partial, partial),
                Down::Final(transcript) => self.finals.push_back(transcript),
                Down::Failed(why) => return Err(self.fail(why)),
                Down::Ended => return Ok(self.finals.drain(..).collect()),
            }
        }
    }
}

fn deliver(on_partial: &mut Option<&mut dyn FnMut(Partial)>, partial: Partial) {
    if let Some(sink) = on_partial.as_deref_mut() {
        sink(partial);
    }
}

/// The socket's side: open, then carry audio up and replies down until
/// the stream ends, fails, or the session is dropped.
async fn run(
    request: Request,
    mut from_caller: channel::UnboundedReceiver<Up>,
    to_caller: mpsc::Sender<Down>,
    ready: mpsc::Sender<Result<()>>,
    written: Arc<AtomicUsize>,
) {
    let endpoint = request.remote.endpoint.clone();
    let request_id = new_request_id();
    let mut socket = match open(&request, &request_id).await {
        Ok(socket) => socket,
        Err(why) => {
            let _ = ready.send(Err(why));
            return;
        }
    };
    let _ = ready.send(Ok(()));

    let mut closing = false;
    loop {
        tokio::select! {
            command = from_caller.recv() => match command {
                Some(Up::Audio(samples)) => {
                    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                    let sent = socket.send(Message::Binary(bytes.into())).await;
                    written.fetch_sub(samples.len(), Ordering::Relaxed);
                    if let Err(why) = sent {
                        let _ = to_caller.send(Down::Failed(network(&endpoint, why)));
                        return;
                    }
                }
                Some(Up::Close) => {
                    closing = true;
                    let close = ClientMessage::CloseStream { request_id: request_id.clone() };
                    let sent = match encode(&close) {
                        Ok(text) => socket.send(Message::Text(text.into())).await.map_err(|why| network(&endpoint, why)),
                        Err(why) => Err(why),
                    };
                    if let Err(why) = sent {
                        let _ = to_caller.send(Down::Failed(why));
                        return;
                    }
                }
                None => {
                    let _ = socket.close(None).await;
                    return;
                }
            },
            frame = socket.next() => {
                let reply = match frame {
                    None | Some(Ok(Message::Close(_))) => {
                        let _ = to_caller.send(match closing {
                            true => Down::Ended,
                            false => Down::Failed(network(&endpoint, "the server ended the stream")),
                        });
                        return;
                    }
                    // A server that has answered close_stream may drop the
                    // socket without a closing handshake; its finals came first.
                    Some(Err(_)) if closing => {
                        let _ = to_caller.send(Down::Ended);
                        return;
                    }
                    Some(Err(why)) => {
                        let _ = to_caller.send(Down::Failed(network(&endpoint, why)));
                        return;
                    }
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(_)) => continue,
                };
                let Ok(message) = decode(&reply) else {
                    continue;
                };
                let down = match &message {
                    ServerMessage::Partial { .. } => partial_from(&message).map(Down::Partial),
                    ServerMessage::Final { .. } => transcript_from(&message).map(Down::Final),
                    ServerMessage::Error { code, message, retry_after_ms, queue_position, .. } => {
                        let why = error_for(&endpoint, code, message, *retry_after_ms, *queue_position);
                        let _ = to_caller.send(Down::Failed(why));
                        return;
                    }
                    _ => None,
                };
                if let Some(down) = down
                    && to_caller.send(down).is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Connects, asks for the stream, and waits to be accepted, refusing a
/// server that would detect boundaries the caller asked to decide.
async fn open(request: &Request, request_id: &str) -> Result<Socket> {
    let endpoint = &request.remote.endpoint;
    let mut socket = connect(&request.remote).await?;
    let caller_bounded = request.pause_tolerance.is_none();
    let asked = ClientMessage::OpenStream {
        request_id: request_id.to_string(),
        format: WireFormat::mono_16k(),
        language: request.language.clone(),
        want_partials: true,
        pause_tolerance_ms: request.pause_tolerance.map(|t| t.as_millis() as u64),
        live_interims: request.live_interims,
        interim_min_interval_ms: Some(request.interim_min_interval.as_millis() as u64),
        boundaries: Some(if caller_bounded { "caller" } else { "server" }.to_string()),
    };
    socket
        .send(Message::Text(encode(&asked)?.into()))
        .await
        .map_err(|why| network(endpoint, why))?;

    let answer = tokio::time::timeout(
        request.remote.connect_timeout,
        accepted(&mut socket, endpoint),
    );
    let applied = match answer.await {
        Err(_) => return Err(network(endpoint, "the stream was not accepted in time")),
        Ok(outcome) => outcome?,
    };
    if caller_bounded && applied.as_deref() != Some("caller") {
        let _ = socket.close(None).await;
        return Err(Error::InvalidValue {
            setting: "boundaries",
            expected: "a server that can leave boundaries to the caller".to_string(),
            got: "one that predates that choice and would detect them itself".to_string(),
        });
    }
    Ok(socket)
}

/// The boundaries the server said it applied, once it accepts.
async fn accepted(socket: &mut Socket, endpoint: &str) -> Result<Option<String>> {
    loop {
        let text = match socket.next().await {
            None | Some(Ok(Message::Close(_))) => {
                return Err(network(endpoint, "the server closed before accepting"));
            }
            Some(Err(why)) => return Err(network(endpoint, why)),
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(_)) => continue,
        };
        match decode(&text)? {
            ServerMessage::Accepted { boundaries, .. } => return Ok(boundaries),
            ServerMessage::Error {
                code,
                message,
                retry_after_ms,
                queue_position,
                ..
            } => {
                return Err(error_for(
                    endpoint,
                    &code,
                    &message,
                    retry_after_ms,
                    queue_position,
                ));
            }
            _ => continue,
        }
    }
}
