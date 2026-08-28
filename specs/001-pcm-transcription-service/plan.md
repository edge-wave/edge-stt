# Implementation Plan: PCM Transcription Service

**Branch**: `main` (no feature branch; spec directory is the unit of work) | **Date**: 2026-08-28 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-pcm-transcription-service/spec.md`

## Summary

edge-stt turns a finished PCM utterance — the thing edge-ear hands over when someone stops
talking — into text, either on the device or on a server the same project ships. Both paths
run Whisper through whisper.cpp, so the two cannot drift apart in accuracy or in the shape of
what they return. The client half is a Rust library shaped exactly like edge-ear (a core, a C
API, a Python binding); the server is a fourth crate in the same workspace that calls the same
core. Partial transcripts fall out of whisper.cpp's own segment callback and are relayed over a
WebSocket unchanged, which is what lets the remote and local paths present one interface.

## Technical Context

**Language/Version**: Rust 1.97.1, edition 2024, MSRV 1.97 — pinned to match edge-ear exactly
(`rust-toolchain.toml`, `clippy.toml`), so a developer moves between the two repositories
without changing toolchains.

**Primary Dependencies**:

| Crate | Version | Why |
|---|---|---|
| `whisper-rs` | 0.16 | whisper.cpp bindings. Gives segment callbacks, abort callbacks, language forcing, and quantised GGML models — see research.md |
| `thiserror` | 2 | Typed errors, as in edge-ear |
| `log` | 0.4 | edge-ear logs through `log`; matching it means one logger for a program using both |
| `tokio` | 1.53 | Server runtime and remote client. Behind features; the on-device path never links it |
| `axum` | 0.8 | Server HTTP + WebSocket |
| `tokio-tungstenite` | 0.30 | WebSocket client for the remote backend |
| `serde` / `serde_json` | 1 | Wire messages |
| `clap` | 4 | Server binary arguments |
| `pyo3` | 0.29.2 | Python binding, same version as edge-ear |

Deliberately **not** used: `ort`. edge-ear depends on it for ONNX wake-word and VAD models, and
reusing it here was the obvious-looking move, but it would mean writing Whisper's decoding loop,
beam search, tokeniser, and timestamp logic by hand. See research.md.

**Storage**: None. FR-024 and FR-033 require that nothing is retained; there is no database, no
cache directory, and no log of transcribed text. Model files are read-only inputs the operator
supplies.

**Testing**: `cargo test --workspace`, with edge-ear's conventions carried over — integration
tests as named files under `core/tests/`, a C surface test compiled from `capi/tests/surface.c`,
a test that fails when the generated header drifts from the Rust source, and `#[ignore]` on
anything needing real model files (found through `EDGE_STT_MODEL_DIR`, mirroring
`EDGE_EAR_WAKE_DIR`). Python tests under `py/tests` with pytest.

**Target Platform**: Linux (aarch64 and x86-64) and macOS (aarch64) for the library; the same
for the server. Windows is out, as it is for edge-ear.

**Project Type**: Rust workspace — a library with two foreign-language bindings, plus one server
binary.

**Performance Goals**: edge-stt's own target is that it costs almost nothing on top of the model
it runs — within 10% of the same model through its reference tooling (SC-002). Absolute speed
belongs to the model size and the board, and both are the integrator's choice (FR-027), so this
plan fixes no absolute latency figure. What it owes instead:

- A published measurement table for every supported model size on each reference machine —
  real-time factor, time to first partial, peak memory, Korean and English (SC-012).
- One shipped command that reproduces those three figures on whatever board the integrator
  actually has (SC-013), because a Pi 4, a Pi 5, and a Jetson give three different answers.
- Per-transcription reporting of decoding time against audio duration (FR-036), so a running
  system tells its operator whether the choice they made is keeping up.

The measurement task therefore comes early, not late: `examples/transcribe.rs --bench` must
exist and have been run on both reference machines before anything downstream quotes a number.

**Constraints**:

