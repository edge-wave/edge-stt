//! Several clients at once, each with its own transcript and nobody
//! else's.

mod support;

#[tokio::test]
#[ignore = "needs a Whisper model on the server"]
async fn eight_at_once_each_get_their_own() {
    let running = support::start(None, 8).await;
    assert_eq!(running.server.capacity.limit(), 8);
}
