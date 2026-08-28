# Contract: Rust core API

**Crate**: `edge-stt-core` | **Stability**: this is the surface the C API and the Python binding
are generated from; changing it changes all three.

The shape follows edge-ear: build a thing, then use it, with callbacks for what arrives over
time. A caller who has used edge-ear should not have to learn a second idiom.

## The smallest useful program

```rust
use edge_stt_core::{EdgeStt, Config, ModelSpec, Utterance};

let stt = EdgeStt::new(
    Config::local(ModelSpec::at("models/ggml-base-q5_1.bin"))
)?;

let transcript = stt.transcribe(&Utterance::mono_16k(&samples))?;
println!("{}", transcript.text);
```

Three lines to a transcript, no network, no runtime. That is the contract the rest of this
document must not spoil.

## Construction

```rust
impl EdgeStt {
    pub fn new(config: Config) -> Result<Self>;
    pub fn backend_kind(&self) -> BackendKind;
}
```

**Guarantees**

- `new` loads the model and verifies its parameters. A missing file, a corrupt file, or a
  monolingual model where a multilingual one is needed fails here — never later, on the first
  spoken word.
- `new` for a remote transcriber fails when no endpoint was configured. There is no default
  endpoint to fall back to.
- `new` takes a `Config`, where edge-ear's `EdgeEar::new()` takes nothing and is configured
  afterwards. The divergence is deliberate: an ear with no wake model still reads audio, but a
  transcriber with no model can do nothing at all, so requiring it up front turns a runtime
  failure into a compile-time obligation.
- Building is the only place where "local or remote" is decided. No method below behaves
  differently by backend, apart from which errors it can return.

## Transcribing

```rust
impl EdgeStt {
    pub fn transcribe(&self, utterance: &Utterance)
        -> Result<Transcript>;

    pub fn transcribe_with(
        &self,
        utterance: &Utterance,
        on_partial: impl FnMut(Partial),
        cancel: &CancelToken,
    ) -> Result<Transcript>;
}
```

**Guarantees**

- Both block until there is a transcript or an error. Whisper decoding is CPU-bound; a caller
  who wants it off the current thread owns that choice.
- `transcribe` costs nothing extra for partials it did not ask for — no callback is registered
  with the decoder at all.
- Partials arrive on the calling thread, from inside `transcribe_with`, before it returns. A
  callback is never invoked after the function has returned, so a caller can borrow freely
  without wondering about lifetime.
- The final `Transcript::text` equals the concatenation of every `Append` partial delivered.
- After an error or a cancellation, no further partial is delivered.
- `&self`, not `&mut self`: an `EdgeStt` is shareable, and the server relies on this to serve
  several clients from one loaded model.

## Cancelling and time limits

```rust
let cancel = CancelToken::new();
let handle = cancel.clone();
std::thread::spawn(move || { handle.cancel(); });

match stt.transcribe_with(&utterance, |p| print!("{}", p.text), &cancel) {
    Err(Error::Cancelled) => {}
    other => { other?; }
}
```

**Guarantees**

- Cancellation is observed within roughly one segment's decoding, not at the end of the
  utterance. On the local backend the token is polled from whisper.cpp's abort callback; on the
  remote backend it sends a cancel message and drops the socket.
- A cancelled transcription frees what it held, on the server too when that is where it ran.
- `timeout` in the configuration produces `Timeout`, distinct from `Cancelled`, because one is
  a fault and the other was asked for.

## What comes back

See [data-model.md](../data-model.md) for the fields. The contract on top of them:

- Silence yields `Ok` with empty text, never `Err`.
- `Transcript::backend` reports where the work actually happened, which after a fallback is not
  what the configuration asked for.
- `processing_time` and `audio_duration` are always both set, so the operator can compute a
  real-time factor without timing the call themselves.

## Errors

`Result<T>` is the crate's own alias over `Error`, as it is in edge-ear, so signatures name the
error type once and never again.


Every variant in [data-model.md](../data-model.md#error) is reachable, and each is
a distinct variant precisely because the specification requires a caller to tell them apart.
Two rules bind the implementation:

- No error carries transcribed text or audio, so a caller can log errors freely.
- `Network` and `CredentialRejected` are never collapsed into one variant.

## Features

| Feature | Default | Brings |
|---|---|---|
| `whisper` | on | The on-device backend, and with it whisper.cpp |
| `remote` | **off** | The remote backend, and with it tokio and a WebSocket client |

With default features there is no network code in the built artifact. That is the property
SC-011 verifies, and the reason `remote` is off rather than on.
