//! The offline claim, checked by watching the process rather than by
//! reading the source. A dependency could reach out without us.

#[cfg(not(feature = "remote"))]
mod support;

#[cfg(not(feature = "remote"))]
use std::process::Command;

#[cfg(not(feature = "remote"))]
use edge_stt_core::{Config, EdgeStt, ModelSpec, Utterance};

/// A build that can reach a server is not the build this file is
/// about; everything here is checked without the remote feature.
#[test]
#[cfg(feature = "remote")]
fn this_build_can_reach_a_server_so_there_is_nothing_here_to_check() {}

#[cfg(not(feature = "remote"))]
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
#[cfg(not(feature = "remote"))]
#[ignore = "needs a Whisper model"]
fn transcribing_on_the_device_opens_no_socket() {
    let before = open_sockets();
    assert!(
        before.is_empty(),
        "the test process already holds sockets: {before:?}"
    );

    let stt = EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model");
    let samples = support::silence(2.0);
    stt.transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");

    let after = open_sockets();
    assert!(after.is_empty(), "transcribing opened {after:?}");
}

#[test]
#[cfg(not(feature = "remote"))]
fn a_default_build_carries_no_remote_backend() {
    use edge_stt_core::{Error, RemoteConfig};

    let outcome = EdgeStt::new(Config::remote(RemoteConfig::at("ws://localhost:1/never")));
    assert!(
        matches!(
            outcome,
            Err(Error::BackendUnavailable { backend: "remote" })
        ),
        "a build without the remote feature must say so, got {outcome:?}",
        outcome = outcome.err()
    );
}
