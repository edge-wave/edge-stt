//! Python binding for edge-stt. Binds the core crate directly, adding
//! nothing but the shapes Python expects.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use edge_stt_core::{
    CancelToken, Config, EdgeStt as Core, Error, Language, ModelSpec, Partial, Transcript,
    Utterance,
};

create_exception!(edge_stt, EdgeSttError, PyException);
create_exception!(edge_stt, UnsupportedAudioError, EdgeSttError);
create_exception!(edge_stt, AudioTooLongError, EdgeSttError);
create_exception!(edge_stt, ModelMissingError, EdgeSttError);
create_exception!(edge_stt, ModelUnusableError, EdgeSttError);
create_exception!(edge_stt, InsufficientResourcesError, EdgeSttError);
create_exception!(edge_stt, NetworkError, EdgeSttError);
create_exception!(edge_stt, CredentialRejectedError, EdgeSttError);
create_exception!(edge_stt, ServerAtCapacityError, EdgeSttError);
create_exception!(edge_stt, ServerError, EdgeSttError);
create_exception!(edge_stt, TimeoutError, EdgeSttError);
create_exception!(edge_stt, CancelledError, EdgeSttError);
create_exception!(edge_stt, InvalidValueError, EdgeSttError);
create_exception!(edge_stt, BackendUnavailableError, EdgeSttError);

/// One class per cause, so `except NetworkError` works without anyone
/// having to read a message.
fn to_py(error: Error) -> PyErr {
    let message = error.to_string();
    match error {
        Error::UnsupportedAudio { .. } => UnsupportedAudioError::new_err(message),
        Error::AudioTooLong { .. } => AudioTooLongError::new_err(message),
        Error::ModelMissing { .. } => ModelMissingError::new_err(message),
        Error::ModelUnusable { .. } => ModelUnusableError::new_err(message),
        Error::InsufficientResources { .. } => InsufficientResourcesError::new_err(message),
        Error::Network { .. } => NetworkError::new_err(message),
        Error::CredentialRejected { .. } => CredentialRejectedError::new_err(message),
        Error::ServerAtCapacity { .. } => ServerAtCapacityError::new_err(message),
        Error::ServerError { .. } => ServerError::new_err(message),
        Error::Timeout { .. } => TimeoutError::new_err(message),
        Error::Cancelled => CancelledError::new_err(message),
        Error::InvalidValue { .. } => InvalidValueError::new_err(message),
        Error::BackendUnavailable { .. } => BackendUnavailableError::new_err(message),
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct Segment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f32,
}

#[pymethods]
impl Segment {
    fn __repr__(&self) -> String {
        format!(
            "Segment({:?}, {:.2}-{:.2})",
            self.text, self.start, self.end
        )
    }
}

#[pyclass(name = "Transcript", frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyTranscript {
    pub text: String,
    pub segments: Vec<Segment>,
    pub language: String,
    pub confidence: f32,
    pub audio_duration: f64,
    pub processing_time: f64,
    pub backend: String,
}

#[pymethods]
impl PyTranscript {
    /// Under one means the hardware is keeping up with a speaker.
    fn real_time_factor(&self) -> f64 {
        if self.audio_duration <= 0.0 {
            0.0
        } else {
            self.processing_time / self.audio_duration
        }
    }

    fn __repr__(&self) -> String {
        format!("Transcript({:?}, language={:?})", self.text, self.language)
    }
}

fn to_python(transcript: &Transcript) -> PyTranscript {
    PyTranscript {
        text: transcript.text.clone(),
        segments: transcript
            .segments
            .iter()
            .map(|s| Segment {
                text: s.text.clone(),
                start: s.start.as_secs_f64(),
                end: s.end.as_secs_f64(),
                confidence: s.confidence,
            })
            .collect(),
        language: transcript.language.to_string(),
        confidence: transcript.confidence,
        audio_duration: transcript.audio_duration.as_secs_f64(),
        processing_time: transcript.processing_time.as_secs_f64(),
        backend: transcript.backend.to_string(),
    }
}

#[pyclass(name = "Partial", frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyPartial {
    pub seq: u32,
    pub text: String,
    pub replaces: bool,
}

#[pymethods]
impl PyPartial {
    fn __repr__(&self) -> String {
        format!("Partial({}, {:?})", self.seq, self.text)
    }
}

