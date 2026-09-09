//! Two callers asking for live words at once. A server that hands each
//! session every core makes them fight over every graph node, so this
//! is the test that the thread cap is wired through and works.

mod support;

use std::time::Duration;

use edge_stt_core::BackendChoice;
use edge_stt_core::wire::ServerMessage;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// Two threads each on a machine with more than that, so neither
/// session can claim the whole processor.
const THREADS_EACH: u16 = 2;

/// Generous on purpose. What it catches is a pair of sessions that
/// never finish, not a machine that is a little slower than another.
const PATIENCE: Duration = Duration::from_secs(120);

fn open_live(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":true,"live_interims":true,"interim_min_interval_ms":300}}"#
    )
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// One live session run to its final result, reported as how many
/// interims replaced what came before and what was heard in the end.
async fn live_session(endpoint: String, request_id: &str) -> (usize, Vec<String>) {
    let (mut socket, _) = tokio_tungstenite::connect_async(endpoint)
        .await
        .expect("a connection");
    socket
        .send(Message::Text(open_live(request_id).into()))
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
        .send(Message::Text(
            format!(r#"{{"type":"close_stream","request_id":"{request_id}"}}"#).into(),
        ))
        .await
        .expect("a send");

    let mut replacing = 0;
    let mut finals = Vec::new();
    while let Ok(Some(Ok(frame))) = tokio::time::timeout(PATIENCE, socket.next()).await {
        let Ok(text) = frame.into_text() else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<ServerMessage>(&text) else {
            continue;
        };
        match message {
            ServerMessage::Partial { kind, .. } if kind == "replace" => replacing += 1,
            ServerMessage::Final { text, .. } => {
                finals.push(text);
                break;
            }
            _ => {}
        }
    }
    (replacing, finals)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Whisper model and a VAD model"]
async fn two_callers_get_live_words_at_the_same_time() {
    let running = support::start_streaming_with_threads(4, Some(THREADS_EACH)).await;
    let endpoint = running.endpoint();

    let stt = running.server.transcriber().expect("a loaded model");
    let BackendChoice::Local(model) = &stt.config().backend else {
        panic!("a server serving live sessions recognises locally");
    };
    assert_eq!(
        model.thread_count(),
        THREADS_EACH,
        "the cap never reached the recogniser, so the sessions will contend"
    );

    let (first, second) = tokio::join!(
        live_session(endpoint.clone(), "first"),
        live_session(endpoint, "second"),
    );

    for (label, (replacing, finals)) in [("first", first), ("second", second)] {
        assert!(
            replacing > 0,
            "{label} asked for live words and was told nothing while speech ran"
        );
        assert_eq!(
            finals.len(),
            1,
            "{label} expected one utterance, got {finals:?}"
        );
    }
}
