//! A server that speaks the protocol, so the remote backend can be
//! finished and tested without the real one existing yet.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

/// What the stub should do when a client arrives.
#[derive(Debug, Clone)]
pub enum Behaviour {
    /// Answer properly, with these partials before the final text.
    Transcribe { text: String, partials: Vec<String> },
    /// Refuse the handshake, the way a wrong credential is refused.
    Unauthorized,
    /// Accept, then say there is no room.
    AtCapacity,
    /// Accept, send one partial, then vanish mid-request.
    DropMidRequest,
    /// Accept and then say nothing at all.
    Stall,
    /// Accept and then fail.
    ServerError,
}

pub struct StubServer {
    pub endpoint: String,
    stop: Arc<AtomicBool>,
    credential: Option<String>,
}

impl StubServer {
    pub fn start(behaviour: Behaviour) -> Self {
        Self::start_with_credential(behaviour, None)
    }

    pub fn start_with_credential(behaviour: Behaviour, credential: Option<&str>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, port) = std::sync::mpsc::channel();
        let listening = stop.clone();
        let wanted = credential.map(str::to_string);
        let expected = wanted.clone();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
                let address: SocketAddr = listener.local_addr().expect("an address");
                ready.send(address.port()).expect("the test to be waiting");

                while !listening.load(Ordering::Relaxed) {
                    let accepted =
                        tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;
                    let Ok(Ok((stream, _))) = accepted else {
                        continue;
                    };
                    let behaviour = behaviour.clone();
                    let expected = expected.clone();
                    tokio::spawn(async move { serve(stream, behaviour, expected).await });
                }
            });
        });

        let port = port.recv().expect("the stub to bind");
        Self {
            endpoint: format!("ws://127.0.0.1:{port}/api/v1/transcribe"),
            stop,
            credential: wanted,
        }
    }

    pub fn credential(&self) -> Option<&str> {
        self.credential.as_deref()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

// tungstenite chooses the closure's shape, and its error type is
// wide; boxing it here would not fit what accept_hdr_async takes.
#[allow(clippy::result_large_err)]
async fn serve(stream: tokio::net::TcpStream, behaviour: Behaviour, expected: Option<String>) {
    let refuse = matches!(behaviour, Behaviour::Unauthorized);
    let check = move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        let presented = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::to_string);
        let wrong = expected
            .as_ref()
            .is_some_and(|want| presented.as_ref() != Some(want));
        if refuse || wrong {
            let mut denied = ErrorResponse::new(Some("unauthorized".to_string()));
            *denied.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
            return Err(denied);
        }
        Ok(response)
    };

    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, check).await else {
        return;
    };

    let mut request_id = String::new();
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let kind = parsed
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if let Some(id) = parsed.get("request_id").and_then(|v| v.as_str()) {
            request_id = id.to_string();
        }
        if kind == "cancel" {
            let _ = send(&mut socket, json_cancelled(&request_id)).await;
            return;
        }
        if kind != "end" {
            continue;
        }

        let _ = send(&mut socket, json_accepted(&request_id)).await;
        match &behaviour {
            Behaviour::Transcribe { text, partials } => {
                for (seq, piece) in partials.iter().enumerate() {
                    let body = json_partial(&request_id, seq as u32, piece);
                    let _ = send(&mut socket, body).await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                let _ = send(&mut socket, json_final(&request_id, text)).await;
            }
            Behaviour::AtCapacity => {
                let _ = send(
                    &mut socket,
                    json_error(&request_id, "at_capacity", "no room"),
                )
                .await;
            }
            Behaviour::ServerError => {
                let _ = send(&mut socket, json_error(&request_id, "internal", "boom")).await;
            }
            Behaviour::DropMidRequest => {
                let _ = send(&mut socket, json_partial(&request_id, 0, "half a")).await;
                return;
            }
            Behaviour::Stall => {
                tokio::time::sleep(Duration::from_secs(120)).await;
            }
            Behaviour::Unauthorized => {}
        }
        return;
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn send(socket: &mut Socket, body: String) -> Result<(), ()> {
    socket
        .send(Message::Text(body.into()))
        .await
        .map_err(|_| ())
}

fn json_accepted(id: &str) -> String {
    format!(r#"{{"type":"accepted","request_id":"{id}","queue_position":0}}"#)
}

fn json_partial(id: &str, seq: u32, text: &str) -> String {
    format!(
        r#"{{"type":"partial","request_id":"{id}","seq":{seq},"kind":"append","text":"{text}","start_ms":0,"end_ms":100}}"#
    )
}

fn json_final(id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"final","request_id":"{id}","text":"{text}","language":"en","confidence":0.9,"audio_duration_ms":1000,"processing_time_ms":100,"segments":[{{"text":"{text}","start_ms":0,"end_ms":1000,"confidence":0.9}}]}}"#
    )
}

fn json_cancelled(id: &str) -> String {
    format!(r#"{{"type":"cancelled","request_id":"{id}"}}"#)
}

fn json_error(id: &str, code: &str, message: &str) -> String {
    format!(
        r#"{{"type":"error","request_id":"{id}","code":"{code}","message":"{message}","retry_after_ms":2000}}"#
    )
}
