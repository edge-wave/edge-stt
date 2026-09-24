//! A caller that does not know where speech stops leaves it to the
//! server's detector, and every utterance it finds comes back in order.

mod support;

use std::time::Duration;

use edge_stt_core::{EndpointConfig, Error, SessionConfig};

fn detector() -> SessionConfig {
    let endpointing = EndpointConfig::new().with_pause_tolerance(Duration::from_millis(1_500));
    SessionConfig::new().with_endpointing(endpointing)
}

#[test]
#[ignore = "needs a Whisper model, a VAD model, and a sample recording"]
fn two_sentences_apart_come_back_as_two_finals_in_order() {
    let (_runtime, running) = support::serving(support::start_streaming(2));
    let stt = support::client(&running);
    let mut session = stt.open_session(detector()).expect("a session");

    let (spoken, _) = support::spoken_sample();
    let mut recording = spoken.clone();
    recording.extend(std::iter::repeat_n(0, 16_000 * 4));
    recording.extend(&spoken);

    let mut finals = Vec::new();
    for chunk in recording.chunks(1_600) {
        if let Some(transcript) = session.push(chunk, None).expect("a push") {
            finals.push(transcript);
        }
    }
    finals.extend(session.close(None).expect("a close"));

    assert_eq!(finals.len(), 2, "{finals:?}");
    // Punctuation is the model's to vary between two hearings.
    assert_eq!(
        words(&finals[0].text),
        words(&finals[1].text),
        "the same sentence twice"
    );
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect()
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_server_with_no_detector_refuses_a_detector_session() {
    let (_runtime, running) = support::serving(support::start_without_vad(2));
    let stt = support::client(&running);
    match stt.open_session(detector()) {
        Err(Error::InvalidValue { setting, .. }) => assert_eq!(setting, "endpointing"),
        Err(other) => panic!("expected the detector to be named, got {other}"),
        Ok(_) => panic!("a server with no VAD model accepted a detector session"),
    }
}