fn to_python_partial(partial: &Partial) -> PyPartial {
    PyPartial {
        seq: partial.seq,
        text: partial.text.clone(),
        replaces: partial.kind == edge_stt_core::PartialKind::Replace,
    }
}

/// What `transcribe_stream` hands back: partials as they arrive, then
/// the finished transcript on `result`.
#[pyclass]
pub struct PartialStream {
    partials: Mutex<Option<Receiver<Partial>>>,
    worker: Option<std::thread::JoinHandle<Result<Transcript, Error>>>,
    finished: Option<PyTranscript>,
    cancel: CancelToken,
}

#[pymethods]
impl PartialStream {
    fn __iter__(this: PyRef<'_, Self>) -> PyRef<'_, Self> {
        this
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyPartial>> {
        loop {
            let waited = {
                let held = self.partials.lock().expect("a lock nobody poisons");
                let Some(partials) = held.as_ref() else {
                    return Ok(None);
                };
                partials.recv_timeout(Duration::from_millis(20))
            };
            match waited {
                Ok(partial) => return Ok(Some(to_python_partial(&partial))),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Let the rest of the program run, and let Ctrl-C
                    // through, while the decoder works.
                    py.detach(std::thread::yield_now);
                    py.check_signals()?;
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    *self.partials.lock().expect("a lock nobody poisons") = None;
                    self.collect(py)?;
                    return Ok(None);
                }
            }
        }
    }

    /// The finished transcript, once the partials have run out.
    #[getter]
    fn result(&mut self, py: Python<'_>) -> PyResult<Option<PyTranscript>> {
        let running = self
            .partials
            .lock()
            .expect("a lock nobody poisons")
            .is_some();
        if self.finished.is_none() && !running {
            self.collect(py)?;
        }
        Ok(self.finished.clone())
    }

    fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl PartialStream {
    fn collect(&mut self, py: Python<'_>) -> PyResult<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let outcome = py.detach(|| worker.join());
        match outcome {
            Ok(Ok(transcript)) => {
                self.finished = Some(to_python(&transcript));
                Ok(())
            }
            Ok(Err(why)) => Err(to_py(why)),
            Err(_) => Err(EdgeSttError::new_err("the decoder stopped unexpectedly")),
        }
    }
}

/// A caller-fed, open-ended audio stream. `EdgeStt.open_session`
/// returns one; where `transcribe`/`transcribe_stream` take a whole
/// recording, this takes samples as they arrive and decides for
/// itself where one utterance ends.
#[pyclass(name = "Session")]
pub struct PySession {
    /// Keeps the model alive for as long as the session is open --
    /// `session` unsafely borrows through this Arc's stable address.
    _core: Arc<Core>,
    session: Mutex<edge_stt_core::AudioSession<'static>>,
}

#[pymethods]
impl PySession {
    /// Feeds one piece of newly-captured audio. Returns the finished
    /// utterance the moment one is detected, otherwise `None`. If
    /// `on_partial` is given, it is called -- on this thread, with the
    /// GIL briefly reacquired each time -- for interim results while
    /// this specific push decodes one.
    #[pyo3(signature = (samples, sample_rate = 16_000, on_partial = None))]
    fn push(
        &self,
        py: Python<'_>,
        samples: &Bound<'_, PyAny>,
        sample_rate: u32,
        on_partial: Option<Py<PyAny>>,
    ) -> PyResult<Option<PyTranscript>> {
        let audio = read_samples(samples, sample_rate)?;
        py.detach(|| {
            let mut session = self.session.lock().expect("a lock nobody poisons");
            match &on_partial {
                Some(callback) => {
                    let mut sink = |partial: Partial| {
                        let handed = to_python_partial(&partial);
                        Python::attach(|py| {
                            if let Err(why) = callback.call1(py, (handed,)) {
                                why.print(py);
                            }
                        });
                    };
                    session.push(&audio, Some(&mut sink))
                }
                None => session.push(&audio, None),
            }
        })
        .map(|found| found.as_ref().map(to_python))
        .map_err(to_py)
    }

    /// Finalizes and returns whatever utterance was in progress. A
    /// second call returns `None` rather than raising. The utterance
    /// this finalizes decodes like any other push -- `on_partial`
    /// works the same way here too.
    #[pyo3(signature = (on_partial = None))]
    fn close(
        &self,
        py: Python<'_>,
        on_partial: Option<Py<PyAny>>,
    ) -> PyResult<Option<PyTranscript>> {
        py.detach(|| {
            let mut session = self.session.lock().expect("a lock nobody poisons");
            match &on_partial {
                Some(callback) => {
                    let mut sink = |partial: Partial| {
                        let handed = to_python_partial(&partial);
                        Python::attach(|py| {
                            if let Err(why) = callback.call1(py, (handed,)) {
                                why.print(py);
                            }
                        });
                    };
                    session.close(Some(&mut sink))
                }
                None => session.close(None),
            }
        })
        .map(|found| found.as_ref().map(to_python))
        .map_err(to_py)
    }

    fn __enter__(this: PyRef<'_, Self>) -> PyRef<'_, Self> {
        this
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.close(py, None)?;
        Ok(false)
    }
}

#[pyclass(name = "EdgeStt")]
pub struct PyEdgeStt {
    core: Arc<Core>,
}

#[pymethods]
impl PyEdgeStt {
    /// Loads the model now, so a missing file is an error here rather
    /// than on the first spoken word.
    #[new]
    #[pyo3(signature = (model, language = None, timeout = None))]
    fn new(model: &str, language: Option<&str>, timeout: Option<f64>) -> PyResult<Self> {
        let mut config = Config::local(ModelSpec::at(model));
        if let Some(tag) = language {
            config = config.with_language(Language::new(tag));
        }
        if let Some(seconds) = timeout {
            config = config.with_timeout(Duration::from_secs_f64(seconds));
        }
        let core = Core::new(config).map_err(to_py)?;
        Ok(Self {
            core: Arc::new(core),
        })
    }

    #[getter]
    fn backend(&self) -> String {
        self.core.backend_kind().to_string()
    }

    /// The GIL is released around decoding, so the rest of the program
    /// keeps running.
    #[pyo3(signature = (samples, sample_rate = 16_000))]
    fn transcribe(
        &self,
        py: Python<'_>,
        samples: &Bound<'_, PyAny>,
        sample_rate: u32,
    ) -> PyResult<PyTranscript> {
        let audio = read_samples(samples, sample_rate)?;
        let core = Arc::clone(&self.core);
        let transcript = py
            .detach(move || core.transcribe(&Utterance::mono_16k(&audio)))
            .map_err(to_py)?;
        Ok(to_python(&transcript))
    }

    /// Yields partials as they are decoded; the transcript is on the
    /// stream's `result` once it is exhausted.
    #[pyo3(signature = (samples, sample_rate = 16_000))]
    fn transcribe_stream(
        &self,
        samples: &Bound<'_, PyAny>,
        sample_rate: u32,
    ) -> PyResult<PartialStream> {
        let audio = read_samples(samples, sample_rate)?;
        let core = Arc::clone(&self.core);
        let cancel = CancelToken::new();
        let inside = cancel.clone();
        let (sender, receiver) = channel();

        let worker = std::thread::spawn(move || {
            core.transcribe_with(
                &Utterance::mono_16k(&audio),
                |partial| {
                    let _ = sender.send(partial);
                },
                &inside,
            )
        });

        Ok(PartialStream {
            partials: Mutex::new(Some(receiver)),
            worker: Some(worker),
            finished: None,
            cancel,
        })
    }

    /// Opens a continuous session against a second, separate VAD
    /// model file -- push samples to it as they arrive. Pass
    /// `caller_boundaries=True` instead of a VAD model when the caller
    /// already knows where speech stops: an utterance then ends only at
    /// `close` or at the maximum duration. Ask for
    /// `live_interims` to also hear words while the speaker is still
    /// talking, which costs repeated recognition and so is not implied
    /// by passing a callback.
    #[pyo3(signature = (
        vad_model = None,
        pause_tolerance = None,
        live_interims = false,
        interim_min_interval = None,
        caller_boundaries = false,
    ))]
    fn open_session(
        &self,
        vad_model: Option<&str>,
        pause_tolerance: Option<f64>,
        live_interims: bool,
        interim_min_interval: Option<f64>,
        caller_boundaries: bool,
    ) -> PyResult<PySession> {
        let mut config = edge_stt_core::SessionConfig::new();
        match (vad_model, caller_boundaries) {
            (Some(_), true) => {
                return Err(InvalidValueError::new_err(
                    "pass vad_model or caller_boundaries, not both",
                ));
            }
            (Some(path), false) => {
                let mut endpointing = edge_stt_core::EndpointConfig::new(path);
                if let Some(seconds) = pause_tolerance {
                    endpointing =
                        endpointing.with_pause_tolerance(Duration::from_secs_f64(seconds));
                }
                config = config.with_endpointing(endpointing);
            }
            (None, true) => config = config.with_caller_boundaries(),
            (None, false) => {}
        }
        if live_interims {
            config = config.with_live_interims();
        }
        if let Some(seconds) = interim_min_interval {
            config = config.with_interim_min_interval(Duration::from_secs_f64(seconds));
        }
        // SAFETY: `_core` below is a clone of `self.core`, keeping the
        // `EdgeStt` this borrows from alive for as long as the session
        // exists -- the Arc's address does not move once allocated.
        let core: &'static Core = unsafe { &*Arc::as_ptr(&self.core) };
        let session = core.open_session(config).map_err(to_py)?;
        Ok(PySession {
            _core: Arc::clone(&self.core),
            session: Mutex::new(session),
        })
    }

