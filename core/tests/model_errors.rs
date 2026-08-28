//! A model that is not there, or not a model, is caught when the
//! transcriber is built. These need no model of their own.

use std::io::Write;

use edge_stt_core::{Config, EdgeStt, Error, ModelSpec};

#[test]
fn a_missing_file_fails_at_construction_naming_the_path() {
    let path = std::env::temp_dir().join("edge-stt-no-such-model.bin");
    let _ = std::fs::remove_file(&path);

    match EdgeStt::new(Config::local(ModelSpec::at(&path))) {
        Err(Error::ModelMissing { path: searched }) => assert_eq!(searched, path),
        other => panic!(
            "expected a missing model, got {other:?}",
            other = other.err()
        ),
    }
}

#[test]
fn a_file_that_is_not_a_model_fails_as_unusable() {
    let path = std::env::temp_dir().join("edge-stt-not-a-model.bin");
    let mut file = std::fs::File::create(&path).expect("a writable temporary directory");
    file.write_all(b"this is not a whisper model")
        .expect("a write");
    drop(file);

    let outcome = EdgeStt::new(Config::local(ModelSpec::at(&path)));
    let _ = std::fs::remove_file(&path);

    match outcome {
        Err(Error::ModelUnusable { path: named, why }) => {
            assert_eq!(named, path);
            assert!(!why.is_empty(), "the reason must say something");
        }
        other => panic!(
            "expected an unusable model, got {other:?}",
            other = other.err()
        ),
    }
}

#[test]
fn a_remote_transcriber_without_an_endpoint_fails_at_construction() {
    use edge_stt_core::RemoteConfig;

    let outcome = EdgeStt::new(Config::remote(RemoteConfig::at("")));
    assert!(matches!(
        outcome,
        Err(Error::InvalidValue {
            setting: "endpoint",
            ..
        })
    ));
}

#[test]
fn an_accelerator_this_build_lacks_is_refused_rather_than_ignored() {
    use edge_stt_core::Accelerator;

    let path = std::env::temp_dir().join("edge-stt-accelerator-check.bin");
    std::fs::write(&path, b"placeholder").expect("a writable temporary directory");

    let model = ModelSpec::at(&path).with_accelerator(Accelerator::Cuda);
    let outcome = EdgeStt::new(Config::local(model));
    let _ = std::fs::remove_file(&path);

    match outcome {
        Err(Error::InvalidValue {
            setting: "accelerator",
            got,
            ..
        }) => assert_eq!(got, "cuda"),
        other => panic!(
            "expected a refused accelerator, got {other:?}",
            other = other.err()
        ),
    }
}
