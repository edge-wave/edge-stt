//! A session on the network path, against a stub that speaks the
//! stream protocol, so how it behaves needs no model to check.

#![cfg(all(feature = "remote", feature = "streaming"))]

mod support;

use std::time::{Duration, Instant};

use edge_stt_core::{
    Config, EdgeStt, EndpointConfig, Error, Partial, PartialKind, RemoteConfig, SessionConfig,
};
use support::stub_server::{Behaviour, Reply, StreamScript, StubServer};

const CHUNK: usize = 1_600;

fn against(script: &StreamScript) -> (StubServer, EdgeStt) {
    let server = StubServer::start(Behaviour::Stream(script.clone()));
    let config = Config::remote(RemoteConfig::at(&server.endpoint));
    (server, EdgeStt::new(config).expect("a remote transcriber"))
}

fn caller_bounded() -> SessionConfig {
    SessionConfig::new().with_caller_boundaries()
}

/// Waits for `ready`, pushing nothing so whatever came back is drained.
fn until(mut ready: impl FnMut() -> bool) -> bool {
    let give_up = Instant::now() + Duration::from_secs(3);
    while Instant::now() < give_up {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn audio_reaches_the_server_before_the_session_closes() {
    let mut script = StreamScript::echoing("caller");
    script.on_close = vec![Reply::Final("done".to_string())];
    let (_server, stt) = against(&script);
    let mut session = stt.open_session(caller_bounded()).expect("a session");

    for _ in 0..10 {
        let early = session.push(&[0; CHUNK], None).expect("a push");
        assert!(
            early.is_none(),
            "nothing ends a caller-bounded utterance early"
        );
    }
    assert!(
        until(|| script.frames() == 10),
        "the server had {} frames before close",
        script.frames()
    );

    let finals = session.close(None).expect("a close");
    let texts: Vec<_> = finals.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, vec!["done"]);

    let log = script.log.lock().unwrap();
    let opened = log.opened_with.as_ref().expect("an open_stream");
    assert_eq!(opened["boundaries"], "caller");
    assert_eq!(opened["want_partials"], true);
    assert!(opened.get("pause_tolerance_ms").is_none());
    assert_eq!(log.frames_at_close, Some(10));
}

#[test]
fn a_server_that_does_not_echo_caller_boundaries_is_refused() {
    let script = StreamScript::default();
    let (_server, stt) = against(&script);
    match stt.open_session(caller_bounded()) {
        Err(Error::InvalidValue { setting, .. }) => assert_eq!(setting, "boundaries"),
        Err(other) => panic!("expected the boundaries to be named, got {other}"),
        Ok(_) => panic!("an older server was accepted for caller boundaries"),
    }
}

#[test]
fn a_server_vanishing_mid_stream_is_a_network_error_from_then_on() {
    let mut script = StreamScript::echoing("caller");
    script.drop_after_frames = Some(3);
    let (_server, stt) = against(&script);
    let mut session = stt.open_session(caller_bounded()).expect("a session");

    let mut failure = None;
    let give_up = Instant::now() + Duration::from_secs(3);
    while failure.is_none() && Instant::now() < give_up {
        if let Err(why) = session.push(&[0; CHUNK], None) {
            failure = Some(why);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(failure, Some(Error::Network { .. })),
        "{failure:?}"
    );
    assert!(matches!(
        session.push(&[0; CHUNK], None),
        Err(Error::Network { .. })
    ));
}

#[test]
fn audio_the_link_cannot_carry_is_bounded_by_the_maximum_duration() {
    let mut script = StreamScript::echoing("caller");
    script.stop_reading = true;
    let server = StubServer::start(Behaviour::Stream(script.clone()));
    let config = Config::remote(RemoteConfig::at(&server.endpoint))
        .with_max_duration(Duration::from_secs(1));
    let stt = EdgeStt::new(config).expect("a remote transcriber");
    let mut session = stt.open_session(caller_bounded()).expect("a session");

    // Twenty minutes of audio is far more than any socket buffers hold.
    let failure = (0..12_000).find_map(|_| session.push(&[0; CHUNK], None).err());
    assert!(
        matches!(failure, Some(Error::Network { .. })),
        "{failure:?}"
    );
}

#[test]
fn interims_reach_the_callback_during_a_push_and_are_dropped_without_one() {
    let mut script = StreamScript::echoing("caller");
    script.after_frames = vec![
        (1, Reply::Partial("and so".to_string())),
        (2, Reply::Partial("and so my".to_string())),
    ];
    let (_server, stt) = against(&script);
    let config = caller_bounded().with_live_interims();
    let mut session = stt.open_session(config).expect("a session");

    session.push(&[0; CHUNK], None).expect("a push");
    std::thread::sleep(Duration::from_millis(200));
    // The first came back while no callback was given, so it is gone.
    session.push(&[0; CHUNK], None).expect("a push");

    let caller = std::thread::current().id();
    let mut seen: Vec<Partial> = Vec::new();
    assert!(until(|| {
        let mut sink = |partial: Partial| {
            assert_eq!(std::thread::current().id(), caller);
            seen.push(partial);
        };
        session.push(&[], Some(&mut sink)).expect("a push");
        !seen.is_empty()
    }));
    let texts: Vec<_> = seen.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, vec!["and so my"]);
    assert_eq!(seen[0].kind, PartialKind::Replace);

    let log = script.log.lock().unwrap();
    let opened = log.opened_with.as_ref().expect("an open_stream");
    assert_eq!(opened["live_interims"], true);
    assert!(opened["interim_min_interval_ms"].is_u64());
}

#[test]
fn finals_come_back_one_per_push_and_the_rest_at_close_in_order() {
    let mut script = StreamScript::echoing("server");
    script.after_frames = vec![
        (2, Reply::Final("one".to_string())),
        (3, Reply::Final("two".to_string())),
    ];
    script.on_close = vec![Reply::Final("three".to_string())];
    let (_server, stt) = against(&script);
    let detector = EndpointConfig::new().with_pause_tolerance(Duration::from_millis(700));
    let mut session = stt
        .open_session(SessionConfig::new().with_endpointing(detector))
        .expect("a session with no on-device model");

    for _ in 0..3 {
        session.push(&[0; CHUNK], None).expect("a push");
    }
    let mut first = None;
    assert!(until(|| {
        first = session.push(&[], None).expect("a push");
        first.is_some()
    }));
    assert_eq!(first.expect("a final").text, "one");

    let rest = session.close(None).expect("a close");
    let texts: Vec<_> = rest.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(texts, vec!["two", "three"]);

    let log = script.log.lock().unwrap();
    let opened = log.opened_with.as_ref().expect("an open_stream");
    assert_eq!(opened["boundaries"], "server");
    assert_eq!(opened["pause_tolerance_ms"], 700);
}

#[test]
fn a_server_without_a_detector_is_refused_naming_it() {
    let script = StreamScript {
        refuse: Some("streaming_unavailable"),
        ..StreamScript::default()
    };
    let (_server, stt) = against(&script);
    match stt.open_session(SessionConfig::new().with_endpointing(EndpointConfig::new())) {
        Err(Error::InvalidValue { setting, .. }) => assert_eq!(setting, "endpointing"),
        Err(other) => panic!("expected the missing detector to be named, got {other}"),
        Ok(_) => panic!("a server with no detector accepted a detector session"),
    }
}

#[test]
fn a_session_that_names_no_boundaries_is_refused_on_the_network_path() {
    let script = StreamScript::echoing("server");
    let (_server, stt) = against(&script);
    match stt.open_session(SessionConfig::new()) {
        Err(Error::InvalidValue { setting, .. }) => assert_eq!(setting, "endpointing"),
        Err(other) => panic!("expected endpointing to be named, got {other}"),
        Ok(_) => panic!("a network session opened with no boundaries at all"),
    }
}
