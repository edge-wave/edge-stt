//! Reaching a server. The transcript that comes back has the same
//! shape the on-device backend produces, which is the whole point.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::runtime::Runtime;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use super::{Backend, Opened, Work};
use crate::config::{BackendKind, Boundaries, Config, Language, RemoteConfig, SessionConfig};
use crate::error::{Error, Result};
use crate::transcript::Transcript;
use crate::utterance::Utterance;
use crate::wire::{ClientMessage, ServerMessage, WireFormat, partial_from, transcript_from};

mod stream;

pub struct RemoteBackend {
    remote: RemoteConfig,
    language: Option<Language>,
    runtime: Runtime,
    max_duration: Duration,
    timeout: Option<Duration>,
}

impl RemoteBackend {
    pub fn connect(remote: &RemoteConfig, config: &Config) -> Result<Self> {
        remote.check()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|why| Error::Network {
                endpoint: remote.endpoint.clone(),
                why: why.to_string(),
            })?;
        Ok(Self {
            remote: remote.clone(),
            language: config.language.clone(),
            runtime,
            max_duration: config.max_duration,
            timeout: config.timeout,
        })
    }

    fn network(&self, why: impl ToString) -> Error {
        Error::Network {
            endpoint: self.remote.endpoint.clone(),
            why: why.to_string(),
        }
    }
}

impl Backend for RemoteBackend {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_, '_>) -> Result<Transcript> {
        let started = Instant::now();
        if work.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let request_id = new_request_id();
        let audio = utterance.as_bytes();
        let start = ClientMessage::Start {
            request_id: request_id.clone(),
            format: WireFormat::mono_16k(),
            language: self.language.as_ref().map(|l| l.as_str().to_string()),
            want_partials: work.wants_partials(),
        };
        let end = ClientMessage::End {
            request_id: request_id.clone(),
        };

        self.runtime.block_on(async {
            let mut socket = self.open().await?;

            socket
                .send(Message::Text(encode(&start)?.into()))
                .await
                .map_err(|e| self.network(e))?;
            socket
                .send(Message::Binary(audio.into()))
                .await
                .map_err(|e| self.network(e))?;
            socket
                .send(Message::Text(encode(&end)?.into()))
                .await
                .map_err(|e| self.network(e))?;

            self.read_replies(&mut socket, work, started).await
        })
    }

    /// A session runs on the server whole, so its early words, and its
    /// boundaries when a detector is wanted, all come from there.
    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities {
            live_interims: true,
            revises: true,
            self_endpointing: false,
            hosts_sessions: true,
        }
    }

    fn open_hosted(&self, config: &SessionConfig) -> Result<Opened<'_>> {
        let pause_tolerance = match &config.boundaries {
            Boundaries::Caller => None,
            Boundaries::Detector(endpointing) => Some(endpointing.pause_tolerance),
            Boundaries::Recognizer => {
                return Err(Error::BackendUnavailable {
                    backend: "a server that finds boundaries without a detector",
                });
            }
        };
        let stream = stream::RemoteStream::open(stream::Request {
            remote: self.remote.clone(),
            language: self.language.as_ref().map(|l| l.as_str().to_string()),
            pause_tolerance,
            live_interims: config.live_interims,
            interim_min_interval: config.interim_min_interval,
            max_duration: self.max_duration,
            timeout: self.timeout,
        })?;
        Ok(Opened::Hosted(Box::new(stream)))
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Remote
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The handshake both a request and a stream begin with.
async fn connect(remote: &RemoteConfig) -> Result<Socket> {
    let mut request = remote
        .endpoint
        .as_str()
        .into_client_request()
        .map_err(|e| Error::InvalidValue {
            setting: "endpoint",
            expected: "a websocket address".to_string(),
            got: e.to_string(),
        })?;
    if let Some(credential) = &remote.credential {
        let value = format!("Bearer {}", credential.expose());
        let header = HeaderValue::from_str(&value).map_err(|_| Error::InvalidValue {
            setting: "credential",
            expected: "characters a header can carry".to_string(),
            got: "unusable".to_string(),
        })?;
        request.headers_mut().insert("authorization", header);
    }

    let attempt = tokio::time::timeout(
        remote.connect_timeout,
        tokio_tungstenite::connect_async(request),
    )
    .await;

    match attempt {
        Err(_) => Err(network(
            &remote.endpoint,
            "the connection was not answered in time",
        )),
        Ok(Err(why)) => Err(classify_handshake(&remote.endpoint, why)),
        Ok(Ok((socket, _response))) => Ok(socket),
    }
}

