//! There is no server this library knows about unless you name one.

mod support;

use edge_stt_core::{Config, EdgeStt, Error, RemoteConfig};

#[test]
fn a_remote_client_without_an_endpoint_never_gets_built() {
    match EdgeStt::new(Config::remote(RemoteConfig::at(""))) {
        Err(Error::InvalidValue {
            setting: "endpoint",
            ..
        }) => {}
        other => panic!("expected a refusal, got {other:?}", other = other.err()),
    }
    match EdgeStt::new(Config::remote(RemoteConfig::at("   "))) {
        Err(Error::InvalidValue {
            setting: "endpoint",
            ..
        }) => {}
        other => panic!(
            "whitespace is not an endpoint, got {other:?}",
            other = other.err()
        ),
    }
}

#[test]
fn the_crate_carries_no_address_of_its_own() {
    let mut found = Vec::new();
    walk(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(found.is_empty(), "these files name a server: {found:?}");
}

fn walk(directory: std::path::PathBuf, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(path, found);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(source) = std::fs::read_to_string(&path)
            && (source.contains("ws://") || source.contains("wss://"))
        {
            found.push(path.display().to_string());
        }
    }
}

#[test]
fn a_credential_never_reaches_a_log_by_accident() {
    let remote = RemoteConfig::at("ws://host:8000/x").with_credential("hunter2");
    let printed = format!("{remote:?}");
    assert!(!printed.contains("hunter2"), "{printed}");
    assert!(printed.contains("redacted"), "{printed}");
}
