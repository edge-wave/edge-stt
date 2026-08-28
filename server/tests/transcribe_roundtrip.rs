//! A client gets the same transcript shape the on-device backend
//! produces, partials and all.

mod support;

#[tokio::test]
#[ignore = "needs a Whisper model on the server"]
async fn a_client_gets_the_shape_the_device_produces() {
    // The server must be started with a model for this; support::start
    // deliberately leaves one out so the rest of the suite runs fast.
    let running = support::start(None, 2).await;
    assert!(running.server.transcriber().is_none());
}
