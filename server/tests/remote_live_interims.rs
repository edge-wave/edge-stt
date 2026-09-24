//! Words while the speaker is still talking, through the library, from
//! a real server that runs the session.

mod support;

use std::time::Duration;

use edge_stt_core::{Partial, PartialKind, SessionConfig};

#[test]
#[ignore = "needs a Whisper model and a sample recording"]
fn interims_arrive_over_the_network_before_the_speaker_stops() {
    let (_runtime, running) = support::serving(support::start_without_vad(2));
    let stt = support::client(&running);
    let interval = Duration::from_millis(300);
    let config = SessionConfig::new()
        .with_caller_boundaries()
        .with_live_interims()
        .with_interim_min_interval(interval);
    let mut session = stt.open_session(config).expect("a session");

    let (samples, _) = support::spoken_sample();
    let chunks: Vec<_> = samples.chunks(1_600).collect();
    let mut seen: Vec<Partial> = Vec::new();
    let mut before_the_last = 0;
    for (index, chunk) in chunks.iter().enumerate() {
        let mut sink = |partial: Partial| seen.push(partial);
        session.push(chunk, Some(&mut sink)).expect("a push");
        if index + 1 < chunks.len() {
            before_the_last = seen.len();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    session.close(None).expect("a close");

    assert!(
        before_the_last > 0,
        "no interim arrived while audio was still being pushed"
    );
    // The interval is the server's gate, tested where it lives; arrival
    // times here include a network hop that may bunch two together.
    for pair in seen.windows(2) {
        assert_ne!(
            pair[0].text, pair[1].text,
            "the same words were delivered twice"
        );
    }
    assert!(seen.iter().all(|p| p.kind == PartialKind::Replace));
}
