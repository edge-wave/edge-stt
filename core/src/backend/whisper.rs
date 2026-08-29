//! The on-device backend: whisper.cpp, reached through whisper-rs.

use std::borrow::Cow;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use whisper_rs::{
    FullParams, SamplingStrategy, SegmentCallbackData, WhisperContext, WhisperContextParameters,
    WhisperState, convert_integer_to_float_audio, get_lang_str,
};

use super::{Backend, Work};
use crate::config::{Accelerator, BackendKind, Config, Language, ModelSpec};
use crate::error::{Error, Result};
use crate::transcript::{Partial, Segment, Transcript};
use crate::utterance::Utterance;

/// What a loaded model turns out to be. Printed by the probe example
/// and pinned by a test, because a model fed the wrong shape is quiet.
#[derive(Debug, Clone)]
pub struct ModelFacts {
    pub multilingual: bool,
    pub vocabulary: i32,
    pub audio_context: i32,
    pub description: String,
}

pub struct WhisperBackend {
    context: WhisperContext,
    model: ModelSpec,
    language: Option<Language>,
}

impl WhisperBackend {
    /// Loads the model now, so a missing or unusable file is an error
    /// here rather than on the first spoken word.
    pub fn load(model: &ModelSpec, config: &Config) -> Result<Self> {
        if !model.path.is_file() {
            return Err(Error::ModelMissing {
                path: model.path.clone(),
            });
        }

        let accelerator = model.accelerator.unwrap_or_default();
        check_accelerator(accelerator)?;

        let mut parameters = WhisperContextParameters::new();
        parameters.use_gpu(accelerator != Accelerator::Cpu);

        let context = WhisperContext::new_with_params(&model.path, parameters).map_err(|why| {
            Error::ModelUnusable {
                path: model.path.clone(),
                why: why.to_string(),
            }
        })?;

        if let Some(language) = &config.language
            && !context.is_multilingual()
            && language.as_str() != "en"
        {
            return Err(Error::ModelUnusable {
                path: model.path.clone(),
                why: format!("speaks only English, but {language} was asked for"),
            });
        }

        Ok(Self {
            context,
            model: model.clone(),
            language: config.language.clone(),
        })
    }

    pub fn facts(&self) -> ModelFacts {
        ModelFacts {
            multilingual: self.context.is_multilingual(),
            vocabulary: self.context.n_vocab(),
            audio_context: self.context.n_audio_ctx(),
            description: self
                .context
                .model_type_readable_str()
                .map_or_else(|_| "unknown".to_string(), str::to_string),
        }
    }

    fn reported_language(&self) -> Language {
        self.language
            .clone()
            .unwrap_or_else(|| Language::new("auto"))
    }
}

impl Backend for WhisperBackend {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_>) -> Result<Transcript> {
        let started = Instant::now();
        if work.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let audio_duration = utterance.duration();
        if utterance.is_below_silence_floor() {
            return Ok(Transcript::empty(
                self.reported_language(),
                audio_duration,
                started.elapsed(),
                BackendKind::Local,
            ));
        }

        let mut audio = vec![0.0f32; utterance.samples.len()];
        convert_integer_to_float_audio(utterance.samples, &mut audio).map_err(|why| {
            Error::InvalidValue {
                setting: "samples",
                expected: "16-bit mono audio".to_string(),
                got: why.to_string(),
            }
        })?;

        let timed_out = Arc::new(AtomicBool::new(false));
        let outcome = self.decode(&audio, work, Arc::clone(&timed_out), started);

        // An ending that was asked for wins over whatever the decoder
        // said on the way out; a stopped encode is what it looks like.
        if work.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if timed_out.load(Ordering::Relaxed) {
            let limit = work.timeout.unwrap_or_default();
            return Err(Error::Timeout { limit });
        }
        let decoded = outcome?;

        let text: String = decoded.segments.iter().map(|s| s.text.as_str()).collect();
        let confidence = average_confidence(&decoded.segments);
        Ok(Transcript {
            text: text.trim().to_string(),
            segments: decoded.segments,
            language: decoded.language,
            confidence,
            audio_duration,
            processing_time: started.elapsed(),
            backend: BackendKind::Local,
        })
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Local
    }
}

/// What the abort callback reads. Boxed so its address survives the
/// decode, and read only from whisper.cpp's own threads.
struct Stop {
    cancel: crate::cancel::CancelToken,
    deadline: Option<Instant>,
    timed_out: Arc<AtomicBool>,
}

