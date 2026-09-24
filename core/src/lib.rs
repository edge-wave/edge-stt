//! Speech to text at the edge. Hand over a recording, get the words.
//!
//! The backend is chosen when the transcriber is built and nowhere
//! else, so moving between the device and a server changes one line.

pub mod backend;
pub mod cancel;
pub mod config;
pub mod endpoint;
pub mod error;
pub mod live;
pub mod session;
pub mod transcript;
pub mod utterance;
#[cfg(feature = "remote")]
pub mod wire;

pub use cancel::CancelToken;
pub use config::{
    Accelerator, AudioFormat, BackendChoice, BackendKind, Boundaries, Config,
    DEFAULT_INTERIM_MIN_INTERVAL, Language, ModelSize, ModelSpec, RemoteConfig, SampleType, Secret,
    SessionConfig,
};
pub use endpoint::EndpointConfig;
pub use error::{Error, Result};
pub use session::AudioSession;
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

    /// Hands back a session that ends utterances where its `Boundaries`
    /// say. Each session owns its own state, so several may be open at
    /// once against one loaded recognizer.
    ///
    /// Accepts an `EndpointConfig` exactly as it always has, and also a
    /// `SessionConfig` for a caller who wants words while speech
    /// continues or decides the boundaries itself.
    #[cfg(feature = "streaming")]
    pub fn open_session(&self, config: impl Into<SessionConfig>) -> Result<AudioSession<'_>> {
        let config = config.into();
        config.check()?;

        if !self.backend.capabilities().hosts_sessions {
            return self.open_local(&*self.backend, &config);
        }
        if matches!(config.boundaries, Boundaries::Recognizer) {
            return Err(no_boundaries());
        }
        match self.backend.open_hosted(&config)? {
            backend::Opened::Hosted(stream) => Ok(AudioSession::hosted(
                self,
                stream,
                &config,
                self.config.max_duration,
            )),
            backend::Opened::Local(fallen_back) => self.open_local(fallen_back, &config),
        }
    }

    /// A session whose boundaries are found, and whose utterances are
    /// decoded, on this device by `recognizer`.
    #[cfg(feature = "streaming")]
    fn open_local<'a>(
        &'a self,
        recognizer: &'a dyn Backend,
        config: &SessionConfig,
    ) -> Result<AudioSession<'a>> {
        let can = recognizer.capabilities();
        if config.live_interims && !can.live_interims {
            return Err(Error::InvalidValue {
                setting: "live_interims",
                expected: "a recognizer that can produce results while speech continues"
                    .to_string(),
                got: format!("{}, which cannot", recognizer.kind()),
            });
        }

        let endpointer = match (&config.boundaries, can.self_endpointing) {
            (Boundaries::Detector(endpointing), _) => Some(load_detector(endpointing)?),
            (Boundaries::Recognizer, true) | (Boundaries::Caller, _) => None,
            (Boundaries::Recognizer, false) => return Err(no_boundaries()),
        };

        // Opened for boundaries as well as for early words: a
        // recognizer that endpoints itself has to hear the audio to do it.
        let finds_boundaries = matches!(config.boundaries, Boundaries::Recognizer);
        let live = match config.live_interims || finds_boundaries {
            true => Some(recognizer.open_live()?),
            false => None,
        };

        Ok(AudioSession::new(
            self,
            recognizer,
            endpointer,
            live,
            config,
            self.config.max_duration,
        ))
    }

    #[cfg(not(feature = "streaming"))]
    pub fn open_session(&self, _config: impl Into<SessionConfig>) -> Result<AudioSession<'_>> {
        Err(Error::BackendUnavailable {
            backend: "streaming",
        })
    }

    #[cfg(feature = "streaming")]
    pub(crate) fn backend(&self) -> &dyn Backend {
        &*self.backend
    }

    pub(crate) fn run(
        &self,
        utterance: &Utterance<'_>,
        on_partial: Option<&mut dyn FnMut(Partial)>,
        cancel: &CancelToken,
    ) -> Result<Transcript> {
        self.run_on(&*self.backend, utterance, on_partial, cancel)
    }

    /// The same limits and timeout whichever backend decodes, so a
    /// session that fell back to this device is held to them too.
    pub(crate) fn run_on(
        &self,
        backend: &dyn Backend,
        utterance: &Utterance<'_>,
        on_partial: Option<&mut dyn FnMut(Partial)>,
        cancel: &CancelToken,
    ) -> Result<Transcript> {
        // Distinct lifetimes on Work let this take a caller's existing
        // `on_partial` borrow alongside a cancel token this call owns.
        utterance.check(self.config.max_duration)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut work = Work::new(cancel);
        work.timeout = self.config.timeout;
        work.on_partial = on_partial;
        backend.transcribe(utterance, &mut work)
    }
}

#[cfg(test)]
impl EdgeStt {
    /// Wraps a backend handed over directly, so a test can exercise a
    /// session without a model file anywhere on disk.
    pub(crate) fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            config: Config::local(ModelSpec::at("not read")),
        }
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

#[cfg(feature = "streaming")]
fn no_boundaries() -> Error {
    Error::InvalidValue {
        setting: "endpointing",
        expected: "a boundary-detection model, a recognizer that finds its own boundaries, \
                   or boundaries left to the caller"
            .to_string(),
        got: "none of them".to_string(),
    }
}

#[cfg(all(feature = "streaming", feature = "whisper"))]
fn load_detector(config: &EndpointConfig) -> Result<Box<dyn endpoint::Endpointer>> {
    Ok(Box::new(endpoint::whisper_vad::WhisperVad::load(config)?))
}

/// A build without the on-device recognizer has no device detector
/// either, and never opens a device session to need one.
#[cfg(all(feature = "streaming", not(feature = "whisper")))]
fn load_detector(_config: &EndpointConfig) -> Result<Box<dyn endpoint::Endpointer>> {
    Err(Error::BackendUnavailable { backend: "whisper" })
}
