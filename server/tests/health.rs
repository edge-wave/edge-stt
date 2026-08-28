//! Alive and ready are different questions, because loading a large
//! model takes long enough that one answer would get a working server
//! killed.

mod support;

#[tokio::test]
async fn alive_answers_before_the_model_has_loaded() {
    let running = support::start(None, 2).await;
    let response = reqwest::get(running.url("/healthz"))
        .await
        .expect("a reply");
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn ready_stays_unready_until_the_model_is_up() {
    let running = support::start(None, 2).await;
    let response = reqwest::get(running.url("/readyz")).await.expect("a reply");
    assert_eq!(response.status(), 503, "nothing has been loaded yet");
    assert!(!running.server.readiness.is_ready());
}
