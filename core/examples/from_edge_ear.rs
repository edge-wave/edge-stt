//! Transcribe a recording, or refuse it, depending on why it ended.
//! edge-ear knows whether it heard speech; nothing was reading that.
//!
//! usage: from_edge_ear --reason silence|max-length|no-speech|stopped
//!                      [--language ko] [--accelerator metal] <model> <wav>

use std::process::ExitCode;

use edge_stt_core::{Accelerator, Config, EdgeStt, Language, ModelSpec, Utterance};

const USAGE: &str = "usage: from_edge_ear --reason silence|max-length|no-speech|stopped \
                     [--language ko] [--accelerator cpu|metal|cuda|vulkan] <model> <wav>";

/// edge-ear's own reason for a recording ending, restated. This crate
/// does not depend on edge-ear; the caller between them passes it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndReason {
    Silence,
    MaxLength,
    NoSpeech,
    Stopped,
}

impl EndReason {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "silence" => Some(EndReason::Silence),
            "max-length" => Some(EndReason::MaxLength),
            "no-speech" => Some(EndReason::NoSpeech),
            "stopped" => Some(EndReason::Stopped),
            _ => None,
        }
    }

    /// The whole of the gate. Nothing was said, so there is nothing to
    /// transcribe, and asking regardless is what invents words.
    fn was_anything_said(self) -> bool {
        self != EndReason::NoSpeech
    }
}

struct Args {
    reason: EndReason,
    language: Option<String>,
    accelerator: Option<String>,
    model: String,
    wav: String,
}

fn main() -> ExitCode {
    let Some(args) = parse(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    if !args.reason.was_anything_said() {
        println!(
            "{:?}: nothing was said, so nothing is transcribed",
            args.reason
        );
        return ExitCode::SUCCESS;
    }

    let mut model = ModelSpec::at(&args.model);
    if let Some(name) = &args.accelerator {
        let Some(accelerator) = accelerator(name) else {
            eprintln!("{name} is not an accelerator this knows");
            return ExitCode::FAILURE;
        };
        model = model.with_accelerator(accelerator);
    }

    // Named rather than detected, for the same reason the gate exists:
    // a decoder left to guess is a decoder with more room to invent.
    let mut config = Config::local(model);
    if let Some(tag) = &args.language {
        config = config.with_language(Language::new(tag));
    }

    let samples = match read_wav(&args.wav) {
        Ok(samples) => samples,
        Err(why) => {
            eprintln!("{}: {why}", args.wav);
            return ExitCode::FAILURE;
        }
    };

    let stt = match EdgeStt::new(config) {
        Ok(stt) => stt,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    match stt.transcribe(&Utterance::mono_16k(&samples)) {
        Ok(transcript) if transcript.text.is_empty() => {
            println!("{:?}: nothing came back", args.reason);
            ExitCode::SUCCESS
        }
        Ok(transcript) => {
            println!("{:?}: {}", args.reason, transcript.text);
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

fn accelerator(name: &str) -> Option<Accelerator> {
    match name {
        "cpu" => Some(Accelerator::Cpu),
        "metal" => Some(Accelerator::Metal),
        "cuda" => Some(Accelerator::Cuda),
        "vulkan" => Some(Accelerator::Vulkan),
        _ => None,
    }
}

fn parse(mut args: impl Iterator<Item = String>) -> Option<Args> {
    let mut reason = None;
    let mut language = None;
    let mut accelerator = None;
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reason" => reason = EndReason::parse(&args.next()?),
            "--language" => language = Some(args.next()?),
            "--accelerator" => accelerator = Some(args.next()?),
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() != 2 {
        return None;
    }
    Some(Args {
        reason: reason?,
        language,
        accelerator,
        model: positional.remove(0),
        wav: positional.remove(0),
    })
}

fn read_wav(path: &str) -> Result<Vec<i16>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!(
            "{} Hz {} channel {}-bit, and only 16000 Hz mono 16-bit can be transcribed",
            spec.sample_rate, spec.channels, spec.bits_per_sample
        ));
    }
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
