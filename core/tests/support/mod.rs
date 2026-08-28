//! Shared by the tests that need a real model. Everything here reads
//! the environment rather than assuming a path.

#![allow(dead_code)]

use std::path::PathBuf;

/// Where the operator put their Whisper files, the same way edge-ear
/// finds wake word models.
pub fn model_path() -> PathBuf {
    let dir = std::env::var("EDGE_STT_MODEL_DIR")
        .expect("set EDGE_STT_MODEL_DIR to a directory holding a ggml Whisper model");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|why| panic!("{dir}: {why}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension().is_some_and(|e| e == "bin")
                && path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("ggml-"))
        })
        .collect();
    candidates.sort();
    candidates.pop().unwrap_or_else(|| panic!("no ggml-*.bin under {dir}"))
}

/// A recording of known speech, and the words in it.
pub fn spoken_sample() -> (Vec<i16>, String) {
    let path = std::env::var("EDGE_STT_SAMPLE_WAV")
        .expect("set EDGE_STT_SAMPLE_WAV to a 16 kHz mono 16-bit recording of speech");
    let expected = std::env::var("EDGE_STT_SAMPLE_TEXT")
        .expect("set EDGE_STT_SAMPLE_TEXT to the words spoken in EDGE_STT_SAMPLE_WAV");
    (read_wav(&path), expected)
}

pub fn read_wav(path: &str) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).unwrap_or_else(|why| panic!("{path}: {why}"));
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "{path} must be 16 kHz");
    assert_eq!(spec.channels, 1, "{path} must be mono");
    assert_eq!(spec.bits_per_sample, 16, "{path} must be 16-bit");
    reader.samples::<i16>().map(|s| s.expect("a readable sample")).collect()
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
