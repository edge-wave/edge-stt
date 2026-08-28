//! Short audio is silence. Long audio is refused, never truncated.

use std::time::Duration;

use edge_stt_core::{Error, Utterance};

fn seconds(count: f32) -> Vec<i16> {
    vec![0i16; (16_000.0 * count) as usize]
}

#[test]
fn duration_comes_from_the_samples() {
    let samples = seconds(2.5);
    let utterance = Utterance::mono_16k(&samples);
    assert_eq!(utterance.duration(), Duration::from_millis(2_500));
}

#[test]
fn a_tenth_of_a_second_is_accepted_as_silence() {
    let samples = seconds(0.1);
    let utterance = Utterance::mono_16k(&samples);
    assert!(utterance.check(Duration::from_secs(300)).is_ok());
    assert!(utterance.is_below_silence_floor());
}

#[test]
fn a_second_of_speech_is_not_silence() {
    let samples = seconds(1.0);
    assert!(!Utterance::mono_16k(&samples).is_below_silence_floor());
}

#[test]
fn ten_minutes_is_refused_against_the_stated_limit() {
    let samples = seconds(600.0);
    let utterance = Utterance::mono_16k(&samples);
    match utterance.check(Duration::from_secs(300)) {
        Err(Error::AudioTooLong { limit, got }) => {
            assert_eq!(limit, Duration::from_secs(300));
            assert_eq!(got, Duration::from_secs(600));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn nothing_is_truncated_on_the_way_through() {
    let samples = seconds(4.0);
    let utterance = Utterance::mono_16k(&samples);
    assert_eq!(utterance.samples.len(), samples.len());
    assert_eq!(utterance.as_bytes().len(), samples.len() * 2);
}
