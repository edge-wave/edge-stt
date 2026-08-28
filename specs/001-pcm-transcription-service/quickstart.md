# Quickstart & Validation Guide

**Date**: 2026-08-28 | **Plan**: [plan.md](./plan.md)

How to build edge-stt, get a first transcript, and check that each user story in the
specification actually holds. Runnable steps and expected outcomes only — the implementation
belongs in `tasks.md`.

## Prerequisites

```bash
# Linux
sudo apt install build-essential cmake pkg-config

# macOS
xcode-select --install
brew install cmake
```

The C++ toolchain and CMake are needed because whisper.cpp is C++. This is the one way edge-stt
is heavier to build than edge-ear, which needs only ALSA headers.

Rust comes from `rust-toolchain.toml` (1.97.1) — no action needed if rustup is installed.

## Model files

The repository ships no model weights, for the same licensing reason edge-ear ships no wake
word. Fetch one and point the tests at it:

```bash
mkdir -p ~/models/whisper
curl -L -o ~/models/whisper/ggml-base-q5_0.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_0.bin

export EDGE_STT_MODEL_DIR=~/models/whisper
```

`base` is enough to see the thing work. For Korean at usable accuracy use `small` or larger, and
read the speed note in [research.md](./research.md#7-reference-hardware-and-where-the-speed-choice-belongs)
before expecting real-time on a small board.

## Build and check

The same three commands edge-ear's CONTRIBUTING requires before a pull request:

```bash
cargo clippy --workspace --all-targets --features full -- -D warnings
cargo fmt --all --check
cargo test --workspace
```

Tests needing a real model file are `#[ignore]`d, so the suite above runs without one. To
include them:

```bash
cargo test --workspace -- --ignored
```

## First transcript

```bash
cargo run --example transcribe -- ~/models/whisper/ggml-base-q5_0.bin sample.wav
```

Expected: the spoken words on stdout, followed by the model, the audio duration, and the
decoding time.

Before trusting a model you have not used here before, print what it really is — the habit
edge-ear's CONTRIBUTING insists on:

```bash
cargo run --example probe_model -- ~/models/whisper/ggml-base-q5_0.bin
```

Expected: multilingual or not, vocabulary size, and expected sample rate. If these disagree with
what you assumed, stop and fix the assumption.

## Validating the user stories

### Story 1 — on-device transcription

```bash
cargo test --workspace --no-default-features --features whisper -- --ignored transcribe
```

Expected: known recordings produce their known transcripts. Silence produces an empty transcript
and passes, rather than erroring.

To check the offline claim rather than trusting it:

```bash
# Linux — no interfaces exist inside the namespace at all
sudo unshare --net cargo test --workspace --no-default-features --features whisper -- --ignored

# macOS — assert the process opened no sockets
cargo test -p edge-stt-core --test no_network -- --ignored
```

Expected: everything passes with no network available. A failure here means something reached
for a socket on the default path, which is the one thing this design must not allow.

### Story 2 — remote transcription

Against the stub server the test suite starts for itself:

```bash
cargo test -p edge-stt-core --features remote --test backend_parity
```

Expected: the same caller code, run against both backends, produces the same transcript shape
and the same text. The test fails if either backend gains a field or a behaviour the other lacks
— which is how FR-018's promise is kept over time rather than at review time.

Fault injection:

```bash
cargo test -p edge-stt-core --features remote --test remote_faults
```

Expected: host down yields `Network`; wrong credential yields `CredentialRejected`; a connection
dropped mid-request yields `Network` and no partial presented as final; a stalled server yields
`Timeout` at the configured limit and never later.

### Story 3 — partial transcripts

```bash
cargo test --workspace --all-features -- --ignored partials
```

Expected, on both backends: the first partial arrives well before the final result; every
partial is marked non-final; concatenating them in `seq` order equals the final text exactly;
nothing arrives after the final result.

By eye:

```bash
cargo run --example transcribe -- --partials ~/models/whisper/ggml-small-q5_0.bin long.wav
```

Expected: text appearing in pieces as it decodes, not all at the end.

### Story 4 — the server

```bash
cargo run -p edge-stt-server -- \
  --model ~/models/whisper/ggml-small-q5_0.bin \
  --bind 0.0.0.0:8000 \
  --credential-file ~/.config/edge-stt/token
```

Then, from another shell:

```bash
curl -s localhost:8000/healthz    # 200 immediately
curl -s localhost:8000/readyz     # 503 while loading, 200 once the model is up
```

Point a client at it by configuration alone — no code change from the Story 1 example:

```bash
EDGE_STT_ENDPOINT=ws://localhost:8000/api/v1/transcribe \
EDGE_STT_TOKEN=$(cat ~/.config/edge-stt/token) \
cargo run --example transcribe --features remote -- sample.wav
```

Expected: the same transcript as Story 1 produced locally, with `backend` reported as remote.

Concurrency and refusal:

```bash
cargo test -p edge-stt-server -- --ignored
```

Expected: 8 clients at once each get their own correct transcript with no cross-talk; a ninth
past capacity is told its queue position or refused with `at_capacity`, never left silent; a bad
credential is refused at the handshake with nothing transcribed; a client that disconnects
mid-utterance stops the server's decoding without disturbing the others.

## Measuring your own board

edge-stt does not tell you which model size to run — that depends on your board, your language,
and how long your product can wait. It gives you the numbers to decide with, on the hardware you
actually have:

```bash
cargo run --release --example transcribe -- --bench MODEL sample_5s.wav sample_30s.wav
```

Reports three figures per model: decoding time against audio duration (the real-time factor),
time to first partial, and peak memory. Run it for each size you are considering, in the
language you care about.

Expect the answers to differ sharply. On a Pi-class board `base` beats real time and is poor at
Korean, while `small` handles Korean and does not beat real time; on the Mac mini with Metal,
`large-v3` beats real time comfortably. Both are fine answers — which one is right is yours to
pick, and nothing in the library will refuse the choice.

The same command produces the table the project publishes for its two reference machines. If
your figures differ wildly from the published ones for comparable hardware, that is worth
reporting.

At runtime, the same information comes back with every transcript, in `processing_time` against
`audio_duration`, so a deployed system keeps telling you whether the choice still holds.

## Python

```bash
cd py && maturin develop
python -c "from edge_stt import EdgeStt; print(EdgeStt(model='$EDGE_STT_MODEL_DIR/ggml-base-q5_0.bin'))"
pytest
```

## C

```bash
cargo build -p edge-stt-capi
cc capi/examples/transcribe.c -Icapi/include -Ltarget/debug -ledge_stt_capi -o /tmp/transcribe
/tmp/transcribe ~/models/whisper/ggml-base-q5_0.bin sample.wav
```

Expected: the same transcript the Rust example printed. If the header is stale, `cargo test -p
edge-stt-capi` fails first and tells you so.
