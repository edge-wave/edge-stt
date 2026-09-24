//! A stream whose boundaries the caller decides: the server detects
//! nothing, needs no VAD model, and ends the utterance at `close_stream`.

mod support;

use std::time::Duration;

use edge_stt_core::wire::ServerMessage;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn open_stream(request_id: &str, boundaries: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":false,"boundaries":"{boundaries}"}}"#
    )
}

fn close_stream(request_id: &str) -> String {
    format!(r#"{{"type":"close_stream","request_id":"{request_id}"}}"#)
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next message the server sends, or `None` once it is quiet for `wait`.
async fn next(socket: &mut Socket, wait: Duration) -> Option<ServerMessage> {
    loop {
        let frame = tokio::time::timeout(wait, socket.next())
            .await
            .ok()??
            .ok()?;
        let Ok(text) = frame.into_text() else {
            continue;
        };
        if let Ok(message) = serde_json::from_str::<ServerMessage>(&text) {
            return Some(message);
        }
    }
}

#[tokio::test]
async fn an_unknown_kind_of_boundaries_is_refused() {
    let running = support::start(None, 2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    socket
        .send(Message::Text(open_stream("odd", "sideways").into()))
        .await
        .expect("a send");

    match next(&mut socket, Duration::from_secs(5)).await {
        Some(ServerMessage::Error { code, .. }) => assert_eq!(code, "invalid_request"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a Whisper model and a sample recording"]
async fn a_caller_bounded_stream_ends_only_at_close() {
    let running = support::start_without_vad(2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    socket
        .send(Message::Text(open_stream("cb", "caller").into()))
        .await
        .expect("a send");

    match next(&mut socket, Duration::from_secs(5)).await {
        Some(ServerMessage::Accepted { boundaries, .. }) => {
            assert_eq!(boundaries.as_deref(), Some("caller"));
        }
        other => panic!("expected acceptance without a VAD model, got {other:?}"),
    }

    // Speech, a pause longer than any detector's default, then speech.
    let (samples, _) = support::spoken_sample();
    let pause = vec![0i16; 16_000 * 4];
    for piece in [&samples[..], &pause[..], &samples[..]] {
        for chunk in piece.chunks(1_600) {
            socket
                .send(Message::Binary(samples_to_bytes(chunk).into()))
                .await
                .expect("a send");
        }
    }
    let early = next(&mut socket, Duration::from_secs(2)).await;
    assert!(
        !matches!(early, Some(ServerMessage::Final { .. })),
        "the server ended the utterance on its own: {early:?}"
    );

    socket
        .send(Message::Text(close_stream("cb").into()))
        .await
        .expect("a send");
    let mut finals = Vec::new();
    while let Some(message) = next(&mut socket, Duration::from_secs(30)).await {
        if let ServerMessage::Final {
            audio_duration_ms, ..
        } = message
        {
            finals.push(audio_duration_ms);
        }
    }
    let heard = (samples.len() * 2 + pause.len()) as u64 / 16;
    assert_eq!(
        finals.len(),
        1,
        "one transcript for all of it, got {finals:?}"
    );
    assert!(
        finals[0].abs_diff(heard) <= 1,
        "{} ms of {heard}",
        finals[0]
    );
}
