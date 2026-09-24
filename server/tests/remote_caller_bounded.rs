//! A caller-bounded session through the library to a real server gives
//! the same words as handing the server the whole recording at once.

mod support;

use edge_stt_core::{SessionConfig, Utterance};

#[test]
#[ignore = "needs a Whisper model and a sample recording"]
fn a_caller_bounded_session_matches_transcribing_the_recording_whole() {
    let (_runtime, running) = support::serving(support::start_without_vad(2));
    let stt = support::client(&running);

    // Speech, a pause longer than any detector's default, then speech.
    let (spoken, _) = support::spoken_sample();
    let mut recording = spoken.clone();
    recording.extend(std::iter::repeat_n(0, 16_000 * 4));
    recording.extend(&spoken);

    let whole = stt
        .transcribe(&Utterance::mono_16k(&recording))
        .expect("a transcript");

    let mut session = stt
        .open_session(SessionConfig::new().with_caller_boundaries())
        .expect("a session");
    for chunk in recording.chunks(1_600) {
        let early = session.push(chunk, None).expect("a push");
        assert!(early.is_none(), "the utterance ended before close");
    }
    let closed = session.close(None).expect("a close");

    assert_eq!(closed.len(), 1, "one utterance, one transcript");
    assert_eq!(closed[0].text, whole.text);
    assert_eq!(closed[0].audio_duration, whole.audio_duration);
}
