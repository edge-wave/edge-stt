//! A credential is checked at the handshake, before any audio is read.

mod support;

use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

async fn knock(endpoint: &str, credential: Option<&str>) -> Result<(), u16> {
    let mut request = endpoint.into_client_request().expect("an address");
    if let Some(value) = credential {
        let header = HeaderValue::from_str(&format!("Bearer {value}")).expect("a header");
        request.headers_mut().insert("authorization", header);
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => Ok(()),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            Err(response.status().as_u16())
        }
        Err(_) => Err(0),
    }
}

#[tokio::test]
async fn no_credential_is_refused_when_one_was_configured() {
    let running = support::start(Some("open-sesame"), 2).await;
    assert_eq!(knock(&running.endpoint(), None).await, Err(401));
}

#[tokio::test]
async fn the_wrong_credential_is_refused() {
    let running = support::start(Some("open-sesame"), 2).await;
    assert_eq!(knock(&running.endpoint(), Some("guess")).await, Err(401));
}

#[tokio::test]
async fn the_right_credential_gets_in() {
    let running = support::start(Some("open-sesame"), 2).await;
    assert_eq!(
        knock(&running.endpoint(), Some("open-sesame")).await,
        Ok(())
    );
}

#[tokio::test]
async fn a_server_told_to_be_open_lets_anyone_in() {
    let running = support::start(None, 2).await;
    assert_eq!(knock(&running.endpoint(), None).await, Ok(()));
}
