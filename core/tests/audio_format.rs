//! The one shape transcription accepts, and what happens to the rest.

use edge_stt_core::config::{AudioFormat, SampleType};
use edge_stt_core::{Error, Utterance};

#[test]
fn the_default_is_what_the_audio_front_end_produces() {
    let format = AudioFormat::mono_16k();
    assert_eq!(format.sample_rate, 16_000);
    assert_eq!(format.channels, 1);
    assert_eq!(format.sample_type, SampleType::I16);
    assert_eq!(AudioFormat::default(), format);
}

#[test]
fn the_supported_shape_passes() {
    assert!(AudioFormat::mono_16k().check_transcribable().is_ok());
}

#[test]
fn every_other_shape_is_rejected_naming_both_sides() {
    let wrong = [
        AudioFormat::new(44_100, 1, SampleType::I16),
        AudioFormat::new(16_000, 2, SampleType::I16),
        AudioFormat::new(16_000, 1, SampleType::F32),
        AudioFormat::new(8_000, 1, SampleType::I16),
    ];

    for format in wrong {
        match format.check_transcribable() {
            Err(Error::UnsupportedAudio { expected, got }) => {
                assert_eq!(expected, AudioFormat::mono_16k());
                assert_eq!(got, format);
                let message = Error::UnsupportedAudio { expected, got }.to_string();
                assert!(message.contains(&expected.to_string()), "{message}");
                assert!(message.contains(&got.to_string()), "{message}");
            }
            other => panic!("{format} should have been rejected, got {other:?}"),
        }
    }
}

#[test]
fn an_utterance_in_the_wrong_shape_is_rejected_before_anything_else() {
    let samples = vec![0i16; 16_000];
    let utterance = Utterance::new(&samples, AudioFormat::new(44_100, 1, SampleType::I16));
    assert!(matches!(
        utterance.check(std::time::Duration::from_secs(300)),
        Err(Error::UnsupportedAudio { .. })
    ));
}
