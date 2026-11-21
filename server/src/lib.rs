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
use edge_stt_core::{Accelerator, Config, EdgeStt, Language, ModelSpec};

use capacity::Capacity;
use health::Readiness;

pub struct Server {
    stt: OnceLock<Arc<EdgeStt>>,
    pub credential: Option<String>,
    /// The language this server was started with. A client asking for
    /// another one is refused rather than quietly given this.
    pub language: Option<String>,
    /// Languages accepted besides `language`, detected per finished utterance.
    pub other_languages: Vec<String>,
    /// A small model that picks among the languages instead of the large one.
    pub language_model: Option<std::path::PathBuf>,
    pub capacity: Capacity,
    pub readiness: Readiness,
    /// Needed only for continuous (open_stream) sessions; absent means
    /// this server refuses them rather than guessing a default.
    pub vad_model: Option<std::path::PathBuf>,
    /// How many threads one recognition may use. Every session decodes
    /// with this, so it is what stops live callers oversubscribing.
    pub threads: Option<u16>,
    /// Where the model runs. A build without that accelerator fails to
    /// load rather than quietly decoding on the processor.
    pub accelerator: Accelerator,
    /// Candidates a finished utterance is decoded with; None is greedy.
    pub beam_size: Option<u8>,
    /// Words every finished utterance is told came before it.
    pub prompt: Option<String>,
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
            other_languages: Vec::new(),
            language_model: None,
            capacity: Capacity::new(capacity),
            readiness: Readiness::default(),
            vad_model,
            threads: None,
            accelerator: Accelerator::Cpu,
            beam_size: None,
            prompt: None,
        }
    }

    /// Caps what one recognition takes, so a machine serving several
    /// live sessions divides its cores instead of contending for them.
    pub fn with_threads(mut self, threads: u16) -> Self {
        self.threads = Some(threads);
        self
    }

    /// Runs the model on this accelerator instead of the processor.
    pub fn with_accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = accelerator;
        self
    }

    /// Decodes each finished utterance with this many candidates.
    pub fn with_beam_size(mut self, beam_size: u8) -> Self {
        self.beam_size = Some(beam_size);
        self
    }

    /// Hands this text to each finished utterance as what came before.
    pub fn with_prompt(mut self, prompt: String) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Accepts these languages too, detecting which one each utterance is in.
    pub fn with_other_languages(mut self, languages: Vec<String>) -> Self {
        self.other_languages = languages;
        self
    }

    /// Picks the language with this small model, which costs far less than the large one.
    pub fn with_language_model(mut self, path: std::path::PathBuf) -> Self {
        self.language_model = Some(path);
        self
    }

    /// Whether a client asking for this language is served.
    pub fn serves(&self, asked: &str) -> bool {
        self.language.as_deref() == Some(asked) || self.other_languages.iter().any(|l| l == asked)
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
            let mut spec = ModelSpec::at(&model).with_accelerator(server.accelerator);
            if let Some(threads) = server.threads {
                spec = spec.with_threads(threads);
            }
            if let Some(beam_size) = server.beam_size {
                spec = spec.with_beam_size(beam_size);
            }
            if let Some(prompt) = server.prompt.clone() {
                spec = spec.with_prompt(prompt);
            }
            if let Some(path) = &server.language_model {
                spec = spec.with_language_model(path);
            }
            let mut config = Config::local(spec);
            if let Some(tag) = server.language.clone() {
                config = config.with_language(Language::new(tag));
                config = config.with_other_languages(
                    server
                        .other_languages
                        .iter()
                        .cloned()
                        .map(Language::new)
                        .collect(),
                );
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
