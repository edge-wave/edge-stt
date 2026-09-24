//! A server that speaks the protocol, so the remote backend can be
//! finished and tested without the real one existing yet.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
    /// Host a continuous stream the way the script says.
    Stream(StreamScript),
}

/// One thing the stub sends during a stream.
#[derive(Debug, Clone)]
pub enum Reply {
    Partial(String),
    Final(String),
    Error(&'static str),
}

/// How a stream is answered. Everything it heard lands in `log`.
#[derive(Debug, Clone, Default)]
pub struct StreamScript {
    /// What `accepted` echoes; `None` leaves the field out, as an older server does.
    pub echo: Option<&'static str>,
    /// Refuse the open with this error code instead of accepting it.
    pub refuse: Option<&'static str>,
    /// Sent once this many audio frames have arrived.
    pub after_frames: Vec<(usize, Reply)>,
    /// Sent on `close_stream`, before the connection ends.
    pub on_close: Vec<Reply>,
    /// Vanish once this many frames have arrived.
    pub drop_after_frames: Option<usize>,
    /// Never read past the open, so the client's writes back up.
    pub stop_reading: bool,
    pub log: Arc<Mutex<StreamLog>>,
}

/// What a stream's client sent, as the stub saw it.
#[derive(Debug, Default)]
pub struct StreamLog {
    pub opened_with: Option<serde_json::Value>,
    pub frames: usize,
    pub frames_at_close: Option<usize>,
}

impl StreamScript {
    /// A server from after caller-decided boundaries existed, echoing `echo`.
    pub fn echoing(echo: &'static str) -> Self {
        Self {
            echo: Some(echo),
            ..Self::default()
        }
    }

    pub fn frames(&self) -> usize {
        self.log.lock().expect("an unpoisoned log").frames
    }
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
        if kind == "open_stream"
            && let Behaviour::Stream(script) = &behaviour
        {
            script.log.lock().expect("an unpoisoned log").opened_with = Some(parsed.clone());
            host_stream(&mut socket, &request_id, script).await;
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
            Behaviour::Unauthorized | Behaviour::Stream(_) => {}
        }
        return;
    }
}

async fn host_stream(socket: &mut Socket, id: &str, script: &StreamScript) {
    if let Some(code) = script.refuse {
        let _ = send(socket, json_error(id, code, "refused")).await;
        return;
    }
    let echo = script
        .echo
        .map(|value| format!(r#","boundaries":"{value}""#))
        .unwrap_or_default();
    let accepted = format!(r#"{{"type":"accepted","request_id":"{id}","queue_position":0{echo}}}"#);
    if send(socket, accepted).await.is_err() {
        return;
    }
    if script.stop_reading {
        tokio::time::sleep(Duration::from_secs(120)).await;
        return;
    }

    let mut seq = 0;
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Binary(_) => {
                let frames = {
                    let mut log = script.log.lock().expect("an unpoisoned log");
                    log.frames += 1;
                    log.frames
                };
                if script.drop_after_frames == Some(frames) {
                    return;
                }
                for (after, reply) in &script.after_frames {
                    if *after == frames {
                        let _ = send(socket, reply_json(id, &mut seq, reply)).await;
                    }
                }
            }
            Message::Text(text) if text.contains("close_stream") => {
                {
                    let mut log = script.log.lock().expect("an unpoisoned log");
                    log.frames_at_close = Some(log.frames);
                }
                for reply in &script.on_close {
                    let _ = send(socket, reply_json(id, &mut seq, reply)).await;
                }
                let _ = socket.close(None).await;
                return;
            }
            _ => {}
        }
    }
}

fn reply_json(id: &str, seq: &mut u32, reply: &Reply) -> String {
    match reply {
        Reply::Partial(text) => {
            *seq += 1;
            format!(
                r#"{{"type":"partial","request_id":"{id}","seq":{},"kind":"replace","text":"{text}","start_ms":0,"end_ms":0}}"#,
                *seq - 1
            )
        }
        Reply::Final(text) => json_final(id, text),
        Reply::Error(code) => json_error(id, code, "failed"),
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
