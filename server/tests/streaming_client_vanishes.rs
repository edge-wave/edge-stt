//! A dropped connection (not a clean close_stream) discards the
//! in-progress utterance and leaves the server serving everyone else.

mod support;

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;

const OPEN: &str = r#"{"type":"open_stream","request_id":"gone","format":{"sample_rate":16000,"channels":1,"sample_type":"i16"},"want_partials":false}"#;

#[tokio::test]
async fn dropping_mid_stream_leaves_the_server_serving() {
    let running = support::start(None, 2).await;

    {
        let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
            .await
            .expect("a connection");
        socket
            .send(Message::Text(OPEN.into()))
            .await
            .expect("a send");
        socket
            .send(Message::Binary(vec![0u8; 32_000].into()))
            .await
            .expect("a send");
        // No close_stream, and the socket goes away here.
    }

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let again = tokio_tungstenite::connect_async(running.endpoint()).await;
    assert!(again.is_ok(), "another client must still be served");
    assert_eq!(
        reqwest::get(running.url("/healthz"))
            .await
            .expect("a reply")
            .status(),
        200
    );
}

#[tokio::test]
async fn a_stream_before_the_model_is_up_is_told_so_rather_than_left_waiting() {
    let running = support::start(None, 2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");

    socket
        .send(Message::Text(OPEN.into()))
        .await
        .expect("a send");

    use futures_util::StreamExt;
    let reply = socket.next().await.expect("a reply").expect("a frame");
    let text = reply.into_text().expect("text");
    assert!(text.contains("model_unavailable"), "{text}");
}

fn open_live(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":true,"live_interims":true,"interim_min_interval_ms":300,"boundaries":"caller"}}"#
    )
}

/// What a pass costs does not depend on what was said, so noise keeps
/// a decoder as busy as speech would.
fn noise(count: usize) -> Vec<u8> {
    let mut state: u32 = 0x2545_f491;
    (0..count)
        .flat_map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            ((state >> 20) as i16 - 2048).to_le_bytes()
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs a Whisper model"]
async fn a_vanished_stream_stops_decoding_and_frees_its_slot() {
    use futures_util::StreamExt;
    use std::time::{Duration, Instant};

    let running = support::start_without_vad(1).await;

    let (mut first, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    first
        .send(Message::Text(open_live("first").into()))
        .await
        .expect("a send");
    let reply = first.next().await.expect("a reply").expect("a frame");
    assert!(reply.into_text().expect("text").contains("accepted"));

    // At the pace it is spoken, which passes over a growing utterance
    // soon cannot keep up with, so audio is still queued when it goes.
    for _ in 0..150 {
        first
            .send(Message::Binary(noise(1_600).into()))
            .await
            .expect("a send");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(first);

    let vanished = Instant::now();
    let (mut second, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    second
        .send(Message::Text(open_live("second").into()))
        .await
        .expect("a send");
    let reply = tokio::time::timeout(Duration::from_secs(3), second.next())
        .await
        .expect("the slot should come back once the vanished stream stops decoding")
        .expect("a reply")
        .expect("a frame");
    assert!(reply.into_text().expect("text").contains("accepted"));
    eprintln!("slot came back after {:?}", vanished.elapsed());
}
