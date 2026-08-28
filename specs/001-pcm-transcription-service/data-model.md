# Data Model: PCM Transcription Service

**Date**: 2026-08-28 | **Plan**: [plan.md](./plan.md) | **Spec**: [spec.md](./spec.md)

The entities the specification names, given fields, rules, and lifetimes. Rust types are shown
because they are the definition the C and Python bindings and the wire format are all derived
from; nothing here is a wire format on its own — that is
[websocket-protocol.md](./contracts/websocket-protocol.md).

---

## AudioFormat

The shape of a block of samples. Deliberately the same three fields edge-ear's `AudioFormat`
carries, so a value can cross from one library to the other without a conversion nobody
remembers to write.

| Field | Type | Notes |
|---|---|---|
| `sample_rate` | `u32` | Hz |
| `channels` | `u16` | 1 for everything supported today |
| `sample_type` | `SampleType` | `I16` or `F32` |

**Rules**

- `AudioFormat::mono_16k()` — 16 kHz, 1 channel, `I16` — is the default and the only shape
  transcription accepts in the first release. It is what edge-ear produces at end of speech.
- Anything else is rejected at submission, with the expected and received shapes both named in
  the error. No resampling, no channel mixing.

---

## Utterance

One complete recording handed over for transcription. edge-stt never captures audio; an
Utterance always arrives from outside.

| Field | Type | Notes |
|---|---|---|
| `samples` | `Vec<i16>` (borrowed as `&[i16]` at the API edge) | Interleaved, though only mono is accepted |
| `format` | `AudioFormat` | Must satisfy the rules above |
| `captured_at` | `Option<SystemTime>` | Passed through to the result; edge-stt does not interpret it |

**Rules**

- Duration is derived, never stored: `samples.len() / sample_rate`.
- Shorter than 0.3 s is accepted and treated as silence, not rejected — a false wake word should
  produce an empty transcript, not an error.
- Longer than the configured maximum (default 300 s) is rejected up front, naming both the limit
  and the actual duration. Never truncated.

---

## ModelSpec

Which model files a backend should load, and how hard it should work.

| Field | Type | Notes |
|---|---|---|
| `path` | `PathBuf` | The GGML file the operator supplies. No default, no download |
| `size_hint` | `Option<ModelSize>` | `Tiny`/`Base`/`Small`/`Medium`/`LargeV3`, for diagnostics and for the model-parameter test |
| `threads` | `Option<u16>` | Defaults to the number of physical cores |
| `accelerator` | `Option<Accelerator>` | `Cpu`, `Metal`, `Cuda`, `Vulkan`. Defaults to the best the build supports. An accelerator the build lacks is an error at construction, not a silent fall back to the CPU |

**Rules**

- The file is opened and the model loaded when the transcriber is built, not on first use, so a
  missing or corrupt file fails at startup.
- No combination is refused for being slow. A large model on a small board is a choice the
  integrator is allowed to make; the cost comes back in every `Transcript`.
- Loading verifies the model's real parameters — multilingual or not, vocabulary size, expected
  sample rate — and refuses a model whose language set cannot serve the configured language.
  edge-ear's `probe_model` habit, applied here.

---

## Config

Everything a caller sets before transcribing. This is the only place the on-device and remote
choice appears; nothing downstream of it differs.

| Field | Type | Default | Notes |
|---|---|---|---|
| `backend` | `BackendChoice` | `Local` | `Local(ModelSpec)` or `Remote(RemoteConfig)` |
| `fallback_to_local` | `Option<ModelSpec>` | `None` | Off unless asked for |
| `language` | `Option<Language>` | `None` — detect | Forces the decoding language when set |
| `want_partials` | `bool` | `false` | Opt-in, so a caller that ignores them pays nothing |
| `timeout` | `Option<Duration>` | `None` | Applies to a whole transcription, both backends |
| `max_duration` | `Duration` | 300 s | The Utterance limit above |

---

## RemoteConfig

| Field | Type | Notes |
|---|---|---|
| `endpoint` | `Url` | **No default.** Construction fails without it — there is no built-in server address |
| `credential` | `Option<Secret<String>>` | Sent as a bearer token on the handshake. Never logged, and its `Debug` prints a placeholder |
| `connect_timeout` | `Duration` | Separate from the transcription timeout, so an unreachable host is distinguishable from a slow one |

---

## Transcript

What was said, and enough about how it was produced to act on it.

| Field | Type | Notes |
|---|---|---|
| `text` | `String` | The whole utterance, segments joined and trimmed |
| `segments` | `Vec<Segment>` | Empty when the audio held no speech |
| `language` | `Language` | What the model settled on, whether forced or detected |
| `confidence` | `f32` | 0.0–1.0. Language probability when detected; the model's average segment probability otherwise |
| `audio_duration` | `Duration` | What the transcript covers |
| `processing_time` | `Duration` | How long it took — with the field above, this is the real-time factor an operator needs |
| `backend` | `BackendKind` | `Local` or `Remote`, so a caller can tell where a result came from, including after a fallback |

