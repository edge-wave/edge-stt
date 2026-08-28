//! The offline claim, checked by watching the process rather than by
//! reading the source. A dependency could reach out without us.

mod support;

use std::process::Command;

use edge_stt_core::{Config, EdgeStt, ModelSpec, Utterance};

fn open_sockets() -> Vec<String> {
    let pid = std::process::id().to_string();
    let output = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid, "-i"])
        .output()
        .expect("lsof, which both macOS and Linux carry");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .map(str::to_string)
        .collect()
}

#[test]
#[ignore = "needs a Whisper model"]
fn transcribing_on_the_device_opens_no_socket() {
    let before = open_sockets();
    assert!(before.is_empty(), "the test process already holds sockets: {before:?}");

    let stt = EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model");
    let samples = support::silence(2.0);
    stt.transcribe(&Utterance::mono_16k(&samples)).expect("a transcript");

    let after = open_sockets();
    assert!(after.is_empty(), "transcribing opened {after:?}");
}

#[test]
fn a_default_build_carries_no_remote_backend() {
    use edge_stt_core::{Error, RemoteConfig};

    let outcome = EdgeStt::new(Config::remote(RemoteConfig::at("ws://localhost:1/never")));
    assert!(
        matches!(outcome, Err(Error::BackendUnavailable { backend: "remote" })),
        "a build without the remote feature must say so, got {outcome:?}",
        outcome = outcome.err()
    );
}
