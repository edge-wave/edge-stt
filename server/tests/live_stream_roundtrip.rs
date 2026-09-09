//! A client that asks for words while the speaker is still talking
//! gets them over the wire, marked as replacing what came before -- and
//! a client that asks for nothing new sees exactly what it always saw.

mod support;

use std::time::Duration;

use edge_stt_core::wire::ServerMessage;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn open_live(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":true,"live_interims":true,"interim_min_interval_ms":200}}"#
    )
}

fn open_plain(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":true}}"#
    )
}

fn close_stream(request_id: &str) -> String {
    format!(r#"{{"type":"close_stream","request_id":"{request_id}"}}"#)
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// What the server said, as (kind, text) for every interim and the
/// text of every finished utterance.
async fn run(open: String) -> (Vec<(String, String)>, Vec<String>) {
    let running = support::start_streaming(2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");

    socket
        .send(Message::Text(open.into()))
        .await
        .expect("a send");

    let (samples, _) = support::spoken_sample();
    for chunk in samples.chunks(1_600) {
        socket
            .send(Message::Binary(samples_to_bytes(chunk).into()))
            .await
            .expect("a send");
    }
    socket
        .send(Message::Text(close_stream("live").into()))
        .await
        .expect("a send");

    let mut interims = Vec::new();
    let mut finals = Vec::new();
    while let Ok(Some(Ok(frame))) =
        tokio::time::timeout(Duration::from_secs(30), socket.next()).await
    {
        let Ok(text) = frame.into_text() else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<ServerMessage>(&text) else {
            continue;
        };
        match message {
            ServerMessage::Partial { kind, text, .. } => interims.push((kind, text)),
            ServerMessage::Final { text, .. } => {
                finals.push(text);
                break;
            }
            _ => {}
        }
    }
    (interims, finals)
}

#[tokio::test]
#[ignore = "needs a Whisper model and a VAD model"]
async fn words_arrive_over_the_wire_marked_as_replacing() {
    let (interims, finals) = run(open_live("live")).await;

    assert!(!interims.is_empty(), "nothing arrived while speech ran");
    assert!(
        interims.iter().any(|(kind, _)| kind == "replace"),
        "a pass re-recognises everything, so it replaces: {interims:?}"
    );
    assert_eq!(finals.len(), 1, "expected one utterance, got {finals:?}");
}

#[tokio::test]
#[ignore = "needs a Whisper model and a VAD model"]
async fn a_client_that_asks_for_nothing_new_sees_what_it_always_saw() {
    let (interims, finals) = run(open_plain("plain")).await;

    assert!(
        interims.iter().all(|(kind, _)| kind == "append"),
        "the existing path only ever appends: {interims:?}"
    );
    assert_eq!(finals.len(), 1, "expected one utterance, got {finals:?}");
}

#[tokio::test]
#[ignore = "needs a Whisper model and a VAD model"]
async fn asking_for_words_without_asking_for_partials_is_refused() {
    let running = support::start_streaming(2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");

    let open = r#"{"type":"open_stream","request_id":"bad","format":{"sample_rate":16000,"channels":1,"sample_type":"i16"},"want_partials":false,"live_interims":true}"#;
    socket
        .send(Message::Text(open.into()))
        .await
        .expect("a send");

    let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("a reply")
        .expect("a frame")
        .expect("a frame");
    let text = frame.into_text().expect("text");
    let message = serde_json::from_str::<ServerMessage>(&text).expect("a message");
    assert!(
        matches!(message, ServerMessage::Error { .. }),
        "asking for results nobody would be sent should be refused, got {message:?}"
    );
}
