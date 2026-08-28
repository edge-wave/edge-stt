# Phase 0 Research: PCM Transcription Service

**Date**: 2026-08-28 | **Plan**: [plan.md](./plan.md)

Every unknown the Technical Context started with is resolved below. Crate versions were read
from crates.io on the date above; the whisper-rs API surface was read from its published
documentation rather than recalled.

---

## 1. Which open speech model

**Decision**: OpenAI Whisper, in the GGML/GGUF form whisper.cpp consumes, with the size chosen
by the operator per deployment.

**Rationale**: The user set "use a well-known open model" as the first goal, and Whisper is the
one. It is open-weight under MIT, multilingual including Korean, and it is what the echo-vinci
prototype already runs, so transcripts from the new implementation can be compared against a
working baseline on the same machine. Its size ladder — `tiny` through `large-v3` — is exactly
the accuracy-against-memory dial FR-027 asks for.

**Alternatives considered**:

- **Zipformer / Paraformer streaming models (sherpa-onnx)**: genuinely streaming rather than
  chunked, and much lighter on a small board. Rejected as the primary: neither is "well-known"
  in the sense the user meant, and Korean support is weaker outside Chinese-centric variants.
  Worth revisiting if on-device Korean real-time becomes a hard requirement.
- **Moonshine**: faster than Whisper at small sizes, English-only today. Rejected on Korean.
- **Wav2Vec2 / MMS**: no punctuation, weaker on conversational audio, and needs a language model
  bolted on to be competitive.

---

## 2. Which Whisper runtime

**Decision**: `whisper-rs` 0.16 — safe Rust bindings over whisper.cpp.

**Rationale**: Three things this project needs are already in its API, verified in the published
documentation for `FullParams`:

| Need | The API that satisfies it |
|---|---|
| Partial transcripts as decoding proceeds | `set_segment_callback_safe` — fires per completed segment |
| Cancel an in-flight transcription, and enforce a deadline | `set_abort_callback_safe` — a closure polled during decoding, returning whether to stop |
| Force a language, or let the model detect it | `set_language`, `set_detect_language` |
| Distinguish silence from failure | `set_no_speech_thold` |
| Progress reporting for the operator | `set_progress_callback_safe` |

Building any of these by hand would be weeks of work with a decoding loop to get subtly wrong.
whisper.cpp also carries the accelerators the two reference machines need — Metal on the Mac
mini, plain NEON-optimised CPU on the Pi — behind Cargo features rather than separate code
paths. Quantised models (`q5_0`, `q8_0`) roughly halve memory, which is what makes a Pi-class
board viable at all.

**Alternatives considered**:

- **`ort` (ONNX Runtime), reusing edge-ear's existing dependency**: superficially the most
  attractive option, since it would add no new native dependency to a developer's machine.
  Rejected: `ort` gives tensor in, tensor out. Whisper needs an encoder-decoder loop, beam
  search, a tokeniser, timestamp alignment, and language detection layered on top, all of which
  whisper.cpp already has and none of which is this project's contribution. It would also give
  no segment callback, so FR-012's partials would have to be reinvented.
