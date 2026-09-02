//! Speech to text at the edge. Hand over a recording, get the words.
//!
//! The backend is chosen when the transcriber is built and nowhere
//! else, so moving between the device and a server changes one line.

pub mod backend;
pub mod cancel;
pub mod config;
pub mod endpoint;
pub mod error;
pub mod session;
pub mod transcript;
pub mod utterance;
#[cfg(feature = "remote")]
pub mod wire;

pub use cancel::CancelToken;
pub use config::{
    Accelerator, AudioFormat, BackendChoice, BackendKind, Config, Language, ModelSize, ModelSpec,
    RemoteConfig, SampleType, Secret,
};
pub use endpoint::EndpointConfig;
pub use error::{Error, Result};
pub use session::AudioSession;
pub use transcript::{Partial, PartialKind, Segment, Transcript};
pub use utterance::Utterance;

use backend::{Backend, Work};
#[cfg(feature = "streaming")]
use session::SessionSlot;

/// A built transcriber. Whether it decodes here or asks a server is
/// settled by the configuration it was built with.
pub struct EdgeStt {
    backend: Box<dyn Backend>,
    config: Config,
    #[cfg(feature = "streaming")]
    session_open: SessionSlot,
}

impl EdgeStt {
    /// Loads the model, so a missing or unusable one fails now rather
    /// than on the first spoken word.
    pub fn new(config: Config) -> Result<Self> {
        config.check()?;
        let backend = build_backend(&config)?;
        Ok(Self {
            backend,
            config,
            #[cfg(feature = "streaming")]
            session_open: SessionSlot::new(),
        })
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

    /// Loads a second, separate VAD model and hands back a session
    /// that decides its own utterance boundaries from whatever audio
    /// is pushed to it, then decodes each one through this same
    /// transcriber. At most one session may be open at a time
    /// (FR-015); it is released when the session closes or is dropped.
    #[cfg(feature = "streaming")]
    pub fn open_session(&self, config: EndpointConfig) -> Result<AudioSession<'_>> {
        config.check()?;
        let open_flag = self.session_open.claim()?;
        let endpointer = endpoint::whisper_vad::WhisperVad::load(&config).inspect_err(|_| {
            open_flag.store(false, std::sync::atomic::Ordering::Release);
        })?;
        Ok(AudioSession::new(
            self,
            Box::new(endpointer),
            self.config.max_duration,
            open_flag,
        ))
    }

    #[cfg(not(feature = "streaming"))]
    pub fn open_session(&self, _config: EndpointConfig) -> Result<AudioSession<'_>> {
        Err(Error::BackendUnavailable {
            backend: "streaming",
        })
    }

    pub(crate) fn run<'a>(
        &self,
        utterance: &Utterance<'_>,
        on_partial: Option<&'a mut dyn FnMut(Partial)>,
        cancel: &'a CancelToken,
    ) -> Result<Transcript> {
        utterance.check(self.config.max_duration)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut work = Work::new(cancel);
        work.timeout = self.config.timeout;
        work.on_partial = on_partial;
        self.backend.transcribe(utterance, &mut work)
    }
}

fn build_backend(config: &Config) -> Result<Box<dyn Backend>> {
    let primary = match &config.backend {
        BackendChoice::Local(model) => return build_local(model, config),
        BackendChoice::Remote(remote) => build_remote(remote, config)?,
    };
    match &config.fallback_to_local {
        None => Ok(primary),
        Some(model) => {
            let local = build_local(model, config)?;
            Ok(Box::new(backend::fallback::FallbackBackend::new(
                primary, local,
            )))
        }
    }
}

#[cfg(feature = "remote")]
fn build_remote(remote: &RemoteConfig, config: &Config) -> Result<Box<dyn Backend>> {
    Ok(Box::new(backend::remote::RemoteBackend::connect(
        remote, config,
    )?))
}

#[cfg(not(feature = "remote"))]
fn build_remote(_remote: &RemoteConfig, _config: &Config) -> Result<Box<dyn Backend>> {
    Err(Error::BackendUnavailable { backend: "remote" })
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
