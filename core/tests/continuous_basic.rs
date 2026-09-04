//! Feeding a recording in small, live-sized pieces produces the same
//! transcript a caller who cut it by hand would have gotten, and
//! feeding two sentences with a clear pause between them produces two
//! transcripts, not one merged incorrectly.

#![cfg(feature = "streaming")]

mod support;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language};

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

/// Feeds `samples` through a session in fixed-size pieces, as if
/// arriving live, and collects the text of each detected utterance.
fn feed_in_chunks(samples: &[i16]) -> Vec<String> {
    const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio
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
    if let Some(transcript) = session.close(None).expect("a clean close") {
        texts.push(transcript.text);
    }
    texts
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn one_sentence_fed_in_pieces_matches_the_pre_bounded_path() {
    let (samples, expected) = support::spoken_sample();
    let texts = feed_in_chunks(&samples);
    assert_eq!(texts.len(), 1, "expected one utterance, got {texts:?}");
    assert_eq!(texts[0].trim(), expected.trim());
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn two_sentences_separated_by_a_clear_pause_produce_two_transcripts() {
    let (sentence, expected) = support::spoken_sample();
    let mut samples = sentence.clone();
    samples.extend(support::silence(4.0)); // past the 3s default tolerance
    samples.extend(&sentence);

    let texts = feed_in_chunks(&samples);
    assert_eq!(
        texts.len(),
        2,
        "expected two separate utterances, not one merged or one lost, got {texts:?}"
    );
    assert_eq!(texts[0].trim(), expected.trim());
    assert_eq!(texts[1].trim(), expected.trim());
}