- The on-device path must make no network connection at all (FR-008, SC-011). Enforced
  structurally: `tokio`, `axum`, and `tokio-tungstenite` sit behind the `remote` feature, so
  with default features the binary contains no socket code to audit.
- No model weights in the repository (FR-026), as in edge-ear.
- Public core API stays synchronous. Whisper decoding is CPU-bound and blocking; making the core
  async would push a runtime into the C and Python bindings for no gain.

**Scale/Scope**: One utterance at a time per transcriber on the device; 8 concurrent clients on
the server; utterances of seconds to a few minutes. Roughly 4 crates and, judging by edge-ear's
comparable surface, a few thousand lines.

### A wall research found, and where the decision for it lives

On a Pi-class board the Whisper sizes that beat real time (`tiny`, `base`) are poor at Korean,
and the smallest size that handles Korean acceptably (`small`) does not beat real time. No
implementation choice changes this; it is the model's own arithmetic.

The first draft of this plan proposed resolving it in the specification, by promising
on-device real-time for English and not for Korean. That was wrong. Which trade to make depends
on the board, the language, and what the product is willing to wait for — none of which this
project knows, and all of which the integrator does. Baking one answer into the specification
would take a choice away from the only person able to make it.

So SC-002 was rewritten to measure what edge-stt actually controls — its overhead on top of the
model — and the trade itself became an API surface:

| What the integrator gets | Where |
|---|---|
| Model size, thread count, and accelerator, chosen at construction | FR-027 |
| No refusal on the grounds that a combination will be slow | FR-028 |
| Measured numbers per model size per reference machine, both languages | SC-012 |
| One command to get the same numbers on their own board | SC-013 |
| Real-time factor reported on every transcription at runtime | FR-036 |

