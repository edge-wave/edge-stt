//! Start the transcription server.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
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

    /// Force a language instead of detecting one.
    #[arg(long)]
    language: Option<String>,

    /// A GGML Silero VAD file. Needed only to accept continuous
    /// (open_stream) sessions; ordinary requests don't need it.
    #[arg(long)]
    vad_model: Option<PathBuf>,
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

    let mut server = Server::new(
        credential,
        args.language.clone(),
        args.capacity,
        args.vad_model.clone(),
    );
    if let Some(threads) = args.threads {
        server = server.with_threads(threads);
    } else if args.vad_model.is_some() {
        log::warn!(
            "no --threads: each session recognises on every core, which serves one live caller well and several badly"
        );
    }
    let server = Arc::new(server);
    server.load_model_in_background(&args.model);

    let address: SocketAddr = args.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    log::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, router(Arc::clone(&server))).await?;
    Ok(())
}
