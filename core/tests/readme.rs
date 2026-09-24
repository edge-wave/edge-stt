//! The README's examples, compiled. If the API moves and the prose
//! does not, this fails rather than the reader finding out.

use std::path::PathBuf;

fn readme() -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    std::fs::read_to_string(path.join("README.md")).expect("a README at the repository root")
}

/// The transcribing example, verbatim. Compiling is the point; it is
/// never run, because running it would need a model.
#[allow(dead_code)]
fn the_transcribing_example(samples: &[i16]) -> edge_stt_core::Result<()> {
    use edge_stt_core::{Config, EdgeStt, ModelSpec, Utterance};

    let stt = EdgeStt::new(Config::local(ModelSpec::at("models/ggml-base-q5_1.bin")))?;
    let transcript = stt.transcribe(&Utterance::mono_16k(samples))?;
    println!("{}", transcript.text);
    Ok(())
}

#[allow(dead_code)]
#[cfg(feature = "remote")]
fn the_remote_example() -> edge_stt_core::Result<()> {
    use edge_stt_core::{Config, EdgeStt, RemoteConfig};

    let _stt = EdgeStt::new(Config::remote(RemoteConfig::at(
        "ws://host:8000/api/v1/transcribe",
    )))?;
    Ok(())
}

#[allow(dead_code)]
fn the_partials_example(stt: &edge_stt_core::EdgeStt, utterance: &edge_stt_core::Utterance<'_>) {
    use edge_stt_core::CancelToken;

    let cancel = CancelToken::new();
    let _ = stt.transcribe_with(utterance, |p| print!("{}", p.text), &cancel);
}

#[allow(dead_code)]
#[cfg(feature = "streaming")]
fn the_continuous_example(
    stt: &edge_stt_core::EdgeStt,
    samples: &[i16],
) -> edge_stt_core::Result<()> {
    use edge_stt_core::EndpointConfig;

    let detector = EndpointConfig::new().with_local_vad_model("models/ggml-silero-v5.1.2.bin");
    let mut session = stt.open_session(detector)?;
    for chunk in samples.chunks(1_600) {
        if let Some(transcript) = session.push(chunk, None)? {
            println!("{}", transcript.text);
        }
    }
    for transcript in session.close(None)? {
        println!("{}", transcript.text);
    }
    Ok(())
}

#[allow(dead_code)]
#[cfg(feature = "streaming")]
fn the_caller_bounded_example(
    stt: &edge_stt_core::EdgeStt,
    chunks_while_recording: Vec<&[i16]>,
) -> edge_stt_core::Result<()> {
    use edge_stt_core::SessionConfig;

    let mut session = stt.open_session(SessionConfig::new().with_caller_boundaries())?;
    for chunk in chunks_while_recording {
        session.push(chunk, None)?;
    }
    let _transcript = session.close(None)?;
    Ok(())
}

#[allow(dead_code)]
#[cfg(feature = "streaming")]
fn the_live_interims_example(
    stt: &edge_stt_core::EdgeStt,
    chunk: &[i16],
) -> edge_stt_core::Result<()> {
    use edge_stt_core::{EndpointConfig, PartialKind, SessionConfig};

    let detector = EndpointConfig::new().with_local_vad_model("models/ggml-silero-v5.1.2.bin");
    let mut session = stt.open_session(
        SessionConfig::new()
            .with_endpointing(detector)
            .with_live_interims(),
    )?;

    let mut caption = String::new();
    let mut show = |partial: edge_stt_core::Partial| match partial.kind {
        PartialKind::Replace => caption = partial.text,
        PartialKind::Append => caption.push_str(&partial.text),
    };
    if let Some(transcript) = session.push(chunk, Some(&mut show))? {
        println!("{}", transcript.text);
    }
    Ok(())
}

#[test]
fn the_examples_above_are_the_ones_the_readme_shows() {
    let readme = readme();
    for line in [
        "let stt = EdgeStt::new(Config::local(ModelSpec::at(\"models/ggml-base-q5_1.bin\")))?;",
        "let transcript = stt.transcribe(&Utterance::mono_16k(&samples))?;",
        "EdgeStt::new(Config::remote(RemoteConfig::at(\"ws://host:8000/api/v1/transcribe\")))?;",
        "stt.transcribe_with(&utterance, |p| print!(\"{}\", p.text), &cancel)?;",
        "let detector = EndpointConfig::new().with_local_vad_model(\"models/ggml-silero-v5.1.2.bin\");",
        "let mut session = stt.open_session(detector)?;",
        "if let Some(transcript) = session.push(chunk, None)? {",
        "for transcript in session.close(None)? {",
        "let mut session = stt.open_session(SessionConfig::new().with_caller_boundaries())?;",
        "        .with_live_interims(),",
        "    PartialKind::Replace => caption = partial.text,",
        "if let Some(transcript) = session.push(chunk, Some(&mut show))? {",
    ] {
        assert!(
            readme.contains(line),
            "the README no longer shows:\n  {line}"
        );
    }
}

#[test]
fn the_readme_names_the_build_that_actually_works() {
    let readme = readme();
    assert!(
        readme.contains("--features full"),
        "the checking command must be the real one"
    );
    assert!(
        !readme.contains("cargo test --workspace --all-features"),
        "--all-features turns on accelerators most machines cannot build"
    );
    assert!(
        readme.contains("cmake"),
        "whisper.cpp needs CMake and the README must say so"
    );
}

#[test]
fn the_readme_promises_no_model_it_does_not_ship() {
    let readme = readme();
    assert!(
        readme.contains("You do"),
        "the model table must say who supplies the weights"
    );
    assert!(
        readme.contains("Ship a model"),
        "the list of what it does not do must say this"
    );
}