unsafe extern "C" fn should_stop(user_data: *mut c_void) -> bool {
    let Some(stop) = (unsafe { user_data.cast::<Stop>().as_ref() }) else {
        return false;
    };
    if stop.cancel.is_cancelled() {
        return true;
    }
    match stop.deadline {
        Some(at) if Instant::now() >= at => {
            stop.timed_out.store(true, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

struct Decoded {
    segments: Vec<Segment>,
    language: Language,
}

impl WhisperBackend {
    /// whisper.cpp only takes an owning callback, so segments come back
    /// over a channel that this thread drains while the worker decodes.
    fn decode(
        &self,
        audio: &[f32],
        work: &mut Work<'_>,
        timed_out: Arc<AtomicBool>,
        started: Instant,
    ) -> Result<Decoded> {
        let (sender, receiver) = mpsc::channel::<SegmentCallbackData>();

        let wants_partials = work.wants_partials();
        let cancel = work.cancel.clone();
        let deadline = work.timeout.map(|limit| started + limit);
        let threads = i32::from(self.model.thread_count());
        let language = self.language.as_ref().map(|l| l.as_str().to_string());
        let context = &self.context;
        let model_path = self.model.path.clone();

        let outcome = std::thread::scope(|scope| {
            let worker = scope.spawn(move || -> Result<Decoded> {
                let mut state = context.create_state().map_err(|why| Error::ModelUnusable {
                    path: model_path.clone(),
                    why: why.to_string(),
                })?;

                let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                params.set_n_threads(threads);
                params.set_print_special(false);
                params.set_print_progress(false);
                params.set_print_realtime(false);
                params.set_print_timestamps(false);
                // None asks whisper.cpp to detect and then transcribe;
                // its detect_language flag detects and stops there.
                params.set_language(language.as_deref());
                let mut stop = Box::new(Stop {
                    cancel,
                    deadline,
                    timed_out,
                });
                // whisper-rs's safe wrapper aborts the encoder even when
                // the closure says otherwise, so the trampoline is ours.
                unsafe {
                    params.set_abort_callback(Some(should_stop));
                    let handle: *mut Stop = &raw mut *stop;
                    params.set_abort_callback_user_data(handle.cast::<c_void>());
                }
                if wants_partials {
                    params.set_segment_callback_safe(move |data| {
                        let _ = sender.send(data);
                    });
                }

                let decoded = state
                    .full(params, audio)
                    .map_err(|why| Error::ModelUnusable {
                        path: model_path.clone(),
                        why: why.to_string(),
                    });
                // Outlives the decode by construction; dropping it any
                // earlier would leave the callback a dangling handle.
                drop(stop);
                decoded?;
                collect(&state)
            });

            // Draining until the channel closes would wait forever:
            // whisper.cpp owns the callback holding the sender, and
            // does not give it back. The worker finishing is the end.
            let mut seq = 0u32;
            let mut hand_over = |work: &mut Work<'_>, data: SegmentCallbackData| {
                // Filtered here as well as at the end, so joining the
                // partials still gives the final text exactly.
                if !is_annotation(&data.text) {
                    work.emit(partial_from(&mut seq, data));
                }
            };
            loop {
                match receiver.recv_timeout(Duration::from_millis(20)) {
                    Ok(data) => hand_over(work, data),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if worker.is_finished() {
                            while let Ok(data) = receiver.try_recv() {
                                hand_over(work, data);
                            }
                            break;
                        }
                    }
                }
            }
            worker.join()
        });

        match outcome {
            Ok(decoded) => decoded,
            Err(_) => Err(Error::ModelUnusable {
                path: self.model.path.clone(),
                why: "the decoder thread stopped unexpectedly".to_string(),
            }),
        }
    }
}

fn collect(state: &WhisperState) -> Result<Decoded> {
    let mut segments = Vec::new();
    for index in 0..state.full_n_segments() {
        let Some(segment) = state.get_segment(index) else {
            continue;
        };
        let text = match segment.to_str_lossy() {
            Ok(Cow::Borrowed(text)) => text.to_string(),
            Ok(Cow::Owned(text)) => text,
            Err(_) => continue,
        };
        if is_annotation(&text) {
            continue;
        }
        segments.push(Segment {
            text,
            start: centiseconds(segment.start_timestamp()),
            end: centiseconds(segment.end_timestamp()),
            confidence: 1.0 - segment.no_speech_probability(),
        });
    }

    let language = get_lang_str(state.full_lang_id_from_state()).unwrap_or("auto");
    Ok(Decoded {
        segments,
        language: Language::new(language),
    })
}

/// whisper.cpp writes what it heard instead of words when there were
/// none: "[BLANK_AUDIO]", "(music)". Those are notes, not speech.
fn is_annotation(text: &str) -> bool {
    let trimmed = text.trim();
    let wrapped = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'));
    wrapped && !trimmed[1..trimmed.len() - 1].contains(['[', ']', '(', ')'])
}

fn partial_from(seq: &mut u32, data: SegmentCallbackData) -> Partial {
    let segment = Segment {
        text: data.text.clone(),
        start: centiseconds(data.start_timestamp),
        end: centiseconds(data.end_timestamp),
        confidence: 1.0,
    };
    let partial = Partial::append(*seq, data.text, Some(segment));
    *seq += 1;
    partial
}

fn centiseconds(value: i64) -> Duration {
    Duration::from_millis(value.max(0) as u64 * 10)
}

/// Whisper reports how sure it is that a stretch held no speech; the
/// other side of that is how sure it is about the words.
fn average_confidence(segments: &[Segment]) -> f32 {
    if segments.is_empty() {
        return 0.0;
    }
    segments.iter().map(|s| s.confidence).sum::<f32>() / segments.len() as f32
}

fn check_accelerator(accelerator: Accelerator) -> Result<()> {
    let available = match accelerator {
        Accelerator::Cpu => true,
        Accelerator::Metal => cfg!(feature = "metal"),
        Accelerator::Cuda => cfg!(feature = "cuda"),
        Accelerator::Vulkan => cfg!(feature = "vulkan"),
    };
    if available {
        Ok(())
    } else {
        Err(Error::InvalidValue {
            setting: "accelerator",
            expected: "one this build carries".to_string(),
            got: accelerator.to_string(),
        })
    }
}
