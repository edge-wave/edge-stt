//! Read a wav, print what was said, and say what it cost.
//!
//! usage: transcribe [--partials] [--bench] [--language ko]
//!                   [--accelerator metal] [--threads 4] <model> <wav>...

use std::process::ExitCode;
use std::time::Instant;

use edge_stt_core::{
    Accelerator, CancelToken, Config, EdgeStt, Language, ModelSpec, Transcript, Utterance,
};

const USAGE: &str = "usage: transcribe [--partials] [--bench] [--language ko] \
                     [--accelerator cpu|metal|cuda|vulkan] [--threads N] <model> <wav>...";

struct Args {
    partials: bool,
    bench: bool,
    language: Option<String>,
    accelerator: Option<String>,
    threads: Option<u16>,
    model: String,
    wavs: Vec<String>,
}

fn main() -> ExitCode {
    let Some(args) = parse(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let model = match local_model(&args) {
        Ok(model) => model,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    // The whole of switching to a server: one setting, no code.
    #[allow(unused_mut)]
    let mut config = match std::env::var("EDGE_STT_ENDPOINT") {
        Ok(endpoint) => {
            #[cfg(feature = "remote")]
            {
                let mut remote = edge_stt_core::RemoteConfig::at(endpoint);
                if let Ok(token) = std::env::var("EDGE_STT_TOKEN") {
                    remote = remote.with_credential(token);
                }
                Config::remote(remote)
            }
            #[cfg(not(feature = "remote"))]
            {
                let _ = endpoint;
                eprintln!("this build has no remote backend; rebuild with --features remote");
                return ExitCode::FAILURE;
            }
        }
        Err(_) => Config::local(model),
    };
    if let Some(tag) = &args.language {
        config = config.with_language(Language::new(tag));
    }

    let loading = Instant::now();
    let stt = match EdgeStt::new(config) {
        Ok(stt) => stt,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    if args.bench {
        println!("model loaded in {:.2?}", loading.elapsed());
    }

    for wav in &args.wavs {
        let samples = match read_wav(wav) {
            Ok(samples) => samples,
            Err(why) => {
                eprintln!("{wav}: {why}");
                return ExitCode::FAILURE;
            }
        };
        match run(&stt, &samples, &args) {
            Ok(transcript) => report(wav, &transcript, &args),
            Err(why) => {
                eprintln!("{wav}: {why}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn run(stt: &EdgeStt, samples: &[i16], args: &Args) -> edge_stt_core::Result<Transcript> {
    let utterance = Utterance::mono_16k(samples);
    if !args.partials && !args.bench {
        return stt.transcribe(&utterance);
    }

    let cancel = CancelToken::new();
    let started = Instant::now();
    let mut first_partial = None;
    let transcript = stt.transcribe_with(
        &utterance,
        |partial| {
            first_partial.get_or_insert_with(|| started.elapsed());
            if args.partials {
                print!("{}", partial.text);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        },
        &cancel,
    )?;
    if args.partials {
        println!();
    }
    if args.bench
        && let Some(at) = first_partial
    {
        println!("first partial after {at:.2?}");
    }
    Ok(transcript)
}

fn report(wav: &str, transcript: &Transcript, args: &Args) {
    if !args.partials {
        println!("{}", transcript.text);
    }
    if !args.bench {
        return;
    }
    println!(
        "{wav}: {:.2?} of audio in {:.2?} ({:.2}x), language {}, {} segments, confidence {:.2}",
        transcript.audio_duration,
        transcript.processing_time,
        transcript.real_time_factor(),
        transcript.language,
        transcript.segments.len(),
        transcript.confidence,
    );
}

/// The accelerator has to be asked for. A build carrying Metal still
/// decodes on the processor until something says so.
fn local_model(args: &Args) -> Result<ModelSpec, String> {
    let mut model = ModelSpec::at(&args.model);
    if let Some(name) = &args.accelerator {
        let accelerator = match name.as_str() {
            "cpu" => Accelerator::Cpu,
            "metal" => Accelerator::Metal,
            "cuda" => Accelerator::Cuda,
            "vulkan" => Accelerator::Vulkan,
            other => return Err(format!("{other} is not an accelerator this knows")),
        };
        model = model.with_accelerator(accelerator);
    }
    if let Some(threads) = args.threads {
        if threads == 0 {
            return Err("a decode needs at least one thread".to_string());
        }
        model = model.with_threads(threads);
    }
    Ok(model)
}

fn parse(mut args: impl Iterator<Item = String>) -> Option<Args> {
    let mut parsed = Args {
        partials: false,
        bench: false,
        language: None,
        accelerator: None,
        threads: None,
        model: String::new(),
        wavs: vec![],
    };
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--partials" => parsed.partials = true,
            "--bench" => parsed.bench = true,
            "--language" => parsed.language = Some(args.next()?),
            "--accelerator" => parsed.accelerator = Some(args.next()?),
            "--threads" => parsed.threads = Some(args.next()?.parse().ok()?),
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() < 2 {
        return None;
    }
    parsed.model = positional.remove(0);
    parsed.wavs = positional;
    Some(parsed)
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
