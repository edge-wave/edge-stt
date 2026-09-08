//! Push a recording at a server as if a microphone were producing it,
//! and time every reply. Says when words first reach a caller.
//!
//! usage: stream_client <ws://host:port/api/v1/transcribe> <wav> [pause_tolerance_ms]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

const SAMPLE_RATE: usize = 16_000;

/// A tenth of a second, which is the shape a capture callback usually
/// hands over rather than anything this protocol requires.
const CHUNK: usize = SAMPLE_RATE / 10;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: stream_client <ws url> <wav> [pause_tolerance_ms]");
        return ExitCode::FAILURE;
    }
    let pause_ms: u64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(3000);

    let samples = match read_wav(&args[1]) {
        Ok(samples) => samples,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let (mut socket, _) = match tokio_tungstenite::connect_async(&args[0]).await {
        Ok(pair) => pair,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };

    let open = serde_json::json!({
        "type": "open_stream",
        "request_id": "probe",
        "format": { "sample_rate": 16000, "channels": 1, "sample_type": "i16" },
        "want_partials": true,
        "pause_tolerance_ms": pause_ms,
    });
    if socket.send(Message::text(open.to_string())).await.is_err() {
        eprintln!("could not open the stream");
        return ExitCode::FAILURE;
    }

    let started = Instant::now();
    let held = Duration::from_secs_f64(samples.len() as f64 / SAMPLE_RATE as f64);
    println!(
        "pushing {:.2?} of audio at the rate it would be captured",
        held
    );

    // Reading and writing at once, because a reply may arrive at any
    // point during the push and the timing of that is the measurement.
    let (mut writer, mut reader) = socket.split();
    let pushed = samples.clone();
    let sender = tokio::spawn(async move {
        for chunk in pushed.chunks(CHUNK) {
            let mut bytes = Vec::with_capacity(chunk.len() * 2);
            for sample in chunk {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            if writer.send(Message::binary(bytes)).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let close = serde_json::json!({ "type": "close_stream", "request_id": "probe" });
        let _ = writer.send(Message::text(close.to_string())).await;
    });

    let mut first_partial: Option<Duration> = None;
    let mut finals = 0u32;
    // Reading until the far end goes quiet, rather than until the first
    // finished utterance, so a whole recording's worth is visible.
    let idle = Duration::from_secs(20);
    loop {
        let message = match tokio::time::timeout(idle, reader.next()).await {
            Ok(Some(Ok(message))) => message,
            _ => break,
        };
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let at = started.elapsed();
        match value["type"].as_str() {
            Some("partial") => {
                if first_partial.is_none() {
                    first_partial = Some(at);
                }
                println!(
                    "  {:>8.2?}  partial  {:<7} {:?}",
                    at,
                    value["kind"].as_str().unwrap_or("?"),
                    value["text"].as_str().unwrap_or("")
                );
            }
            Some("final") => {
                finals += 1;
                println!(
                    "  {:>8.2?}  final {:<2} {:?}",
                    at,
                    finals,
                    value["text"].as_str().unwrap_or("")
                );
            }
            Some("error") => {
                println!("  {:>8.2?}  error    {}", at, value);
                break;
            }
            _ => {}
        }
    }
    sender.abort();

    println!();
    println!("audio ran               {:.2?}", held);
    match first_partial {
        Some(at) => println!("first words reached us  {:.2?}", at),
        None => println!("first words reached us  never"),
    }
    println!("utterances delivered    {finals}");
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
