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
