//! Print what a model really is, before trusting it. Models fail
//! quietly when fed the wrong shape; this is how you find out first.

use std::process::ExitCode;

use edge_stt_core::backend::whisper::WhisperBackend;
use edge_stt_core::{Config, ModelSpec};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: probe_model <path to ggml model>");
        return ExitCode::FAILURE;
    };

    let config = Config::local(ModelSpec::at(&path));
    let backend = match WhisperBackend::load(&ModelSpec::at(&path), &config) {
        Ok(backend) => backend,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let facts = backend.facts();
    println!("path          {path}");
    println!("model         {}", facts.description);
    println!("multilingual  {}", facts.multilingual);
    println!("vocabulary    {}", facts.vocabulary);
    println!("audio context {}", facts.audio_context);
    println!("sample rate   16000 Hz mono 16-bit, always");
    ExitCode::SUCCESS
}
