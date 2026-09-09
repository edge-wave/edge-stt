# edge-stt

Turn a recording into text. On the device, or on a server you run —
the calling code is the same either way.

The other half of [edge-ear](https://github.com/edge-wave/edge-ear),
which listens, hears a wake word, and hands you the recording once the
speaker goes quiet. edge-stt takes that recording and tells you what
was said.

## What it does

- Turns a finished recording into text, using Whisper
- Runs the model on the device, with no network at all
- Or sends the audio to a server you started, chosen by configuration
- Hands you the words as they are decoded, if you want them early
- Reports what each transcript cost, so you can tell whether the
  hardware is keeping up
- With the `streaming` feature, also takes audio with no predetermined
  end and decides for itself where one utterance stops

## What it does not do

- Listen, detect a wake word, or decide when speech ended for a caller
  that hands over pre-cut recordings. That is edge-ear's job — unless
  you turn on `streaming`, see below
- Write replies or turn text into speech
- Ship a model. You supply the file
- Choose a model size for you. Your board, your language, your call
- Run on Windows yet

## Transcribing

```rust
use edge_stt_core::{Config, EdgeStt, ModelSpec, Utterance};

let stt = EdgeStt::new(Config::local(ModelSpec::at("models/ggml-base-q5_1.bin")))?;
let transcript = stt.transcribe(&Utterance::mono_16k(&samples))?;
println!("{}", transcript.text);
```

Nothing above reaches the network, and with default features there is
no network code in the build to reach it with.

A server instead — the configuration changes, the calling code does not:

```rust
let stt = EdgeStt::new(Config::remote(RemoteConfig::at("ws://host:8000/api/v1/transcribe")))?;
```

That needs the `remote` feature, which is off by default.

To see the words as they are decoded, hand over a callback. That is the
whole of asking for them, and it works the same on both backends:

```rust
let cancel = CancelToken::new();
let transcript = stt.transcribe_with(&utterance, |p| print!("{}", p.text), &cancel)?;
```

`cancel` stops one from another thread; a time limit on the config does
the same when it runs out. The two never arrive as the same error.

## Continuous input

For a live feed with no predetermined end — a microphone, not a
finished recording — the `streaming` feature adds a second entry
point that decides utterance boundaries itself, using whisper.cpp's
own built-in Silero VAD support:

```rust
use edge_stt_core::EndpointConfig;

let mut session = stt.open_session(EndpointConfig::new("models/ggml-silero-v5.1.2.bin"))?;
for chunk in samples.chunks(1_600) {
    if let Some(transcript) = session.push(chunk, None)? {
        println!("{}", transcript.text);
    }
}
if let Some(transcript) = session.close(None)? {
    println!("{}", transcript.text); // whatever was still in progress
}
```

It works the same way against the remote backend: the server, not the
caller, decides the boundaries there. `vad_model` is a second, separate
model file — see [Models](#models) below — and `pause_tolerance`
(`EndpointConfig::with_pause_tolerance`) trades responsiveness against
the risk of splitting a natural mid-sentence pause; the default is a
few seconds, the same order of magnitude edge-ear uses for its own
end-of-speech detection.

### Words while the speaker is still talking

Nothing above reaches a caller until the speaker stops. Ask for interim
results during an utterance and words arrive as they are said, each one
carrying everything heard so far:

```rust
use edge_stt_core::{PartialKind, SessionConfig};

let mut session = stt.open_session(
    SessionConfig::new()
        .with_endpointing(EndpointConfig::new("models/ggml-silero-v5.1.2.bin"))
        .with_live_interims(),
)?;

let mut caption = String::new();
let mut show = |partial: edge_stt_core::Partial| match partial.kind {
    PartialKind::Replace => caption = partial.text,
    PartialKind::Append => caption.push_str(&partial.text),
};
if let Some(transcript) = session.push(chunk, Some(&mut show))? {
    println!("{}", transcript.text); // the answer, from the whole utterance
}
```

**Read `kind` rather than assuming.** A recognizer working on an
unfinished utterance corrects itself as more audio arrives, so its
results *replace* what came before instead of adding to it. The finished
utterance is still recognised in full, and that is where the
`Transcript` comes from — an interim is never promoted, and never a
commitment.

**It is not free, which is why it is asked for separately.** Recognising
a growing utterance means recognising it again and again; registering a
callback does not turn this on by itself, so a caller who never asked
pays nothing. Two conditions bound the cost: an interim is delivered
only when the words changed, and no sooner than
`with_interim_min_interval` allows — a third of a second by default.
What that buys you depends entirely on the machine, and
[measurements.md](docs/measurements.md) has figures for both ends of the
range this project targets.

## The server

The other end of the remote backend, in this repository, running the
same code the library runs:

```bash
cargo run -p edge-stt-server -- \
  --model ~/models/whisper/ggml-base-q5_1.bin \
  --bind 0.0.0.0:8000 \
  --credential-file ~/.config/edge-stt/token
```

`/healthz` answers at once, `/readyz` only once the model has loaded,
so a supervisor cannot kill a server that is working. The credential is
checked on the handshake; without `--credential-file` the server
refuses to start unless you say `--open-to-anyone` out loud.

**Serving more than one live caption at a time needs `--threads`.**
Whisper hands every recognition a thread per core, so two live sessions
on one machine spin against each other at every node of the graph and
both crawl. Dividing the cores between the callers you mean to serve —
`--threads 2` on a four-core board, say — is what stops that. It is not
the default because it costs a lone caller speed, and a server told
`--vad-model` without it says so on startup.

## Models

| What | Who supplies it |
|------|-----------------|
| Whisper weights | You do |
| VAD weights (`streaming` feature only) | You do — a second, separate file |

Whisper is MIT, and the GGML files whisper.cpp reads are published
alongside it. Which size to run is your decision — a small board and a
large model is a choice this library will not refuse, and every
transcript tells you what it cost.

Continuous audio input (the `streaming` feature) needs a second model: a Silero VAD converted to
GGML and published by whisper.cpp's own maintainers at `ggml-org/whisper-vad` on Hugging Face.
It decides where one utterance ends and the next begins, and it is not the Whisper file above —
neither is bundled in this repository.

```bash
export EDGE_STT_MODEL_DIR=~/models/whisper
scripts/fetch-model.sh --list
scripts/fetch-model.sh base-q5_1
```

The file is checked against the SHA-256 in `THIRD-PARTY-LICENSES` and
deleted if it does not match. Weights never enter this repository:
`models/`, `*.bin`, `*.gguf`, and `*.ggml` are ignored anywhere in the
tree, and a build fails if one is ever tracked.

The device and the server do not run the same file.
[docs/models.md](docs/models.md) says which belongs where, what an
English-only model refuses, and where Korean runs into the model's own
arithmetic. [docs/measurements.md](docs/measurements.md) has what makes
Whisper slow and how to take the numbers on your own board.

## Building

whisper.cpp is C++, so this needs a C++ toolchain and CMake. edge-ear
does not, and that is the one way edge-stt is heavier to build.

```bash
sudo apt install build-essential cmake pkg-config   # Linux
xcode-select --install && brew install cmake        # macOS

cargo build --workspace
cargo test --workspace --features full
```

`full` is whisper plus remote. Not `--all-features`: that turns on the
CUDA and Vulkan accelerators, which need toolchains most machines lack.

Tests needing a real model are ignored by default, and want a release
build — whisper.cpp compiled for debugging is unusably slow:

```bash
export EDGE_STT_MODEL_DIR=~/models/whisper
export EDGE_STT_SAMPLE_WAV=speech.wav EDGE_STT_SAMPLE_TEXT="what is said in it"
export EDGE_STT_SAMPLE_LANGUAGE=en EDGE_STT_ACCELERATOR=metal
cargo test --release --workspace --features full -- --ignored
```

Without `EDGE_STT_ACCELERATOR` they decode on the processor, which on a
shared machine is slow enough to look like a hang. The build has to
carry the accelerator you name.

## Python

```bash
cd py && maturin develop
```

```python
from edge_stt import EdgeStt

with EdgeStt(model="models/ggml-base-q5_1.bin") as stt:
    print(stt.transcribe(samples).text)
```

`samples` may be bytes, a list of ints, or a numpy `int16` array. Each
failure has its own exception class under `EdgeSttError`, so
`except NetworkError` works without reading a message.

Continuous input works the same way as the Rust API:

```python
with stt.open_session(vad_model="models/ggml-silero-v5.1.2.bin") as session:
    for chunk in microphone_chunks():
        transcript = session.push(chunk)
        if transcript is not None:
            print(transcript.text)
```

Both `push` and `close` take an optional `on_partial` callback, same as the Rust and C APIs
-- the utterance `close` finalizes decodes the same way `push` does, so interim results can
still arrive from it too.

## C

```bash
cargo build -p edge-stt-capi
cc capi/examples/transcribe.c -Icapi/include -Ltarget/debug -ledge_stt_capi -o transcribe
```

`capi/include/edge_stt.h` is generated by cbindgen and committed. Do
not edit it: write the Rust doc comment and regenerate. `capi/Doxyfile`
turns the same header into HTML.

Continuous input is a second handle, opened from the first:

```c
edge_stt_session_h session = edge_stt_session_new(stt, "models/ggml-silero-v5.1.2.bin", 0);
edge_stt_session_set_transcript_cb(session, on_transcript, NULL);
edge_stt_session_push(session, samples, count);
/* ...more pushes as audio arrives... */
edge_stt_session_close(session);
edge_stt_session_free(session);
```

`on_transcript` receives an owned `edge_stt_transcript_h` per finished
utterance, freed the same way `edge_stt_transcribe`'s does. `0` for
the last argument to `edge_stt_session_new` means the documented
default pause tolerance. `edge_stt_session_set_partial_cb` is this
session's own partial slot, independent of the handle's -- set it the
same way if interim results are wanted while a push or close decodes.

## Licence

MIT or Apache-2.0, your choice.