- **`candle` (pure Rust, with `candle-transformers`' Whisper)**: no C++ toolchain, and pleasant
  to build. Rejected for now: less exercised on quantised CPU inference on ARM, which is the
  case that decides whether the edge device is usable at all. Reconsider if the C++ build
  becomes a real burden.
- **Calling the `faster-whisper` Python process**, as echo-vinci does: fine for a server, fatal
  for a library that has a C API and must run where there is no Python.

**Cost accepted**: whisper.cpp is C++, so building edge-stt needs a C++ toolchain and CMake.
edge-ear needs only `libasound2-dev`. This must be stated plainly in the README, and it is the
one way edge-stt is heavier to build than its sibling.

---

## 3. How partial transcripts actually behave

**Decision**: partials are append-only in the first release. `PartialKind` carries `Append` and
`Replace`, but only `Append` is ever produced.

**Rationale**: whisper.cpp calls the segment callback when a segment is finalised inside the
current 30-second window; it does not go back and revise a segment it has already emitted. So
the honest answer to FR-015 — say whether a partial replaces or extends — is "extends, always",
and the type says so per partial rather than the caller having to know. `Replace` exists in the
type because a future streaming runtime (the Zipformer option above) does revise, and adding a
variant later would be a breaking change to the C API for no reason.

**Consequence for the remote path**: the server relays each partial as its own message with a
sequence number, so a client can detect a gap. It does not batch them. Batching is exactly the
kind of delay SC-005 forbids — the decoder's own pace is the model's business, but holding a
segment the decoder has already produced would be edge-stt's fault.

---

## 4. Client-to-server transport

**Decision**: WebSocket. A JSON text frame opens the request, one binary frame carries the
audio, JSON text frames carry partials and the final transcript. Credential in an
`Authorization: Bearer` header on the handshake.

**Rationale**: The request is one upload followed by several responses over an indeterminate
period, which is exactly what a plain HTTP request handles badly and a WebSocket handles
naturally. echo-vinci already proved this shape works over Tailscale between the same two
machines, including its `stt_update` streamed-partial message, so this is a port of something
observed working rather than a guess.

**Changed from echo-vinci**: audio goes in a binary frame, not the `audio_hex` string that
prototype used. Hex doubles the bytes on the wire, and a 30-second utterance is about 960 kB
raw — a needless 1 MB of extra traffic per utterance on a home network.

**Alternatives considered**:

- **HTTP POST returning one transcript** (echo-vinci's legacy `/api/v1/process`): simplest, and
  cannot deliver partials. Rejected by FR-014, which requires progressive results on both
  backends.
- **Server-sent events with a separate upload**: works, but two round trips and two things to
  correlate, and no clean path to cancellation from the client.
- **gRPC**: bidirectional streaming is the right shape, and it brings a code generator, a proto
  toolchain, and a much worse debugging story on a home network. Not worth it for one endpoint.
- **An OpenAI-compatible `/v1/audio/transcriptions` endpoint**: attractive for interoperability
  and worth adding later as a second, non-streaming route. Not the primary, because the shape
  is one-shot and cannot carry partials.

---

## 5. Keeping the on-device path provably offline

**Decision**: `tokio`, `axum`, and `tokio-tungstenite` are reachable only through the `remote`
Cargo feature, which is off by default. The `no_network.rs` test asserts the offline claim from
outside the process.

**Rationale**: FR-008 and SC-011 ask for something stronger than "we did not write any network
calls". With the network crates absent from a default build, there is no socket code in the
binary to audit — which is as close as this project can get to edge-ear's "no place to put any".
SC-011 then verifies behaviour rather than intent, by watching at the OS level: on Linux the
test runs the offline suite in a network namespace with no interfaces, on macOS it asserts the
process opened no sockets. A code-reading check would pass a build that called out from a
dependency.

---

## 6. Async boundary

**Decision**: `core`'s public API is synchronous and blocking. Async lives in the `remote`
feature and in the `server` crate. The server bridges with `spawn_blocking`.

**Rationale**: Whisper decoding is CPU-bound; wrapping it in a future buys nothing and costs the
C API, which has no way to express an executor, and the Python binding, which would have to pick
one. edge-ear made the same call — its core is threads and callbacks — so a program using both
libraries deals with one model, not two. The remote backend does need a runtime, which is
another reason it sits behind a feature rather than in the default surface.

---

## 7. Reference hardware, and where the speed choice belongs

**Decision**: two reference machines exist so that measurements can be published, not so that
the project can promise a speed. The reference host is the Mac mini M4 Pro that echo-vinci's
server already runs on, with Metal. The reference edge device is a Raspberry Pi 5, four threads,
no accelerator.

**The measurement that matters**: on a Pi-class board, the Whisper sizes fast enough to beat
real time are `tiny` and `base`, and both are poor at Korean; the smallest size that transcribes
Korean acceptably is `small`, which does not beat real time there. That is the model's own cost
and no implementation choice moves it.

**What this project does about it: nothing, deliberately.** The trade between accuracy and speed
depends on the board, the language, and how long the product is willing to wait — the
integrator knows all three and edge-stt knows none of them. A Pi 4, a Pi 5, and a Jetson Orin
give three different answers, and so do Korean and English on the same board. So:

- The knobs are the integrator's: model size, thread count, and accelerator, set at construction
  (FR-027).
- The service never refuses a combination for being slow (FR-028). If someone wants `large-v3`
  on a Pi 4 because a four-second wait is fine for their product, that is their call.
- The project publishes measurements rather than promises: every supported size on each
  reference machine, in both languages, with real-time factor, time to first partial, and peak
  memory (SC-012).
- One shipped command reproduces those figures on whatever board the integrator actually has
  (SC-013), because the reference machines are examples, not the world.
- At runtime, every transcript carries its decoding time against its audio duration (FR-036), so
  a deployed system reports whether the choice is holding up rather than leaving the operator to
  guess.

**The one speed claim edge-stt does make** is about itself: within 10% of the same model run
through its reference tooling on the same machine (SC-002). That is measurable, it is this
project's actual contribution, and it is a real gate — a thin binding over whisper.cpp should
add nothing, so a regression here means something got wrapped badly.

**Consequence for planning**: the benchmark command is an early task, not a final one. No
downstream document should quote a latency number that has not come out of it.

## 8. Versions verified

Read from crates.io on 2026-08-28: `whisper-rs` 0.16.0, `tokio` 1.53.1, `axum` 0.8.9,
`tokio-tungstenite` 0.30.0, `serde` 1.0.229, `serde_json` 1.0.151, `clap` 4.6.6,
`thiserror` 2, `pyo3` 0.29.2 (matched to edge-ear rather than to the latest).

`ort` remains at `2.0.0-rc.13` with no stable release, which is why edge-ear pins it exactly.
edge-stt does not depend on it, so it inherits none of that risk.
