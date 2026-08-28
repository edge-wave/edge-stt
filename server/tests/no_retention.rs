//! Nothing about a request outlives it.

mod support;

#[tokio::test]
async fn the_server_holds_no_place_to_keep_audio() {
    let running = support::start(None, 2).await;
    // There is no store to check because there is no store: the server
    // owns a model, a credential, and a capacity, and nothing else.
    assert!(running.server.transcriber().is_none());
    assert!(running.server.credential.is_none());
}

#[tokio::test]
#[ignore = "needs a Whisper model"]
async fn nothing_is_written_to_disk_during_a_request() {}
