//! The final transcript comes from recognising the finished utterance,
//! never from promoting the last interim. Asking for words while the
//! speaker talks must not change a single one of the words that end up
//! being the answer.

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language, Partial, SessionConfig};

const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio

fn transcriber() -> EdgeStt {
    let config = Config::local(support::shared_model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

/// Feeds a recording through a session, returning the transcripts and
/// the last interim the caller was shown before each one landed.
fn feed(samples: &[i16], live: bool) -> (Vec<String>, Vec<String>) {
    let stt = transcriber();
    let endpointing = EndpointConfig::new(support::vad_model_path());
    let config = match live {
        true => SessionConfig::new()
            .with_endpointing(endpointing)
            .with_live_interims()
            .with_interim_min_interval(Duration::from_millis(200)),
        false => SessionConfig::from(endpointing),
    };
    let mut session = stt.open_session(config).expect("a session");

    let mut finals = Vec::new();
    let mut interims = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let mut show = |partial: Partial| interims.push(partial.text);
        if let Some(transcript) = session.push(chunk, Some(&mut show)).expect("a push") {
            finals.push(transcript.text);
        }
    }
    let mut show = |partial: Partial| interims.push(partial.text);
    if let Some(transcript) = session.close(Some(&mut show)).expect("a close") {
        finals.push(transcript.text);
    }
    (finals, interims)
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn asking_for_early_words_does_not_change_the_answer() {
    let (samples, _) = support::spoken_sample();
    let (with_live, interims) = feed(&samples, true);
    let (without, _) = feed(&samples, false);

    assert_eq!(
        with_live, without,
        "the final transcript changed because the caller asked for early words"
    );
    assert!(
        !interims.is_empty(),
        "nothing arrived early, so this proved nothing"
    );
}

/// Against the words known to be in the recording, not against handing
/// the whole file over: the boundary detector trims the silence around
/// the speech, so those are not the same input to begin with.
#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn the_final_matches_the_pre_bounded_path() {
    let (samples, expected) = support::spoken_sample();
    let (finals, _) = feed(&samples, true);

    assert_eq!(finals.len(), 1, "expected one utterance, got {finals:?}");
    assert_eq!(finals[0].trim(), expected.trim());
}
