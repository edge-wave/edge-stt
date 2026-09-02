//! Boundary detection through whisper.cpp's own built-in VAD support
//! (Silero today, but nothing here depends on that specifically).
//! `WhisperVadContext` answers "where is the speech in this whole
//! buffer", not "is this one new chunk still speech" -- so this wraps
//! it in a trailing buffer that gets re-scored as audio arrives,
//! rather than passing chunks straight through.

use std::path::PathBuf;

use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use super::{EndpointConfig, Endpointer};
use crate::error::{Error, Result};

/// The only shape `Utterance` accepts, so it is the only rate this
/// needs to reason about.
const SAMPLE_RATE: u32 = 16_000;

pub struct WhisperVad {
    ctx: WhisperVadContext,
    vad_params: WhisperVadParams,
    pause_tolerance_secs: f64,
    model_path: PathBuf,
    /// Audio not yet resolved into a finished utterance or discarded
    /// as silence that turned out to lead nowhere.
    pending: Vec<i16>,
}

impl WhisperVad {
    pub fn load(config: &EndpointConfig) -> Result<Self> {
        if !config.vad_model.is_file() {
            return Err(Error::ModelMissing {
                path: config.vad_model.clone(),
            });
        }

        let ctx = WhisperVadContext::new(
            &config.vad_model.to_string_lossy(),
            WhisperVadContextParams::default(),
        )
        .map_err(|why| Error::ModelUnusable {
            path: config.vad_model.clone(),
            why: why.to_string(),
        })?;

        let mut vad_params = WhisperVadParams::new();
        let min_silence_ms = config.pause_tolerance.as_millis().min(i32::MAX as u128) as i32;
        vad_params.set_min_silence_duration(min_silence_ms);

        Ok(Self {
            ctx,
            vad_params,
            pause_tolerance_secs: config.pause_tolerance.as_secs_f64(),
            model_path: config.vad_model.clone(),
            pending: Vec::new(),
        })
    }

    fn score_pending(&mut self) -> Result<whisper_rs::WhisperVadSegments> {
        let floats = to_float(&self.pending);
        self.ctx
            .segments_from_samples(self.vad_params, &floats)
            .map_err(|why| Error::ModelUnusable {
                path: self.model_path.clone(),
                why: why.to_string(),
            })
    }

    /// A stretch of pure silence can only ever affect the *next*
    /// boundary decision through its most recent `pause_tolerance`
    /// worth -- anything older than that can be forgotten so a long
    /// quiet stretch does not grow this buffer without bound.
    fn forget_stale_silence(&mut self) {
        let cap = ((self.pause_tolerance_secs * 2.0) * SAMPLE_RATE as f64) as usize;
        if self.pending.len() > cap {
            let excess = self.pending.len() - cap;
            self.pending.drain(..excess);
        }
    }
}

impl Endpointer for WhisperVad {
    fn push(&mut self, samples: &[i16]) -> Result<Option<Vec<i16>>> {
        self.pending.extend_from_slice(samples);

        let segments = self.score_pending()?;
        let Some(last) = segments.last() else {
            self.forget_stale_silence();
            return Ok(None);
        };

        let elapsed_secs = self.pending.len() as f64 / SAMPLE_RATE as f64;
        let speech_ended_secs = f64::from(last.end) / 100.0;
        if elapsed_secs - speech_ended_secs < self.pause_tolerance_secs {
            return Ok(None);
        }

        let boundary = ((speech_ended_secs * SAMPLE_RATE as f64) as usize).min(self.pending.len());
        Ok(Some(self.pending.drain(..boundary).collect()))
    }

    fn take_remainder(&mut self) -> Option<Vec<i16>> {
        if self.pending.is_empty() {
            return None;
        }
        let had_speech = self
            .score_pending()
            .is_ok_and(|segments| segments.num_segments() > 0);

        if had_speech {
            Some(std::mem::take(&mut self.pending))
        } else {
            self.pending.clear();
            None
        }
    }
}

fn to_float(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|s| f32::from(*s) / 32768.0).collect()
}
