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
    start_with(credential, capacity, None, None, None).await
}

/// Loads a real model in the background and waits for it, plus a VAD
/// model, for tests that need continuous sessions to actually decode.
pub async fn start_streaming(capacity: usize) -> Running {
    start_streaming_with_threads(capacity, None).await
}

/// The same, with a cap on what one recognition may take, which is
/// what a server serving several live sessions at once needs.
pub async fn start_streaming_with_threads(capacity: usize, threads: Option<u16>) -> Running {
    let running = start_with(
        None,
        capacity,
        Some(model_path_env()),
        Some(vad_model_path_env()),
        threads,
    )
    .await;
    while !running.server.readiness.is_ready() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    running
}

/// The biggest ggml-*.bin under EDGE_STT_MODEL_DIR, the same rule
/// core's own test support uses.
pub fn model_path_env() -> std::path::PathBuf {
    let dir = std::env::var("EDGE_STT_MODEL_DIR")
        .expect("set EDGE_STT_MODEL_DIR to a directory holding a ggml Whisper model");
    let mut candidates: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|why| panic!("{dir}: {why}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension().is_some_and(|e| e == "bin")
                && path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("ggml-"))
        })
        .filter_map(|path| std::fs::metadata(&path).ok().map(|m| (m.len(), path)))
        .collect();
    candidates.sort();
    candidates
        .pop()
        .map(|(_, path)| path)
        .unwrap_or_else(|| panic!("no ggml-*.bin under {dir}"))
}

pub fn vad_model_path_env() -> std::path::PathBuf {
    std::env::var("EDGE_STT_VAD_MODEL")
        .map(std::path::PathBuf::from)
        .expect("set EDGE_STT_VAD_MODEL to a ggml Silero VAD file")
}

/// A recording of known speech, and the words in it -- same two
/// variables core's own test support reads.
pub fn spoken_sample() -> (Vec<i16>, String) {
    let path = std::env::var("EDGE_STT_SAMPLE_WAV")
        .expect("set EDGE_STT_SAMPLE_WAV to a 16 kHz mono 16-bit recording of speech");
    let expected = std::env::var("EDGE_STT_SAMPLE_TEXT")
        .expect("set EDGE_STT_SAMPLE_TEXT to the words spoken in EDGE_STT_SAMPLE_WAV");
    let mut reader = hound::WavReader::open(&path).unwrap_or_else(|why| panic!("{path}: {why}"));
    let samples = reader
        .samples::<i16>()
        .map(|s| s.expect("a readable sample"))
        .collect();
    (samples, expected)
}

async fn start_with(
    credential: Option<&str>,
    capacity: usize,
    model: Option<std::path::PathBuf>,
    vad_model: Option<std::path::PathBuf>,
    threads: Option<u16>,
) -> Running {
    let mut server = Server::new(credential.map(str::to_string), None, capacity, vad_model);
    if let Some(threads) = threads {
        server = server.with_threads(threads);
    }
    let server = Arc::new(server);
    if let Some(model) = &model {
        server.load_model_in_background(model);
    }
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