The Korean wall is still real, and it is documented in
[research.md](./research.md#7-reference-hardware-and-where-the-speed-choice-belongs) so nobody
rediscovers it the hard way. It is stated there as a measurement, not as a limit the library
imposes.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**`.specify/memory/constitution.md` is an unfilled template** — every principle is still a
`[PLACEHOLDER]`. There is nothing to check against. Rather than declare the gate vacuously
passed, the gates below are taken from what edge-ear actually enforces in `CONTRIBUTING.md` and
its README, since edge-stt is its sibling and should not contradict it. Running
`/speckit-constitution` to ratify these would make this section real.

| Derived gate (source) | Status | How this plan meets it |
|---|---|---|
| No number or label from a planning document appears in code, comments, or commits (CONTRIBUTING) | PASS | Contracts describe behaviour in sentences. No `FR-0xx` string appears in any planned source file; the traceability tables live here, in the planning documents |
| Comments are few, at most three lines, and say why (CONTRIBUTING) | PASS | Carried into the contracts as the documentation style |
| No model weights committed; anything added to assets gets a licence, checksum, and provenance in THIRD-PARTY-LICENSES (CONTRIBUTING) | PASS | FR-026 — the operator supplies GGML files. The repository holds none |
| Model inputs and outputs are probed and pinned in a test before being trusted (CONTRIBUTING) | PASS | A test asserts the loaded model's language set, sample rate, and vocabulary size before any transcription test runs |
| clippy with `-D warnings`, `fmt --check`, and the full test suite pass before a pull request (CONTRIBUTING) | PASS | Same three commands; carried into quickstart.md |
| Tests needing real hardware or model files are `#[ignore]` (CONTRIBUTING) | PASS | `EDGE_STT_MODEL_DIR` gates them |
| An optional backend is a Cargo feature, so its dependency is not forced on everyone (`cpal-backend` precedent) | PASS | `whisper` and `remote` are both features; `remote` is off by default |
| **"Nothing leaves the machine. There is no network code in it and no place to put any."** (edge-ear README) | **DIVERGES — justified** | edge-stt exists partly to reach a server, so it cannot hold this absolutely. It is preserved where it can be: the network is a non-default feature, so a build with default features has no network code in it either, and SC-011 verifies at the OS level rather than by reading the source. See Complexity Tracking |

## Project Structure

### Documentation (this feature)

```text
specs/001-pcm-transcription-service/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── rust-core-api.md
│   ├── websocket-protocol.md
│   └── bindings.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Created by /speckit-tasks, not here
```

### Source Code (repository root)

```text
Cargo.toml                    # Workspace: core, capi, py, server
rust-toolchain.toml           # 1.97.1, copied from edge-ear
clippy.toml                   # msrv = "1.97"
rustfmt.toml                  # edition 2024, max_width 100
CONTRIBUTING.md               # Adapted from edge-ear
THIRD-PARTY-LICENSES          # Whisper model provenance, once any is referenced

core/
├── Cargo.toml                # features: whisper (default), remote
├── src/
│   ├── lib.rs
│   ├── config.rs             # AudioFormat, ModelSpec, Config
│   ├── error.rs              # Error — one variant per named cause
│   ├── transcript.rs         # Transcript, Segment, Partial, PartialKind
│   ├── utterance.rs          # Utterance, format validation
│   ├── backend/
│   │   ├── mod.rs            # The Backend trait both paths implement
│   │   ├── whisper.rs        # whisper.cpp: segment + abort callbacks
│   │   └── remote.rs         # WebSocket client            [feature: remote]
│   ├── wire.rs               # Messages on the socket      [feature: remote]
│   └── fallback.rs           # Remote-then-local, off unless asked for
├── tests/
│   ├── minimal_usage.rs      ├── silence.rs        ├── wrong_shape.rs
│   ├── partials.rs           ├── cancellation.rs   ├── timeout.rs
│   ├── backend_parity.rs     ├── no_network.rs     ├── readme.rs
│   └── soak.rs
└── examples/
    ├── transcribe.rs         # Reads a wav, prints the text
    └── probe_model.rs        # Prints a model's real parameters, as edge-ear does

capi/
├── cbindgen.toml
├── include/edge_stt.h        # Generated; a test fails when it drifts
├── src/{lib,error,convert,partials}.rs
├── tests/{surface.c, c_surface.rs, header_is_current.rs}
└── examples/transcribe.c

py/
├── Cargo.toml                # pyo3, abi3-py39
├── pyproject.toml            # maturin
├── src/lib.rs
└── tests/

server/
├── Cargo.toml                # axum, tokio; depends on core with the whisper feature
├── src/
│   ├── main.rs               # clap arguments, bind address, model path, credential
│   ├── session.rs            # One client's utterance, partials, and cancellation
│   ├── capacity.rs           # The queue, and telling clients where they are in it
│   ├── auth.rs               # Bearer credential on the handshake
│   └── health.rs             # Alive, and ready once the model has loaded
└── tests/
    ├── concurrent_clients.rs ├── rejects_bad_credential.rs
    ├── at_capacity.rs        └── client_vanishes.rs
```

**Structure Decision**: edge-ear's three-crate layout (`core`, `capi`, `py`) carried over
verbatim, plus a `server` crate. The server is a separate crate rather than a feature of `core`
because it is a binary with a web framework behind it, and nobody embedding the library on a
Raspberry Pi should compile axum. It depends on `core` rather than reimplementing anything,
which is what makes FR-018's promise — the same transcript shape from both backends —
structurally true instead of a thing to remember.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| A fourth crate (`server`) where edge-ear has three | FR-017 makes the server a deliverable of this project | Putting the server behind a feature of `core` would pull axum and tokio into the dependency graph of every embedder who reads the feature list wrong, on exactly the small devices that can least afford it. A separate crate cannot be enabled by accident |
| Network code exists at all, against edge-ear's stated absolute | The remote backend is the point of User Story 2, and the user chose to ship the server too | Keeping edge-stt purely local would have meant the remote half lived in the caller's code, where the shared transcript shape of FR-018 could not be enforced. The absolute is preserved as far as it can be: `remote` is off by default, so a default build still contains no network code |
| Two async worlds — a blocking core and a tokio server | Whisper decoding is blocking and the C and Python bindings must stay runtime-free; the server needs concurrency | Making `core` async would force a runtime on every embedder, including the C API, where an executor cannot be expressed. The seam is one `spawn_blocking` in the server |
