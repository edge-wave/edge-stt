//! Transcription server for edge-stt clients. Runs the same backend
//! the library runs, so the two cannot answer differently.

pub mod auth;
pub mod capacity;
pub mod health;
pub mod session;
pub mod ws;

use std::path::Path;
use std::sync::{Arc, OnceLock};

use axum::Router;
use axum::routing::get;
use edge_stt_core::{Config, EdgeStt, Language, ModelSpec};

use capacity::Capacity;
use health::Readiness;

pub struct Server {
    stt: OnceLock<Arc<EdgeStt>>,
    pub credential: Option<String>,
    /// The language this server was started with. A client asking for
    /// another one is refused rather than quietly given this.
    pub language: Option<String>,
    pub capacity: Capacity,
    pub readiness: Readiness,
    /// Needed only for continuous (open_stream) sessions; absent means
    /// this server refuses them rather than guessing a default.
    pub vad_model: Option<std::path::PathBuf>,
}

impl Server {
    pub fn new(
        credential: Option<String>,
        language: Option<String>,
        capacity: usize,
        vad_model: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            stt: OnceLock::new(),
            credential,
            language,
            capacity: Capacity::new(capacity),
            readiness: Readiness::default(),
            vad_model,
        }
    }

    pub fn transcriber(&self) -> Option<Arc<EdgeStt>> {
        self.stt.get().cloned()
    }

    /// Loading happens off the accepting thread, which is why there are
    /// two health routes rather than one.
    pub fn load_model_in_background(self: &Arc<Self>, model: &Path) {
        let server = Arc::clone(self);
        let model = model.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let mut config = Config::local(ModelSpec::at(&model));
            if let Some(tag) = server.language.clone() {
                config = config.with_language(Language::new(tag));
            }
            match EdgeStt::new(config) {
                Ok(stt) => {
                    let _ = server.stt.set(Arc::new(stt));
                    server.readiness.mark_ready();
                    log::info!("model loaded from {}", model.display());
                }
                Err(why) => log::error!("{why}"),
            }
        });
    }
}

pub fn router(server: Arc<Server>) -> Router {
    let readiness = server.readiness.clone();
    Router::new()
        .route("/healthz", get(health::alive))
        .route("/readyz", get(move || health::ready(readiness.clone())))
        .route("/api/v1/transcribe", get(ws::upgrade))
        .with_state(server)
}
