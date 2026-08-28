//! A client that walks away must not take the server with it, nor
//! leave anything decoding for nobody.

mod support;

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;

const START: &str = r#"{"type":"start","request_id":"gone","format":{"sample_rate":16000,"channels":1,"sample_type":"i16"},"want_partials":false}"#;

#[tokio::test]
async fn dropping_mid_request_leaves_the_server_serving() {
    let running = support::start(None, 2).await;

    {
        let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
            .await
            .expect("a connection");
        socket
            .send(Message::Text(START.into()))
            .await
            .expect("a send");
        socket
            .send(Message::Binary(vec![0u8; 32_000].into()))
            .await
            .expect("a send");
        // No end frame, and the socket goes away here.
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
async fn a_request_before_the_model_is_up_is_told_so_rather_than_left_waiting() {
    let running = support::start(None, 2).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(running.endpoint())
        .await
        .expect("a connection");

    socket
        .send(Message::Text(START.into()))
        .await
        .expect("a send");
    socket
        .send(Message::Text(
            r#"{"type":"end","request_id":"gone"}"#.into(),
        ))
        .await
        .expect("a send");

    use futures_util::StreamExt;
    let reply = socket.next().await.expect("a reply").expect("a frame");
    let text = reply.into_text().expect("text");
    assert!(text.contains("model_unavailable"), "{text}");
}
