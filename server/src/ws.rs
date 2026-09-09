//! The websocket route. Everything a client sees of the server.

use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use edge_stt_core::wire::{ClientMessage, ServerMessage};
use edge_stt_core::{Error, Partial, Utterance};

use crate::capacity::{Admission, Capacity};
use crate::session::{Session, SessionState, samples_from};
use crate::{Server, auth};

pub async fn upgrade(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if !auth::accepted(server.credential.as_deref(), &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(ws.on_upgrade(move |socket| serve(socket, server)))
}

async fn serve(mut socket: WebSocket, server: Arc<Server>) {
    let mut session: Option<Session> = None;

    while let Some(Ok(message)) = socket.recv().await {
        match message {
            Message::Text(text) => {
                let Ok(parsed) = serde_json::from_str::<ClientMessage>(&text) else {
                    continue;
                };
                match parsed {
                    ClientMessage::Start {
                        request_id,
                        format,
                        language,
                        want_partials,
                    } => {
                        if format != edge_stt_core::wire::WireFormat::mono_16k() {
                            let refusal = error_message(
                                &request_id,
                                "unsupported_audio",
                                "16000 Hz mono 16-bit is the only shape this server takes",
                            );
                            let _ = send(&mut socket, refusal).await;
                            return;
                        }
                        session = Some(Session::new(request_id, language, want_partials));
                    }
                    ClientMessage::End { .. } => {
                        let Some(current) = session.take() else {
                            continue;
                        };
                        run(&mut socket, &server, current).await;
                        return;
                    }
                    ClientMessage::Cancel { request_id } => {
                        if let Some(current) = session.as_mut() {
                            current.abandon();
                        }
                        let _ = send(&mut socket, ServerMessage::Cancelled { request_id }).await;
                        return;
                    }
                    ClientMessage::OpenStream {
                        request_id,
                        format,
                        language,
                        want_partials,
                        pause_tolerance_ms,
                        live_interims,
                        interim_min_interval_ms,
                    } => {
                        if format != edge_stt_core::wire::WireFormat::mono_16k() {
                            let refusal = error_message(
                                &request_id,
                                "unsupported_audio",
                                "16000 Hz mono 16-bit is the only shape this server takes",
                            );
                            let _ = send(&mut socket, refusal).await;
                            return;
                        }
                        // Producing words the client has said not to
                        // send is work nobody would ever see.
                        if live_interims && !want_partials {
                            let refusal = error_message(
                                &request_id,
                                "invalid_request",
                                "live_interims needs want_partials, or nothing would be sent",
                            );
                            let _ = send(&mut socket, refusal).await;
                            return;
                        }
                        run_continuous(
                            &mut socket,
                            &server,
                            StreamRequest {
                                request_id,
                                language,
                                want_partials,
                                pause_tolerance_ms,
                                live_interims,
                                interim_min_interval_ms,
                            },
                        )
                        .await;
                        return;
                    }
                    // Only meaningful inside run_continuous's own loop; one
                    // here means no stream was ever opened. Nothing to do.
                    ClientMessage::CloseStream { .. } => continue,
                }
            }
            Message::Binary(bytes) => {
                if let Some(current) = session.as_mut() {
                    current.audio = samples_from(&bytes);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // The client vanished. Nothing should still be decoding for it.
    if let Some(mut current) = session {
        current.abandon();
    }
}

async fn run(socket: &mut WebSocket, server: &Arc<Server>, mut session: Session) {
    let Some(stt) = server.transcriber() else {
        let refusal = error_message(
            &session.request_id,
            "model_unavailable",
            "the model is still loading",
        );
        let _ = send(socket, refusal).await;
        return;
    };

    if let Some(asked) = &session.language
        && server.language.as_deref() != Some(asked.as_str())
    {
        let served = server.language.as_deref().unwrap_or("whatever it detects");
        let refusal = error_message(
            &session.request_id,
            "unsupported_language",
            &format!("this server transcribes {served}, not {asked}"),
        );
        let _ = send(socket, refusal).await;
        return;
    }

    let permit = match server.capacity.admit().await {
        Admission::Started(permit) => {
            let accepted = ServerMessage::Accepted {
                request_id: session.request_id.clone(),
                queue_position: 0,
            };
            let _ = send(socket, accepted).await;
            permit
        }
        Admission::Queued(permit, position) => {
            session.state = SessionState::Queued;
            let accepted = ServerMessage::Accepted {
                request_id: session.request_id.clone(),
                queue_position: position,
            };
            let _ = send(socket, accepted).await;
            permit
        }
        Admission::Full => {
            let full = ServerMessage::Error {
                request_id: session.request_id.clone(),
                code: "at_capacity".to_string(),
                message: format!("{} requests already decoding", server.capacity.limit()),
                retry_after_ms: Some(Capacity::retry_after_ms()),
                queue_position: None,
            };
            let _ = send(socket, full).await;
            return;
        }
    };

    session.state = SessionState::Decoding;
    let request_id = session.request_id.clone();
    let cancel = session.cancel.clone();
    let audio = std::mem::take(&mut session.audio);
    let wants = session.want_partials;

    let (partials, mut incoming) = tokio::sync::mpsc::unbounded_channel::<Partial>();
    let decoding = tokio::task::spawn_blocking(move || {
        let utterance = Utterance::mono_16k(&audio);
        let outcome = if wants {
            stt.transcribe_with(
                &utterance,
                |partial| {
                    let _ = partials.send(partial);
                },
                &cancel,
            )
        } else {
            stt.transcribe(&utterance)
        };
        drop(permit);
        outcome
    });
    tokio::pin!(decoding);

    loop {
        tokio::select! {
            Some(partial) = incoming.recv() => {
                let _ = send(socket, ServerMessage::from_partial(&request_id, &partial)).await;
            }
            finished = &mut decoding => {
                // Anything the decoder produced just before it ended.
                while let Ok(partial) = incoming.try_recv() {
                    let _ = send(socket, ServerMessage::from_partial(&request_id, &partial)).await;
                }
                let reply = match finished {
                    Ok(Ok(transcript)) => {
                        session.state = SessionState::Completed;
                        ServerMessage::from_transcript(&request_id, &transcript)
                    }
                    Ok(Err(Error::Cancelled)) => {
                        session.state = SessionState::Cancelled;
                        ServerMessage::Cancelled { request_id: request_id.clone() }
                    }
                    Ok(Err(why)) => {
                        session.state = SessionState::Failed;
                        error_from(&request_id, &why)
                    }
                    Err(_) => {
                        session.state = SessionState::Failed;
                        error_message(&request_id, "internal", "the decoder stopped")
                    }
                };
                let _ = send(socket, reply).await;
                log::debug!("{request_id} ended {:?}", session.state);
                return;
            }
        }
    }
}

/// Continuous input: audio arrives with no predetermined end, and the
/// server -- not the caller -- decides utterance boundaries, sending
/// `final` once per one it finds until `close_stream` or a disconnect.
/// What a client asked for when it opened a stream.
struct StreamRequest {
    request_id: String,
    language: Option<String>,
    want_partials: bool,
    pause_tolerance_ms: Option<u64>,
    live_interims: bool,
    interim_min_interval_ms: Option<u64>,
}

async fn run_continuous(socket: &mut WebSocket, server: &Arc<Server>, request: StreamRequest) {
    let StreamRequest {
        request_id,
        language,
        want_partials,
        pause_tolerance_ms,
        live_interims,
        interim_min_interval_ms,
    } = request;
    let Some(stt) = server.transcriber() else {
        let refusal = error_message(
            &request_id,
            "model_unavailable",
            "the model is still loading",
        );
        let _ = send(socket, refusal).await;
        return;
    };

    if let Some(asked) = &language
        && server.language.as_deref() != Some(asked.as_str())
    {
        let served = server.language.as_deref().unwrap_or("whatever it detects");
        let refusal = error_message(
            &request_id,
            "unsupported_language",
            &format!("this server transcribes {served}, not {asked}"),
        );
        let _ = send(socket, refusal).await;
        return;
    }

    let Some(vad_model) = server.vad_model.clone() else {
        let refusal = error_message(
            &request_id,
            "streaming_unavailable",
            "this server has no VAD model configured",
        );
        let _ = send(socket, refusal).await;
        return;
    };

    // Held for the whole session: it counts as one client, not a
    // per-utterance cost.
    let permit = match server.capacity.admit().await {
        Admission::Started(permit) => {
            let accepted = ServerMessage::Accepted {
                request_id: request_id.clone(),
                queue_position: 0,
            };
            let _ = send(socket, accepted).await;
            permit
        }
        Admission::Queued(permit, position) => {
            let accepted = ServerMessage::Accepted {
                request_id: request_id.clone(),
                queue_position: position,
            };
            let _ = send(socket, accepted).await;
            permit
        }
        Admission::Full => {
            let full = ServerMessage::Error {
                request_id: request_id.clone(),
                code: "at_capacity".to_string(),
                message: format!("{} requests already decoding", server.capacity.limit()),
                retry_after_ms: Some(Capacity::retry_after_ms()),
                queue_position: None,
            };
            let _ = send(socket, full).await;
            return;
        }
    };

    let mut endpointing = edge_stt_core::EndpointConfig::new(&vad_model);
    if let Some(ms) = pause_tolerance_ms {
        endpointing = endpointing.with_pause_tolerance(std::time::Duration::from_millis(ms));
    }
    let mut config = edge_stt_core::SessionConfig::new().with_endpointing(endpointing);
    if live_interims {
        config = config.with_live_interims();
    }
    if let Some(ms) = interim_min_interval_ms {
        config = config.with_interim_min_interval(std::time::Duration::from_millis(ms));
    }

    let (audio_tx, mut events_rx) = crate::session::spawn_continuous(stt, config, want_partials);

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        let samples = samples_from(&bytes);
                        if audio_tx.send(crate::session::AudioInput::Chunk(samples)).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ClientMessage::CloseStream { .. }) = serde_json::from_str(&text) {
                            let _ = audio_tx.send(crate::session::AudioInput::CleanClose);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            event = events_rx.recv() => {
                match event {
                    Some(crate::session::ContinuousEvent::Partial(p)) => {
                        let _ = send(socket, ServerMessage::from_partial(&request_id, &p)).await;
                    }
                    Some(crate::session::ContinuousEvent::Final(t)) => {
                        let _ = send(socket, ServerMessage::from_transcript(&request_id, &t)).await;
                    }
                    Some(crate::session::ContinuousEvent::Error(why)) => {
                        let _ = send(socket, error_from(&request_id, &why)).await;
                    }
                    None => break,
                }
            }
        }
    }
    drop(permit);
}

fn error_from(request_id: &str, why: &Error) -> ServerMessage {
    let (code, message) = match why {
        Error::UnsupportedAudio { .. } => ("unsupported_audio", why.to_string()),
        Error::AudioTooLong { .. } => ("audio_too_long", why.to_string()),
        Error::Timeout { .. } => ("internal", why.to_string()),
        Error::ModelMissing { .. } | Error::ModelUnusable { .. } => {
            ("model_unavailable", "the model cannot be used".to_string())
        }
        Error::InsufficientResources { .. } => ("internal", why.to_string()),
        _ => ("internal", why.to_string()),
    };
    error_message(request_id, code, &message)
}

/// Nothing here carries transcribed text, so a server log stays free
/// of what people said.
fn error_message(request_id: &str, code: &str, message: &str) -> ServerMessage {
    ServerMessage::Error {
        request_id: request_id.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        retry_after_ms: None,
        queue_position: None,
    }
}

async fn send(socket: &mut WebSocket, message: ServerMessage) -> Result<(), ()> {
    let body = serde_json::to_string(&message).map_err(|_| ())?;
    socket
        .send(Message::Text(body.into()))
        .await
        .map_err(|_| ())
}
