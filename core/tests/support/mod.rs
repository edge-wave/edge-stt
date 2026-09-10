//! Shared by the tests that need a real model. Everything here reads
//! the environment rather than assuming a path.

#![allow(dead_code)]

use std::path::PathBuf;

use edge_stt_core::{Accelerator, ModelSpec};

#[cfg(feature = "remote")]
pub mod stub_server;

/// Where the operator put their Whisper files, the same way edge-ear
/// finds wake word models.
pub fn model_path() -> PathBuf {
    let dir = std::env::var("EDGE_STT_MODEL_DIR")
        .expect("set EDGE_STT_MODEL_DIR to a directory holding a ggml Whisper model");
    let mut candidates: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|why| panic!("{dir}: {why}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension().is_some_and(|e| e == "bin")
                && path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("ggml-"))
        })
        .filter_map(|path| std::fs::metadata(&path).ok().map(|m| (m.len(), path)))
        .collect();
    // The biggest one present. The smallest is fastest and bad enough
    // on short clips that a test would measure the model, not us.
    candidates.sort();
    candidates
        .pop()
        .map(|(_, path)| path)
        .unwrap_or_else(|| panic!("no ggml-*.bin under {dir}"))
}

/// The same model, with the cores divided rather than claimed whole.
/// A test opening two live sessions is the case the server's --threads
/// exists for: given every core each, they contend instead of running.
pub fn shared_model_spec() -> ModelSpec {
    let half = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1));
    model_spec().with_threads(half as u16)
}

/// The model, and the accelerator to run it on. Without the variable
/// it is the processor, which is what a plain machine has.
pub fn model_spec() -> ModelSpec {
    let spec = ModelSpec::at(model_path());
    let Ok(name) = std::env::var("EDGE_STT_ACCELERATOR") else {
        return spec;
    };
    let accelerator = match name.as_str() {
        "cpu" => Accelerator::Cpu,
        "metal" => Accelerator::Metal,
        "cuda" => Accelerator::Cuda,
        "vulkan" => Accelerator::Vulkan,
        other => panic!("{other} is not an accelerator"),
    };
    spec.with_accelerator(accelerator)
}

/// Where the operator put their GGML Silero VAD file -- a second,
/// separate model from the Whisper one, so a second variable.
#[cfg(feature = "streaming")]
pub fn vad_model_path() -> PathBuf {
    std::env::var("EDGE_STT_VAD_MODEL")
        .map(PathBuf::from)
        .expect("set EDGE_STT_VAD_MODEL to a ggml Silero VAD file")
}

/// A recording of known speech, and the words in it.
pub fn spoken_sample() -> (Vec<i16>, String) {
    let path = std::env::var("EDGE_STT_SAMPLE_WAV")
        .expect("set EDGE_STT_SAMPLE_WAV to a 16 kHz mono 16-bit recording of speech");
    let expected = std::env::var("EDGE_STT_SAMPLE_TEXT")
        .expect("set EDGE_STT_SAMPLE_TEXT to the words spoken in EDGE_STT_SAMPLE_WAV");
    (read_wav(&path), expected)
}

/// The language the sample is in. Left to detection, a short clip and
/// a small model guess badly, and the test measures the guess.
pub fn sample_language() -> String {
    std::env::var("EDGE_STT_SAMPLE_LANGUAGE").unwrap_or_else(|_| "en".to_string())
}

/// A longer recording, for the tests that need several segments.
/// Never built by repeating a short one: Whisper degrades badly on
/// repetitive audio -- measured at fourteen times slower here, and it
/// collapses twenty seconds into a single segment.
pub fn long_spoken_sample() -> Vec<i16> {
    match std::env::var("EDGE_STT_LONG_WAV") {
        Ok(path) => read_wav(&path),
        Err(_) => spoken_sample().0,
    }
}

pub fn read_wav(path: &str) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).unwrap_or_else(|why| panic!("{path}: {why}"));
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "{path} must be 16 kHz");
    assert_eq!(spec.channels, 1, "{path} must be mono");
    assert_eq!(spec.bits_per_sample, 16, "{path} must be 16-bit");
    reader
        .samples::<i16>()
        .map(|s| s.expect("a readable sample"))
        .collect()
}

pub fn silence(seconds: f32) -> Vec<i16> {
    vec![0i16; (16_000.0 * seconds) as usize]
}

/// Hiss, so a test can tell "no speech" apart from "no audio".
pub fn noise(seconds: f32) -> Vec<i16> {
    let count = (16_000.0 * seconds) as usize;
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 48) as i16) / 64
        })
        .collect()
}
