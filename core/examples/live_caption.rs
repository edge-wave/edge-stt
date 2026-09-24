//! Show words while the speaker is still talking, on the device or from
//! a server.
//!
//! Feeds a recording in at the rate a microphone would produce it and
//! prints every interim as it arrives, so the wait is visible. With
//! `--server`, the server runs the session and the recording's end is
//! where the caller says speech stopped.
//!
//! usage: live_caption [--language ko] [--pause-tolerance-ms N]
//!                     [--interim-ms N] [--threads N]
//!                     <model> <vad model> <wav>
//!        live_caption --server ws://host:8000/api/v1/transcribe
//!                     [--interim-ms N] <wav>

use std::process::ExitCode;
use std::time::{Duration, Instant};

use edge_stt_core::{
    Config, EdgeStt, EndpointConfig, Language, ModelSpec, Partial, RemoteConfig, SessionConfig,
};

const SAMPLE_RATE: usize = 16_000;

/// A tenth of a second, the shape a capture callback usually hands over.
const CHUNK: usize = SAMPLE_RATE / 10;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut language = None;
    let mut pause_ms = 3000u64;
    let mut interim_ms = 300u64;
    let mut threads = None;
    let mut server = None;
    let mut positional = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => server = args.next(),
            "--language" => language = args.next(),
            "--pause-tolerance-ms" => {
                pause_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(pause_ms)
            }
            "--interim-ms" => {
                interim_ms = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(interim_ms)
            }
            "--threads" => threads = args.next().and_then(|v| v.parse::<u16>().ok()),
            _ => positional.push(arg),
        }
    }
    let wanted = if server.is_some() { 1 } else { 3 };
    if positional.len() != wanted {
        eprintln!(
            "usage: live_caption [--language ko] [--pause-tolerance-ms N] \
             [--interim-ms N] [--threads N] <model> <vad model> <wav>\n       \
             live_caption --server <endpoint> [--interim-ms N] <wav>"
        );
        return ExitCode::FAILURE;
    }

    let samples = match read_wav(&positional[wanted - 1]) {
        Ok(samples) => samples,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let mut config = match &server {
        Some(endpoint) => Config::remote(RemoteConfig::at(endpoint)),
        None => {
            let mut model = ModelSpec::at(&positional[0]);
            if let Some(count) = threads {
                model = model.with_threads(count);
            }
            Config::local(model)
        }
    };
    if let Some(tag) = &language {
        config = config.with_language(Language::new(tag));
    }

    let stt = match EdgeStt::new(config) {
        Ok(stt) => stt,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let session = match server {
        Some(_) => SessionConfig::new().with_caller_boundaries(),
        None => SessionConfig::new().with_endpointing(
            EndpointConfig::new()
                .with_local_vad_model(&positional[1])
                .with_pause_tolerance(Duration::from_millis(pause_ms)),
        ),
    };
    let session = session
        .with_live_interims()
        .with_interim_min_interval(Duration::from_millis(interim_ms));

    let mut session = match stt.open_session(session) {
        Ok(session) => session,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let held = Duration::from_secs_f64(samples.len() as f64 / SAMPLE_RATE as f64);
    println!("feeding {held:.2?} of audio at the rate it would be captured");
    let started = Instant::now();

    for (index, chunk) in samples.chunks(CHUNK).enumerate() {
        let mut show = |partial: Partial| {
            println!("  {:>8.2?}  ...  {}", started.elapsed(), partial.text);
        };
        match session.push(chunk, Some(&mut show)) {
            Ok(Some(transcript)) => {
                println!("  {:>8.2?}  ==   {}", started.elapsed(), transcript.text);
            }
            Ok(None) => {}
            Err(why) => {
                eprintln!("{why}");
                return ExitCode::FAILURE;
            }
        }
        // Held to when a microphone would have handed the next chunk
        // over, so falling behind shows up as falling behind.
        let due = started + Duration::from_millis(100) * (index as u32 + 1);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }

    let mut show = |partial: Partial| {
        println!("  {:>8.2?}  ...  {}", started.elapsed(), partial.text);
    };
    match session.close(Some(&mut show)) {
        Ok(transcripts) => {
            for transcript in transcripts {
                println!("  {:>8.2?}  ==   {}", started.elapsed(), transcript.text)
            }
        }
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    }

    println!();
    println!("audio ran  {held:.2?}");
    ExitCode::SUCCESS
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
