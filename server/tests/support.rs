//! Start a server on a free port for a test to talk to.

#![allow(dead_code)]

use std::sync::Arc;

use edge_stt_server::{Server, router};

pub struct Running {
    pub address: String,
    pub server: Arc<Server>,
}

impl Running {
    pub fn endpoint(&self) -> String {
        format!("ws://{}/api/v1/transcribe", self.address)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }
}

/// No model is loaded unless a test asks for one, so most of the
/// protocol can be checked without waiting on whisper.cpp.
pub async fn start(credential: Option<&str>, capacity: usize) -> Running {
    let server = Arc::new(Server::new(credential.map(str::to_string), None, capacity));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    let address = listener.local_addr().expect("an address").to_string();
    let app = router(Arc::clone(&server));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Running { address, server }
}
