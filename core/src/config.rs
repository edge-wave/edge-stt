//! What a caller sets before transcribing, and the shape of the audio
//! it will hand over.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};

/// How one sample is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    I16,
    F32,
}

impl fmt::Display for SampleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleType::I16 => write!(f, "16-bit integer"),
            SampleType::F32 => write!(f, "32-bit float"),
        }
    }
}

/// The shape of a block of samples. The same three fields edge-ear
/// carries, so a value crosses between the two without conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_type: SampleType,
}

impl AudioFormat {
    pub const fn new(sample_rate: u32, channels: u16, sample_type: SampleType) -> Self {
        Self {
            sample_rate,
            channels,
            sample_type,
        }
    }

    /// 16 kHz mono 16-bit -- the only shape transcription accepts,
    /// fixed by Whisper's architecture across every model size.
    pub const fn mono_16k() -> Self {
        Self::new(16_000, 1, SampleType::I16)
    }

    /// Reject anything that is not the one supported shape, naming both
    /// sides so the caller knows what to change.
    pub fn check_transcribable(&self) -> Result<()> {
        let expected = Self::mono_16k();
        if *self == expected {
            Ok(())
        } else {
            Err(Error::UnsupportedAudio {
                expected,
                got: *self,
            })
        }
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::mono_16k()
    }
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let channels = match self.channels {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            n => format!("{n} channels"),
        };
        write!(f, "{} Hz {channels} {}", self.sample_rate, self.sample_type)
    }
}

/// Where the model runs. An accelerator the build lacks is an error,
/// never a silent fall back to the processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accelerator {
    #[default]
    Cpu,
    Metal,
    Cuda,
    Vulkan,
}

impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Accelerator::Cpu => "cpu",
            Accelerator::Metal => "metal",
            Accelerator::Cuda => "cuda",
            Accelerator::Vulkan => "vulkan",
        };
        f.write_str(name)
    }
}

/// Which rung of the model ladder a file is meant to be. Carried for
/// diagnostics; the file itself is what decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSize {
    Tiny,
    Base,
    Small,
    Medium,
    LargeV3,
}

impl fmt::Display for ModelSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ModelSize::Tiny => "tiny",
            ModelSize::Base => "base",
            ModelSize::Small => "small",
            ModelSize::Medium => "medium",
            ModelSize::LargeV3 => "large-v3",
        };
        f.write_str(name)
    }
}

/// The model file to load, and how hard to work at it. Which size to
/// run is the integrator's choice, and no combination is refused here.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub path: PathBuf,
    pub size_hint: Option<ModelSize>,
    pub threads: Option<u16>,
    pub accelerator: Option<Accelerator>,
}

impl ModelSpec {
    pub fn at(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            size_hint: None,
            threads: None,
            accelerator: None,
        }
    }

    pub fn with_size(mut self, size: ModelSize) -> Self {
        self.size_hint = Some(size);
        self
    }

    pub fn with_threads(mut self, threads: u16) -> Self {
        self.threads = Some(threads);
        self
    }

    pub fn with_accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
    }

    /// Every physical core unless the caller said otherwise.
    pub fn thread_count(&self) -> u16 {
        self.threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(4, |n| n.get().min(u16::MAX as usize) as u16)
        })
    }
}

/// A credential. Prints as a placeholder so it cannot reach a log by
/// way of a debug line.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// A language tag, as the model spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language(String);

impl Language {
    pub fn new(tag: impl Into<String>) -> Self {
        Self(tag.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a remote transcriber sends audio. There is no default
/// endpoint, so a misconfigured build cannot reach a stranger.
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    pub endpoint: String,
    pub credential: Option<Secret>,
    pub connect_timeout: Duration,
}

impl RemoteConfig {
    pub fn at(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            credential: None,
            connect_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_credential(mut self, credential: impl Into<String>) -> Self {
        self.credential = Some(Secret::new(credential));
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn check(&self) -> Result<()> {
        if self.endpoint.trim().is_empty() {
            return Err(Error::InvalidValue {
                setting: "endpoint",
                expected: "a websocket address the operator chose".to_string(),
                got: "empty".to_string(),
            });
        }
        Ok(())
    }
}

/// Which backend a transcriber was built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    Remote,
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendKind::Local => f.write_str("local"),
            BackendKind::Remote => f.write_str("remote"),
        }
    }
}

/// Local or remote, with what each needs.
#[derive(Debug, Clone)]
pub enum BackendChoice {
    Local(ModelSpec),
    Remote(RemoteConfig),
}

/// Everything set before transcribing. The only place local or remote
/// is decided; nothing downstream of it differs.
#[derive(Debug, Clone)]
pub struct Config {
    pub backend: BackendChoice,
    pub fallback_to_local: Option<ModelSpec>,
    pub language: Option<Language>,
    pub timeout: Option<Duration>,
    pub max_duration: Duration,
}

impl Config {
    pub fn local(model: ModelSpec) -> Self {
        Self::with_backend(BackendChoice::Local(model))
    }

    pub fn remote(remote: RemoteConfig) -> Self {
        Self::with_backend(BackendChoice::Remote(remote))
    }

    fn with_backend(backend: BackendChoice) -> Self {
        Self {
            backend,
            fallback_to_local: None,
            language: None,
            timeout: None,
            max_duration: Duration::from_secs(300),
        }
    }

    /// Only taken when asked for, so a remote failure surfaces
    /// untouched by default.
    pub fn with_fallback_to_local(mut self, model: ModelSpec) -> Self {
        self.fallback_to_local = Some(model);
        self
    }

    pub fn with_language(mut self, language: Language) -> Self {
        self.language = Some(language);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_max_duration(mut self, max: Duration) -> Self {
        self.max_duration = max;
        self
    }

    pub fn check(&self) -> Result<()> {
        if self.max_duration.is_zero() {
            return Err(Error::InvalidValue {
                setting: "max_duration",
                expected: "greater than zero".to_string(),
                got: "zero".to_string(),
            });
        }
        match &self.backend {
            BackendChoice::Local(_) => Ok(()),
            BackendChoice::Remote(remote) => remote.check(),
        }
    }
}