/// A dead host and a refused credential need different answers from
/// the caller, so they never share a variant.
fn classify_handshake(endpoint: &str, why: tokio_tungstenite::tungstenite::Error) -> Error {
    if let tokio_tungstenite::tungstenite::Error::Http(response) = &why
        && response.status().as_u16() == 401
    {
        return Error::CredentialRejected {
            endpoint: endpoint.to_string(),
        };
    }
    network(endpoint, why)
}

fn network(endpoint: &str, why: impl ToString) -> Error {
    Error::Network {
        endpoint: endpoint.to_string(),
        why: why.to_string(),
    }
}

impl RemoteBackend {
    async fn open(&self) -> Result<Socket> {
        connect(&self.remote).await
    }

    async fn read_replies(
        &self,
        socket: &mut Socket,
        work: &mut Work<'_, '_>,
        started: Instant,
    ) -> Result<Transcript> {
        loop {
            if work.cancel.is_cancelled() {
                let _ = socket.close(None).await;
                return Err(Error::Cancelled);
            }
            if let Some(limit) = work.timeout
                && started.elapsed() >= limit
            {
                let _ = socket.close(None).await;
                return Err(Error::Timeout { limit });
            }

            let next = match work.timeout {
                Some(limit) => {
                    let left = limit.saturating_sub(started.elapsed());
                    match tokio::time::timeout(left, socket.next()).await {
                        Err(_) => {
                            let _ = socket.close(None).await;
                            return Err(Error::Timeout { limit });
                        }
                        Ok(frame) => frame,
                    }
                }
                None => socket.next().await,
            };

            let message = match next {
                None => return Err(self.network("the connection closed before a transcript")),
                Some(Err(why)) => return Err(self.network(why)),
                Some(Ok(Message::Text(text))) => decode(&text)?,
                Some(Ok(Message::Close(_))) => {
                    return Err(self.network("the server closed before a transcript"));
                }
                Some(Ok(_)) => continue,
            };

            match &message {
                ServerMessage::Accepted { .. } => continue,
                ServerMessage::Partial { .. } => {
                    if let Some(partial) = partial_from(&message) {
                        work.emit(partial);
                    }
                }
                ServerMessage::Final { .. } => {
                    let mut transcript = transcript_from(&message)
                        .ok_or_else(|| self.network("a final message without a transcript"))?;
                    transcript.backend = BackendKind::Remote;
                    return Ok(transcript);
                }
                ServerMessage::Cancelled { .. } => return Err(Error::Cancelled),
                ServerMessage::Error {
                    code,
                    message,
                    retry_after_ms,
                    queue_position,
                    ..
                } => {
                    return Err(error_for(
                        &self.remote.endpoint,
                        code,
                        message,
                        *retry_after_ms,
                        *queue_position,
                    ));
                }
            }
        }
    }
}

fn error_for(
    endpoint: &str,
    code: &str,
    message: &str,
    retry_after_ms: Option<u64>,
    queue_position: Option<u32>,
) -> Error {
    let endpoint = endpoint.to_string();
    match code {
        "unsupported_audio" => Error::InvalidValue {
            setting: "audio",
            expected: "16000 Hz mono 16-bit".to_string(),
            got: message.to_string(),
        },
        "audio_too_long" => Error::InvalidValue {
            setting: "audio",
            expected: "audio within the server's limit".to_string(),
            got: message.to_string(),
        },
        "at_capacity" => Error::ServerAtCapacity {
            endpoint,
            queue_position,
            retry_after: retry_after_ms.map(Duration::from_millis),
        },
        "unauthorized" => Error::CredentialRejected { endpoint },
        "unsupported_language" => Error::InvalidValue {
            setting: "language",
            expected: "one the server was started with".to_string(),
            got: message.to_string(),
        },
        "streaming_unavailable" => Error::InvalidValue {
            setting: "endpointing",
            expected: "a server started with a VAD model to find boundaries".to_string(),
            got: message.to_string(),
        },
        "invalid_request" => Error::InvalidValue {
            setting: "request",
            expected: "one this server understands".to_string(),
            got: message.to_string(),
        },
        _ => Error::ServerError {
            endpoint,
            why: message.to_string(),
        },
    }
}

fn encode(message: &ClientMessage) -> Result<String> {
    serde_json::to_string(message).map_err(|why| Error::InvalidValue {
        setting: "message",
        expected: "something that can be sent".to_string(),
        got: why.to_string(),
    })
}

fn decode(text: &str) -> Result<ServerMessage> {
    serde_json::from_str(text).map_err(|why| Error::ServerError {
        endpoint: String::new(),
        why: format!("the server said something unreadable: {why}"),
    })
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
