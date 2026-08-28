//! Speech to text at the edge. Hand over a recording, get the words.
//!
//! The backend is chosen when the transcriber is built and nowhere
//! else, so moving between the device and a server changes one line.

pub mod backend;
pub mod cancel;
pub mod config;
pub mod error;
pub mod transcript;
pub mod utterance;

pub use cancel::CancelToken;
pub use config::{
    Accelerator, AudioFormat, BackendChoice, BackendKind, Config, Language, ModelSize, ModelSpec,
    RemoteConfig, SampleType, Secret,
};
pub use error::{Error, Result};
pub use transcript::{Partial, PartialKind, Segment, Transcript};
pub use utterance::Utterance;

use backend::{Backend, Work};

/// A built transcriber. Whether it decodes here or asks a server is
/// settled by the configuration it was built with.
pub struct EdgeStt {
    backend: Box<dyn Backend>,
    config: Config,
}

impl EdgeStt {
    /// Loads the model, so a missing or unusable one fails now rather
    /// than on the first spoken word.
    pub fn new(config: Config) -> Result<Self> {
        config.check()?;
        let backend = build_backend(&config)?;
        Ok(Self { backend, config })
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.backend.kind()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn transcribe(&self, utterance: &Utterance<'_>) -> Result<Transcript> {
        let cancel = CancelToken::new();
        self.run(utterance, None, &cancel)
    }

    /// Partials arrive on this thread before the call returns, so the
    /// callback may borrow freely.
    pub fn transcribe_with(
        &self,
        utterance: &Utterance<'_>,
        on_partial: impl FnMut(Partial),
        cancel: &CancelToken,
    ) -> Result<Transcript> {
        let mut sink = on_partial;
        self.run(utterance, Some(&mut sink), cancel)
    }

    fn run<'a>(
        &self,
        utterance: &Utterance<'_>,
        on_partial: Option<&'a mut dyn FnMut(Partial)>,
        cancel: &'a CancelToken,
    ) -> Result<Transcript> {
        utterance.check(self.config.max_duration)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut work = Work {
            on_partial: if self.config.want_partials { on_partial } else { None },
            cancel,
            timeout: self.config.timeout,
        };
        self.backend.transcribe(utterance, &mut work)
    }
}

fn build_backend(config: &Config) -> Result<Box<dyn Backend>> {
    match &config.backend {
        BackendChoice::Local(model) => build_local(model, config),
        BackendChoice::Remote(_) => Err(Error::BackendUnavailable { backend: "remote" }),
    }
}

#[cfg(feature = "whisper")]
fn build_local(model: &ModelSpec, config: &Config) -> Result<Box<dyn Backend>> {
    let backend = backend::whisper::WhisperBackend::load(model, config)?;
    Ok(Box::new(backend))
}

#[cfg(not(feature = "whisper"))]
fn build_local(_model: &ModelSpec, _config: &Config) -> Result<Box<dyn Backend>> {
    Err(Error::BackendUnavailable { backend: "whisper" })
}
