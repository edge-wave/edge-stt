//! Start the transcription server.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use edge_stt_core::Accelerator;
use edge_stt_server::{Server, router};

#[derive(Parser, Debug)]
#[command(about = "Turn recordings into text for edge-stt clients")]
struct Args {
    /// The Whisper file to run. Nothing is downloaded for you.
    #[arg(long)]
    model: PathBuf,

    #[arg(long, default_value = "127.0.0.1:8000")]
    bind: String,

    /// A file holding the credential clients must present.
    #[arg(long)]
    credential_file: Option<PathBuf>,

    /// Serve anyone who connects. Say it out loud, or the server will
    /// not start without a credential.
    #[arg(long)]
    open_to_anyone: bool,

    #[arg(long, default_value_t = 8)]
    capacity: usize,

    /// How many threads one recognition may use. Left unset every
    /// session takes every core, so two live callers fight over each.
    #[arg(long)]
    threads: Option<u16>,

    /// Force a language instead of detecting one, or give several, such as
    /// ko,en, to detect among them with the first as the default.
    #[arg(long)]
    language: Option<String>,

    /// A GGML Silero VAD file. Needed only to accept continuous
    /// (open_stream) sessions; ordinary requests don't need it.
    #[arg(long)]
    vad_model: Option<PathBuf>,

    /// Where the model runs: cpu, metal, cuda or vulkan. Anything but
    /// cpu needs the server built with that feature.
    #[arg(long, default_value = "cpu", value_parser = parse_accelerator)]
    accelerator: Accelerator,

    /// Decode each finished utterance with this many candidates instead
    /// of greedily. Words sent while someone is speaking stay greedy.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=16))]
    beam_size: Option<u8>,

    /// A small Whisper file that picks among several --language values,
    /// so detection does not run the large model's encoder a second time.
    #[arg(long)]
    language_model: Option<PathBuf>,

    /// A file of words the speakers are likely to say, handed to each
    /// finished utterance as preceding text so they are spelled that way.
    #[arg(long)]
    prompt_file: Option<PathBuf>,
}

fn parse_accelerator(name: &str) -> Result<Accelerator, String> {
    match name {
        "cpu" => Ok(Accelerator::Cpu),
        "metal" => Ok(Accelerator::Metal),
        "cuda" => Ok(Accelerator::Cuda),
        "vulkan" => Ok(Accelerator::Vulkan),
        other => Err(format!("{other} is not cpu, metal, cuda or vulkan")),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let credential = match &args.credential_file {
        Some(path) => Some(std::fs::read_to_string(path)?.trim().to_string()),
        None if args.open_to_anyone => None,
        None => {
            return Err(
                "no credential; pass --credential-file, or --open-to-anyone to mean it".into(),
            );
        }
    };

    let languages: Vec<String> = args
        .language
        .iter()
        .flat_map(|list| list.split(','))
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    let mut server = Server::new(
        credential,
        languages.first().cloned(),
        args.capacity,
        args.vad_model.clone(),
    );
    // Printed rather than logged, because no logger is installed to show it.
    if let Some(threads) = args.threads {
        server = server.with_threads(threads);
    } else {
        eprintln!(
            "no --threads: each session recognises on every core, which serves one live caller well and several badly"
        );
    }
    let mut server = server
        .with_accelerator(args.accelerator)
        .with_other_languages(languages.iter().skip(1).cloned().collect());
    if let Some(path) = args.language_model.clone() {
        server = server.with_language_model(path);
    }
    if let Some(beam_size) = args.beam_size {
        server = server.with_beam_size(beam_size);
    }
    if let Some(path) = &args.prompt_file {
        let prompt = std::fs::read_to_string(path)?.trim().to_string();
        if !prompt.is_empty() {
            server = server.with_prompt(prompt);
        }
    }
    let server = Arc::new(server);
    server.load_model_in_background(&args.model);

    let address: SocketAddr = args.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    log::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, router(Arc::clone(&server))).await?;
    Ok(())
}
