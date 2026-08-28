//! The one speed claim edge-stt makes about itself: it costs almost
//! nothing on top of the model. How fast the model is belongs to the
//! model and the board, and is measured, not promised.

mod support;

use std::process::Command;
use std::time::{Duration, Instant};

use edge_stt_core::{Config, EdgeStt, Language, ModelSpec, Utterance};

/// whisper.cpp's own binary, the thing we must not be slower than.
fn reference_tool() -> String {
    std::env::var("EDGE_STT_WHISPER_CLI")
        .expect("set EDGE_STT_WHISPER_CLI to whisper.cpp's own whisper-cli binary")
}

#[test]
#[ignore = "needs a Whisper model, a recording, and whisper.cpp's own binary"]
fn transcribing_costs_no_more_than_ten_percent_over_the_reference() {
    let model = support::model_path();
    let wav = std::env::var("EDGE_STT_SAMPLE_WAV").expect("a recording to measure");
    let samples = support::read_wav(&wav);

    let config = Config::local(ModelSpec::at(&model)).with_language(Language::new("en"));
    let stt = EdgeStt::new(config).expect("a model");
    // Warm the caches so the first decode does not pay for both.
    let _ = stt.transcribe(&Utterance::mono_16k(&samples));

    let started = Instant::now();
    stt.transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");
    let ours = started.elapsed();

    let started = Instant::now();
    let output = Command::new(reference_tool())
        .args([
            "-m",
            &model.to_string_lossy(),
            "-l",
            "en",
            "-nt",
            "-f",
            &wav,
        ])
        .output()
        .expect("the reference binary to run");
    let theirs = started.elapsed();
    assert!(output.status.success(), "the reference binary failed");

    let allowed = theirs.mul_f32(1.10).max(theirs + Duration::from_millis(50));
    assert!(
        ours <= allowed,
        "edge-stt took {ours:?} against the reference's {theirs:?}; \
         a thin binding should add nothing"
    );
}