**Rules**

- Silence produces a Transcript with empty `text`, empty `segments`, and a real
  `audio_duration` — never an error, and never invented words.
- `text` is exactly the concatenation of `segments`, so a caller can use either without them
  disagreeing.

## Segment

| Field | Type | Notes |
|---|---|---|
| `text` | `String` | |
| `start`, `end` | `Duration` | Offsets from the beginning of the utterance |
| `confidence` | `f32` | |

Segment-level timing only. Word-level timestamps are out of scope for the first release.

---

## Partial

An interim view, delivered while decoding continues. Never a Transcript, and the type makes that
impossible to confuse.

| Field | Type | Notes |
|---|---|---|
| `seq` | `u32` | From 0, increasing by one, so a gap over the network is detectable |
| `kind` | `PartialKind` | `Append` or `Replace` |
| `text` | `String` | The new text for `Append`; the whole transcript so far for `Replace` |
| `segment` | `Option<Segment>` | Timing, when the backend knows it |

**Rules**

- whisper.cpp does not revise a segment it has emitted, so the first release only ever produces
  `Append`. `Replace` exists for a future streaming runtime that does revise; see research.md.
- Concatenating every `Append` in `seq` order yields the final `Transcript::text`. This is
  asserted in a test, not merely intended.
- No Partial is delivered after the final result, or after a failure or cancellation.

---

## Backend

The trait both paths implement. It is what makes FR-007's promise — switch by configuration,
change no calling code — a property of the type system rather than a discipline.

```rust
pub trait Backend: Send {
    fn transcribe(
        &mut self,
        utterance: &Utterance,
        on_partial: Option<&mut dyn FnMut(Partial)>,
        cancel: &CancelToken,
    ) -> Result<Transcript>;

    fn kind(&self) -> BackendKind;
}
```

Implementations: `WhisperBackend` (feature `whisper`, on by default), `RemoteBackend` (feature
`remote`, off by default), and `FallbackBackend`, which holds both and is only constructed when
the caller asked for fallback.

---

## CancelToken

A shared flag a caller keeps a handle to. `WhisperBackend` polls it from whisper.cpp's abort
callback; `RemoteBackend` sends a cancel message and drops the socket; the server checks it
between segments.

| Method | Notes |
|---|---|
| `cancel()` | Idempotent |
| `is_cancelled()` | |

Cancelling before transcription starts is not an error — it produces `Cancelled` immediately,
which is the sane answer when a wake word turns out to have been a false positive.

---

## Error

One variant per cause the specification requires a caller to be able to tell apart. Every
variant carries what a caller needs to decide between retrying, falling back, and giving up.

| Variant | Carries | Retry? |
|---|---|---|
| `UnsupportedAudio` | expected and received `AudioFormat` | No — the caller's bug |
| `AudioTooLong` | limit, actual | No |
| `ModelMissing` | path searched | No |
| `ModelUnusable` | path, what was wrong | No |
| `InsufficientResources` | model size, what was short | No — try a smaller model |
| `Network` | endpoint, underlying cause | Yes |
| `CredentialRejected` | endpoint | No |
| `ServerAtCapacity` | queue position or retry-after, when the server said | Yes, later |
| `ServerError` | endpoint, the server's message | Maybe |
| `Timeout` | the limit that was hit | Yes |
| `Cancelled` | — | No — this was asked for |

**Rules**

- `Network` and `CredentialRejected` are never merged. A dead host and a bad token call for
  different actions, and the specification requires them to be distinguishable.
- No variant carries transcribed text, so an error can be logged without leaking what was said.

---

## TranscriptionSession *(server only)*

One client's in-flight request. It exists only on the server and never crosses the library API.

| Field | Type | Notes |
|---|---|---|
| `request_id` | `Uuid` | Chosen by the client, echoed in every message about this request |
| `state` | `SessionState` | See below |
| `cancel` | `CancelToken` | Fired by a cancel message, or by the socket closing |
| `next_seq` | `u32` | The partial counter |

**State transitions**

```text
Received ──► Queued ──► Decoding ──► Completed
    │           │           │
    └───────────┴───────────┴──────► Failed
                            │
                            └──────► Cancelled   (client asked, or vanished)
```

- `Queued` is visible to the client, with its position — FR-021's requirement that a client at
  capacity is told, rather than left waiting.
- `Cancelled` and `Failed` are terminal and stop the decoding thread through the token.
- Nothing about a session outlives it. No audio, no transcript, no record.