    fn __enter__(this: PyRef<'_, Self>) -> PyRef<'_, Self> {
        this
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, _args: &Bound<'_, PyAny>) -> bool {
        false
    }

    fn __repr__(&self) -> String {
        format!(
            "EdgeStt(backend={:?})",
            self.core.backend_kind().to_string()
        )
    }
}

/// Accepts bytes, a sequence of ints, or anything with tobytes() --
/// a numpy int16 array among them. The limited API this wheel is built
/// against has no buffer protocol, so there is no zero-copy path.
fn read_samples(value: &Bound<'_, PyAny>, sample_rate: u32) -> PyResult<Vec<i16>> {
    if sample_rate != 16_000 {
        return Err(UnsupportedAudioError::new_err(format!(
            "audio is {sample_rate} Hz, and only 16000 Hz mono 16-bit can be transcribed"
        )));
    }
    if let Ok(bytes) = value.cast::<PyBytes>() {
        return Ok(from_le_bytes(bytes.as_bytes()));
    }
    if let Ok(raw) = value.call_method0("tobytes")
        && let Ok(bytes) = raw.cast::<PyBytes>()
    {
        return Ok(from_le_bytes(bytes.as_bytes()));
    }
    value.extract::<Vec<i16>>().map_err(|_| {
        InvalidValueError::new_err(
            "samples must be bytes, a sequence of ints, or something with tobytes(), \
             such as a numpy int16 array",
        )
    })
}

fn from_le_bytes(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[pymodule]
fn edge_stt(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyEdgeStt>()?;
    module.add_class::<PyTranscript>()?;
    module.add_class::<Segment>()?;
    module.add_class::<PyPartial>()?;
    module.add_class::<PartialStream>()?;
    module.add_class::<PySession>()?;
    for (name, class) in [
        ("EdgeSttError", module.py().get_type::<EdgeSttError>()),
        (
            "UnsupportedAudioError",
            module.py().get_type::<UnsupportedAudioError>(),
        ),
        (
            "AudioTooLongError",
            module.py().get_type::<AudioTooLongError>(),
        ),
        (
            "ModelMissingError",
            module.py().get_type::<ModelMissingError>(),
        ),
        (
            "ModelUnusableError",
            module.py().get_type::<ModelUnusableError>(),
        ),
        (
            "InsufficientResourcesError",
            module.py().get_type::<InsufficientResourcesError>(),
        ),
        ("NetworkError", module.py().get_type::<NetworkError>()),
        (
            "CredentialRejectedError",
            module.py().get_type::<CredentialRejectedError>(),
        ),
        (
            "ServerAtCapacityError",
            module.py().get_type::<ServerAtCapacityError>(),
        ),
        ("ServerError", module.py().get_type::<ServerError>()),
        ("TimeoutError", module.py().get_type::<TimeoutError>()),
        ("CancelledError", module.py().get_type::<CancelledError>()),
        (
            "InvalidValueError",
            module.py().get_type::<InvalidValueError>(),
        ),
        (
            "BackendUnavailableError",
            module.py().get_type::<BackendUnavailableError>(),
        ),
    ] {
        module.add(name, class)?;
    }
    Ok(())
}
