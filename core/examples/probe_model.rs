//! Print what a model really is, before trusting it. Models fail
//! quietly when fed the wrong shape; this is how you find out first.

use std::process::ExitCode;

use edge_stt_core::backend::whisper::WhisperBackend;
use edge_stt_core::{Accelerator, Config, ModelSpec};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: probe_model <path to ggml model> [cpu|metal|cuda|vulkan]");
        return ExitCode::FAILURE;
    };

    // Naming one makes whisper.cpp say whether this build can reach it,
    // which is the question a board is being brought up to answer.
    let mut model = ModelSpec::at(&path);
    let asked = std::env::args().nth(2);
    if let Some(name) = &asked {
        model = match name.as_str() {
            "cpu" => model.with_accelerator(Accelerator::Cpu),
            "metal" => model.with_accelerator(Accelerator::Metal),
            "cuda" => model.with_accelerator(Accelerator::Cuda),
            "vulkan" => model.with_accelerator(Accelerator::Vulkan),
            other => {
                eprintln!("{other} is not an accelerator this knows");
                return ExitCode::FAILURE;
            }
        };
    }

    let config = Config::local(model.clone());
    let backend = match WhisperBackend::load(&model, &config) {
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
    println!("accelerator   {}", asked.as_deref().unwrap_or("cpu"));
    ExitCode::SUCCESS
}
