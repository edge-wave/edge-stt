//! A continuous session counts against the same client-capacity limit
//! that already governs ordinary per-utterance requests -- no
//! separate limit for streaming (FR-014).

mod support;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn open_stream(request_id: &str) -> String {
    format!(
        r#"{{"type":"open_stream","request_id":"{request_id}","format":{{"sample_rate":16000,"channels":1,"sample_type":"i16"}},"want_partials":false}}"#
    )
}

#[tokio::test]
#[ignore = "needs a Whisper model and a VAD model"]
async fn a_continuous_session_holds_the_one_slot_a_capacity_of_one_has() {
    let running = support::start_streaming(1).await;

    let (mut first, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    first
        .send(Message::Text(open_stream("first").into()))
        .await
        .expect("a send");
    let reply = first.next().await.expect("a reply").expect("a frame");
    assert!(
        reply.into_text().expect("text").contains("accepted"),
        "the first session should start immediately"
    );

    let (mut second, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");
    second
        .send(Message::Text(open_stream("second").into()))
        .await
        .expect("a send");
    let waiting = tokio::time::timeout(std::time::Duration::from_millis(300), second.next()).await;
    assert!(
        waiting.is_err(),
        "a second stream must still be waiting behind the one the continuous session holds"
    );
}
