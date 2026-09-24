//! A caller that already knows where speech stops -- edge-ear, which
//! reports the end itself -- gets from a session exactly what handing
//! over the finished recording would have given it.

#![cfg(feature = "streaming")]

mod support;

use edge_stt_core::{Config, EdgeStt, Language, SessionConfig, Utterance};

const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio

#[test]
#[ignore = "needs a Whisper model and a sample recording"]
fn pushing_a_recording_and_closing_matches_transcribing_it_whole() {
    let config = Config::local(support::shared_model_spec())
        .with_language(Language::new(support::sample_language()));
    let stt = EdgeStt::new(config).expect("a model");
    let (samples, _) = support::spoken_sample();

    let whole = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");

    let mut session = stt
        .open_session(SessionConfig::new().with_caller_boundaries())
        .expect("no boundary model should be needed");
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let early = session.push(chunk, None).expect("a push");
        assert!(early.is_none(), "something other than close ended it");
    }
    let pushed = session
        .close(None)
        .expect("a close")
        .expect("the utterance");

    assert_eq!(pushed.text, whole.text);
    assert_eq!(pushed.audio_duration, whole.audio_duration);
}
