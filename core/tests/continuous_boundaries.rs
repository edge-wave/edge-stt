//! A brief non-speech sound inside otherwise-continuous speech must
//! not split the utterance in two, so long as it stays under the
//! configured pause tolerance -- but a pause that exceeds it does
//! split.

#![cfg(feature = "streaming")]

mod support;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language};

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

fn feed_in_chunks(samples: &[i16]) -> Vec<String> {
    const SIMULATED_CHUNK: usize = 1_600;
    let stt = transcriber();
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");
    let mut texts = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        if let Some(transcript) = session.push(chunk, None).expect("a push") {
            texts.push(transcript.text);
        }
    }
    if let Some(transcript) = session.close().expect("a clean close") {
        texts.push(transcript.text);
    }
    texts
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_brief_pause_under_the_default_tolerance_does_not_split_the_utterance() {
    let (sentence, _expected) = support::spoken_sample();
    let mut samples = sentence.clone();
    samples.extend(support::silence(1.0)); // well under the 3s default
    samples.extend(&sentence);

    let texts = feed_in_chunks(&samples);
    assert_eq!(
        texts.len(),
        1,
        "a 1-second pause is under the default tolerance and must not split the utterance, got {texts:?}"
    );
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_pause_past_the_default_tolerance_does_split_the_utterance() {
    let (sentence, _expected) = support::spoken_sample();
    let mut samples = sentence.clone();
    samples.extend(support::silence(4.0)); // past the 3s default
    samples.extend(&sentence);

    let texts = feed_in_chunks(&samples);
    assert_eq!(
        texts.len(),
        2,
        "a 4-second pause is past the default tolerance and must split the utterance, got {texts:?}"
    );
}
