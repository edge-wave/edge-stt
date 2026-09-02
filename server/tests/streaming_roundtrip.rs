//! A client streaming audio to the server, without pre-marking
//! boundaries, gets back the same transcript the on-device path
//! already proved for the same recording.

mod support;

use std::time::Duration;

use edge_stt_core::wire::ServerMessage;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn open_stream(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":false}}"#
    )
}

fn close_stream(request_id: &str) -> String {
    format!(r#"{{"type":"close_stream","request_id":"{request_id}"}}"#)
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[tokio::test]
#[ignore = "needs a Whisper model and a VAD model"]
async fn continuous_audio_over_the_wire_matches_the_on_device_text() {
    let running = support::start_streaming(2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");

    socket
        .send(Message::Text(open_stream("rt").into()))
        .await
        .expect("a send");

    let (samples, expected) = support::spoken_sample();
    for chunk in samples.chunks(1_600) {
        socket
            .send(Message::Binary(samples_to_bytes(chunk).into()))
            .await
            .expect("a send");
    }
    socket
        .send(Message::Text(close_stream("rt").into()))
        .await
        .expect("a send");

    let mut finals = Vec::new();
    while let Ok(Some(Ok(frame))) =
        tokio::time::timeout(Duration::from_secs(15), socket.next()).await
    {
        let Ok(text) = frame.into_text() else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<ServerMessage>(&text) else {
            continue;
        };
        if let ServerMessage::Final { text, .. } = message {
            finals.push(text);
        }
    }

    assert_eq!(finals.len(), 1, "expected one utterance, got {finals:?}");
    assert_eq!(finals[0].trim(), expected.trim());
}
