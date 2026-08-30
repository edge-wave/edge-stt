//! Audio the transcriber cannot take is refused at submission, with
//! both shapes named, and long audio is refused before any decoding.

mod support;

use std::time::Duration;

use edge_stt_core::config::{AudioFormat, SampleType};
use edge_stt_core::{Config, EdgeStt, Error, Utterance};

fn transcriber(max: Duration) -> EdgeStt {
    let config = Config::local(support::model_spec()).with_max_duration(max);
    EdgeStt::new(config).expect("a model")
}

#[test]
#[ignore = "needs a Whisper model"]
fn every_unsupported_shape_names_what_was_expected() {
    let stt = transcriber(Duration::from_secs(300));
    let samples = support::silence(1.0);

    for format in [
        AudioFormat::new(44_100, 1, SampleType::I16),
        AudioFormat::new(16_000, 2, SampleType::I16),
        AudioFormat::new(16_000, 1, SampleType::F32),
    ] {
        match stt.transcribe(&Utterance::new(&samples, format)) {
            Err(Error::UnsupportedAudio { expected, got }) => {
                assert_eq!(expected, AudioFormat::mono_16k());
                assert_eq!(got, format);
            }
            other => panic!("{format} should have been refused, got {other:?}"),
        }
    }
}

#[test]
#[ignore = "needs a Whisper model"]
fn long_audio_is_refused_up_front_rather_than_truncated() {
    let stt = transcriber(Duration::from_secs(5));
    let samples = support::silence(30.0);

    match stt.transcribe(&Utterance::mono_16k(&samples)) {
        Err(Error::AudioTooLong { limit, got }) => {
            assert_eq!(limit, Duration::from_secs(5));
            assert_eq!(got, Duration::from_secs(30));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}
