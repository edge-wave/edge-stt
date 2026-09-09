//! Measure what one recognition pass over a partial utterance costs,
//! and what tightening the encoder bound does to the words.
//!
//! usage: probe_window [--seconds 1,2,4,8] [--audio-ctx 0,100,200,1500,live]
//!                     [--language ko] [--threads N] [--carry-prompt]
//!                     [--accelerator cpu|metal|cuda|vulkan] <model> <wav>

use std::process::ExitCode;

use edge_stt_core::backend::whisper::{WhisperBackend, route_logs};
use edge_stt_core::live::whisper_window::{bound_for, settled};
use edge_stt_core::{Accelerator, Config, Language, ModelSpec};

const USAGE: &str = "usage: probe_window [--seconds 1,2,4,8] \
                     [--audio-ctx 0,100,200,1500,live] [--language ko] [--threads N] \
                     [--carry-prompt] [--accelerator cpu|metal|cuda|vulkan] <model> <wav>";

const SAMPLE_RATE: usize = 16_000;

/// Whisper always encodes a thirty-second window as fifteen hundred
/// frames, so this many frames is what one second of audio is worth.
const FRAMES_PER_SECOND: f64 = 50.0;

/// What to bound the encoder to. `Live` asks the live recogniser for
/// its own answer, so a measurement cannot drift away from what ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Fixed(i32),
    Live,
}

struct Args {
    seconds: Vec<f64>,
    audio_ctx: Vec<Bound>,
    carry_prompt: bool,
    language: Option<String>,
    threads: Option<u16>,
    accelerator: Option<String>,
    model: String,
    wav: String,
}

fn main() -> ExitCode {
    // Timing a pass while whisper.cpp prints thousands of lines
    // measures the printing as much as the recognition.
    route_logs();

    let Some(args) = parse(std::env::args().skip(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let samples = match read_wav(&args.wav) {
        Ok(samples) => samples,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let backend = match load(&args) {
        Ok(backend) => backend,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let held = samples.len() as f64 / SAMPLE_RATE as f64;
    println!("model     {}", args.model);
    println!("audio     {:.1} s from {}", held, args.wav);
    println!();

    // Bound outermost, so a growing buffer is one simulated session and
    // a carried prompt is the previous pass of that same session.
    for ctx in &args.audio_ctx {
        let mut carried: Option<String> = None;
        for seconds in &args.seconds {
            let wanted = (seconds * SAMPLE_RATE as f64) as usize;
            if wanted > samples.len() {
                println!("{seconds:.1} s  not in this recording, skipped");
                continue;
            }
            let window = &samples[..wanted];
            let bound = resolve(*ctx, window.len());
            let prompt = if args.carry_prompt {
                carried.take()
            } else {
                None
            };
            match backend.probe_window(window, bound, prompt.as_deref()) {
                Ok(pass) => {
                    let factor = pass.took.as_secs_f64() / seconds;
                    println!(
                        "{:>5.1} s  ctx {:>9}  {:>10.2?}  {:.2}x realtime",
                        seconds,
                        describe(*ctx, bound, *seconds),
                        pass.took,
                        factor
                    );
                    if let Some(prompt) = &prompt {
                        println!("          primed with {prompt:?}");
                    }
                    println!("          {:?}", pass.text);
                    println!("          {}", boundaries(&pass.segments, *seconds));
                    carried = Some(settled(&pass.text).to_string());
                }
                Err(why) => println!("{seconds:.1} s  ctx {bound:>9}  failed: {why}"),
            }
        }
    }
    ExitCode::SUCCESS
}

/// What this bound comes to for this much audio.
fn resolve(bound: Bound, samples: usize) -> i32 {
    match bound {
        Bound::Fixed(ctx) => ctx,
        Bound::Live => bound_for(samples),
    }
}

/// Where the pass put its segment edges, and how far the last one
/// stops short of the audio. Trimming the fragment at a pass's trailing
/// edge by timing is only possible if there is an edge to trim to.
fn boundaries(segments: &[edge_stt_core::Segment], seconds: f64) -> String {
    if segments.is_empty() {
        return "no segments".to_string();
    }
    let spans: Vec<String> = segments
        .iter()
        .map(|s| format!("{:.2}-{:.2}", s.start.as_secs_f64(), s.end.as_secs_f64()))
        .collect();
    let last = segments.last().map_or(0.0, |s| s.end.as_secs_f64());
    format!(
        "{} segment(s) [{}], ending {:.2} s short of the buffer",
        segments.len(),
        spans.join(" "),
        seconds - last
    )
}

/// Zero is whisper.cpp's own default, the whole window; printing what
/// this much audio is actually worth says how much of it was waste.
fn describe(bound: Bound, resolved: i32, seconds: f64) -> String {
    match bound {
        Bound::Live => format!("live/{resolved}"),
        Bound::Fixed(ctx) if ctx > 0 => ctx.to_string(),
        Bound::Fixed(_) => format!("full/{}", (seconds * FRAMES_PER_SECOND).ceil() as i32),
    }
}

fn load(args: &Args) -> Result<WhisperBackend, String> {
    let mut model = ModelSpec::at(&args.model);
    if let Some(threads) = args.threads {
        model = model.with_threads(threads);
    }
    if let Some(name) = &args.accelerator {
        model = match name.as_str() {
            "cpu" => model.with_accelerator(Accelerator::Cpu),
            "metal" => model.with_accelerator(Accelerator::Metal),
            "cuda" => model.with_accelerator(Accelerator::Cuda),
            "vulkan" => model.with_accelerator(Accelerator::Vulkan),
            other => return Err(format!("{other} is not an accelerator this knows")),
        };
    }

    let mut config = Config::local(model.clone());
    if let Some(tag) = &args.language {
        config = config.with_language(Language::new(tag));
    }
    WhisperBackend::load(&model, &config).map_err(|why| why.to_string())
}

fn parse(mut args: impl Iterator<Item = String>) -> Option<Args> {
    let mut seconds = vec![1.0, 2.0, 4.0, 8.0];
    let mut audio_ctx = vec![Bound::Fixed(0)];
    let mut carry_prompt = false;
    let mut language = None;
    let mut threads = None;
    let mut accelerator = None;
    let mut positional = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => seconds = numbers(&args.next()?)?,
            "--audio-ctx" => audio_ctx = bounds(&args.next()?)?,
            "--carry-prompt" => carry_prompt = true,
            "--language" => language = Some(args.next()?),
            "--threads" => threads = Some(args.next()?.parse().ok()?),
            "--accelerator" => accelerator = Some(args.next()?),
            _ => positional.push(arg),
        }
    }

    if positional.len() != 2 || seconds.is_empty() || audio_ctx.is_empty() {
        return None;
    }
    Some(Args {
        seconds,
        audio_ctx,
        carry_prompt,
        language,
        threads,
        accelerator,
        model: positional.remove(0),
        wav: positional.remove(0),
    })
}

/// A bound is a frame count, or `live` for whatever the live
/// recogniser would choose for that much audio.
fn bounds(list: &str) -> Option<Vec<Bound>> {
    list.split(',')
        .map(|item| match item.trim() {
            "live" => Some(Bound::Live),
            number => number.parse().ok().map(Bound::Fixed),
        })
        .collect()
}

fn numbers(list: &str) -> Option<Vec<f64>> {
    list.split(',').map(|n| n.trim().parse().ok()).collect()
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
