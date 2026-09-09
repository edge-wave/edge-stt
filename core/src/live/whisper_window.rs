//! Words while someone is still speaking, from the recogniser already
//! loaded: the utterance so far is recognised again on every pass, and
//! the encoder is bounded so the silence it pads with is not paid for.

use std::path::PathBuf;

use whisper_rs::{FullParams, SamplingStrategy, WhisperState, convert_integer_to_float_audio};

use crate::backend::LiveDecoder;
use crate::backend::whisper::collect;
use crate::config::Language;
use crate::error::{Error, Result};

const SAMPLE_RATE: usize = 16_000;

/// A second of audio occupies fifty encoder positions. Twice that was
/// safe in both languages at every buffer length measured, and the
/// audio's own count was exactly where the words fell apart.
const POSITIONS_PER_SECOND: usize = 100;

/// Never bound tighter than this. Bounds this small were not measured,
/// and being under the floor costs several times what it saves.
const NARROWEST: i32 = 200;

/// What the recogniser encodes whatever it is handed.
const WIDEST: i32 = 1500;

/// Half a second invented words in both languages; three quarters was
/// already right. A second takes that with a margin, and costs a
/// caller far less than the wait this exists to remove.
const MINIMUM_SPEECH: usize = SAMPLE_RATE;

pub struct WhisperWindow {
    state: WhisperState,
    /// What the caller asked for, which no pass may override.
    configured: Option<Language>,
    /// What the first pass of this utterance decided, kept so the text
    /// cannot change language mid-sentence for no reason a caller can
    /// act on. Cleared when the utterance ends, never the one above.
    pinned: Option<Language>,
    threads: i32,
    model_path: PathBuf,
}

impl WhisperWindow {
    pub(crate) fn new(
        state: WhisperState,
        configured: Option<Language>,
        threads: i32,
        model_path: PathBuf,
    ) -> Self {
        Self {
            state,
            configured,
            pinned: None,
            threads,
            model_path,
        }
    }

    fn language(&self) -> Option<&Language> {
        self.configured.as_ref().or(self.pinned.as_ref())
    }
}

impl LiveDecoder for WhisperWindow {
    fn push(&mut self, utterance_so_far: &[i16]) -> Result<Option<String>> {
        if utterance_so_far.len() < MINIMUM_SPEECH {
            return Ok(None);
        }

        let mut audio = vec![0.0f32; utterance_so_far.len()];
        convert_integer_to_float_audio(utterance_so_far, &mut audio).map_err(|why| {
            Error::InvalidValue {
                setting: "samples",
                expected: "16-bit mono audio".to_string(),
                got: why.to_string(),
            }
        })?;

        // Held in a local because the parameters borrow it for the pass.
        let language = self.language().map(|l| l.as_str().to_string());
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.threads);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_language(language.as_deref());
        params.set_audio_ctx(bound_for(utterance_so_far.len()));

        self.state
            .full(params, &audio)
            .map_err(|why| Error::ModelUnusable {
                path: self.model_path.clone(),
                why: why.to_string(),
            })?;

        let decoded = collect(&self.state)?;
        if self.configured.is_none() && self.pinned.is_none() {
            self.pinned = Some(decoded.language.clone());
        }

        let text: String = decoded.segments.iter().map(|s| s.text.as_str()).collect();
        let text = settled(text.trim());
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(text.to_string()))
    }

    fn reset(&mut self) {
        self.pinned = None;
    }
}

/// The closing word is cut wherever the buffer ends, so it is held back
/// until a later pass hears past it. Timing cannot find it: a short
/// buffer comes back as one segment whose end is the cut itself.
pub fn settled(text: &str) -> &str {
    // Nothing to hold back: one word is either the whole hypothesis or
    // a script that does not put spaces between them.
    match text.rsplit_once(char::is_whitespace) {
        Some((kept, _)) => kept.trim_end(),
        None => text,
    }
}

/// How much of the thirty-second canvas to encode for this much audio.
/// Half of it is deliberately left as margin: with none the recogniser
/// leaves the shape it was trained on and starts repeating itself.
pub fn bound_for(samples: usize) -> i32 {
    let positions = (samples.saturating_mul(POSITIONS_PER_SECOND) / SAMPLE_RATE) as i32;
    positions.clamp(NARROWEST, WIDEST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bound_leaves_half_the_canvas_as_margin() {
        assert_eq!(bound_for(4 * SAMPLE_RATE), 400);
        assert_eq!(bound_for(8 * SAMPLE_RATE), 800);
    }

    #[test]
    fn the_word_the_buffer_cut_is_not_shown() {
        assert_eq!(
            settled("And so my fellow Americans ask"),
            "And so my fellow Americans"
        );
        assert_eq!(settled("오늘 날씨가 아주 맑고 정"), "오늘 날씨가 아주 맑고");
    }

    /// A hypothesis of one word is either all there is or a script that
    /// writes without spaces, and holding either back says nothing.
    #[test]
    fn a_single_word_is_shown_as_it_is() {
        assert_eq!(settled("안녕하세요"), "안녕하세요");
        assert_eq!(settled(""), "");
    }

    #[test]
    fn the_bound_stays_between_the_floor_and_the_whole_window() {
        assert_eq!(bound_for(SAMPLE_RATE / 2), NARROWEST);
        assert_eq!(bound_for(60 * SAMPLE_RATE), WIDEST);
    }
}
