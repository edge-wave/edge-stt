---
description: "Task list for PCM Transcription Service"
---

# Tasks: PCM Transcription Service

**Input**: Design documents from `/specs/001-pcm-transcription-service/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md), [data-model.md](./data-model.md), [contracts/](./contracts/)

**Tests**: Included. Not because tests are always included, but because this feature asked for
them three times over — the specification's success criteria are written as assertions, the
plan's source tree names the test files, and edge-ear's `CONTRIBUTING.md`, which this project
follows, makes a green suite a condition of opening a pull request.

**Organization**: Grouped by user story, so each can be built, tested, and shown on its own.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel — different files, no dependency on unfinished work
- **[Story]**: US1–US4, matching the user stories in spec.md
- Every task names the file it touches

## Path Conventions

A Rust workspace at the repository root, laid out as in
[plan.md](./plan.md#source-code-repository-root): `core/`, `capi/`, `py/`, `server/`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: A workspace that builds, configured the way edge-ear is, so the two feel like one
project.

- [X] T001 Create the workspace `Cargo.toml` at the repository root: members `core`, `capi`, `py`, `server`; `[workspace.package]` with version 0.1.0, edition 2024, rust-version 1.97, license `MIT OR Apache-2.0`, repository `https://github.com/edge-wave/edge-stt`; `[workspace.dependencies]` pinning whisper-rs 0.16, thiserror 2, log 0.4, tokio 1.53, axum 0.8, tokio-tungstenite 0.30, serde 1, serde_json 1, clap 4, uuid 1, pyo3 0.29.2
- [X] T002 [P] Copy edge-ear's toolchain settings verbatim into `rust-toolchain.toml` (channel 1.97.1, components rustfmt and clippy), `clippy.toml` (`msrv = "1.97"`), and `rustfmt.toml` (edition 2024, max_width 100, Unix newlines)
- [X] T003 [P] Add `LICENSE-MIT` and `LICENSE-APACHE` at the repository root, matching edge-ear's dual licence
- [X] T004 [P] Write `THIRD-PARTY-LICENSES` with a Whisper section recording that no weights are shipped, and what a contributor must write down — licence, checksum, origin — before referencing one
- [X] T005 [P] Write `CONTRIBUTING.md` adapted from edge-ear's: say the thing rather than its number, **comments few and at most two lines**, **commit messages at most ten lines**, nothing added to assets without provenance, and the three commands that must pass before a pull request
- [X] T006 [P] Write a `README.md` skeleton saying what edge-stt does and does not do, that it is edge-ear's sibling, and that unlike edge-ear it needs a C++ toolchain and CMake because whisper.cpp is C++
- [X] T007 [P] Add `.gitignore` covering `target/` and a local `models/` directory, so model weights cannot be committed by accident

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The types every story speaks in, and the crate skeletons that hold them. Nothing
here transcribes anything; the point is that after this phase the shape of the API exists and
the backends only have to fill it in.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T008 Create `core/Cargo.toml`: package `edge-stt-core`, features `whisper` (default) and `remote` (off), with whisper-rs behind the first and tokio plus tokio-tungstenite behind the second, both optional
- [X] T009 [P] Create `capi/Cargo.toml` (package `edge-stt-capi`, `crate-type = ["rlib", "cdylib", "staticlib"]`, `publish = false`) and `capi/cbindgen.toml`, both modelled on edge-ear's
- [X] T010 [P] Create `py/Cargo.toml` (package `edge-stt-py`, lib name `edge_stt`, pyo3 with `extension-module` and `abi3-py39`), `py/pyproject.toml` for maturin, and `py/build.rs`
- [X] T011 [P] Create `server/Cargo.toml`: package `edge-stt-server`, a binary, depending on `edge-stt-core` with the `whisper` feature plus axum, tokio, clap, and uuid
- [X] T012 [P] Write `core/src/error.rs`: the `Error` enum with one variant per cause named in [data-model.md](./data-model.md#error), each carrying its detail, plus `pub type Result<T>` — the same pairing edge-ear uses
- [X] T013 [P] Write `core/src/config.rs` part one: `SampleType`, `AudioFormat` with `mono_16k()`, and validation that rejects any other shape while naming both the expected and the received one
- [X] T014 Extend `core/src/config.rs`: `Accelerator`, `ModelSpec` (path, size hint, threads, accelerator), `RemoteConfig` (endpoint with no default, redacting credential, connect timeout), `BackendChoice`, `BackendKind`, and `Config` with `local()` and `remote()` constructors
- [X] T015 [P] Write `core/src/utterance.rs`: `Utterance`, `mono_16k()`, derived duration, audio under 0.3 s accepted as silence, audio over the configured maximum rejected while naming the limit and the actual length
- [X] T016 [P] Write `core/src/transcript.rs`: `Transcript`, `Segment`, `Partial`, and `PartialKind`, with the rule that `text` is exactly the concatenation of `segments`
- [X] T017 [P] Write `core/src/cancel.rs`: `CancelToken`, cloneable, with idempotent `cancel()` and `is_cancelled()`
- [X] T018 Write `core/src/backend/mod.rs`: the `Backend` trait as defined in [data-model.md](./data-model.md#backend), which is what makes the two paths substitutable
- [X] T019 Write `core/src/lib.rs`: module wiring, public re-exports, and the `EdgeStt` type with `new(Config)`, `backend_kind()`, `transcribe()`, and `transcribe_with()` — dispatching to a backend that does not exist yet
- [X] T020 [P] Write `core/tests/audio_format.rs`: `mono_16k()` is what edge-ear produces, and every other shape is rejected with both shapes named in the message
- [X] T021 [P] Write `core/tests/utterance_limits.rs`: a 0.1-second utterance is accepted, a 10-minute one is rejected against the stated limit, and neither is silently truncated
- [X] T022 [P] Write `core/tests/error_surface.rs`: every variant is distinct, a network failure never compares equal to a rejected credential, no variant carries transcribed text, and a credential prints as a placeholder in `Debug`
- [X] T023 Confirm `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, and `cargo test --workspace` all pass on the skeleton

**Checkpoint**: The API exists and compiles. Backends can now be filled in independently.

---

## Phase 3: User Story 1 — On-device transcription (Priority: P1) 🎯 MVP

**Goal**: Hand a recording to edge-stt on a machine with no network and get the words back.

**Independent Test**: Run the suite with networking disabled; known recordings produce their
known transcripts, silence produces an empty transcript, and nothing reaches for a socket.

### Tests for User Story 1

> Write these first and watch them fail. Every one needing a real model file is `#[ignore]`d and
> finds it through `EDGE_STT_MODEL_DIR`, mirroring edge-ear's `EDGE_EAR_WAKE_DIR`.

- [X] T024 [P] [US1] Write `core/tests/minimal_usage.rs`: the three-line example from [contracts/rust-core-api.md](./contracts/rust-core-api.md) compiles and transcribes a known recording
- [X] T025 [P] [US1] Write `core/tests/silence.rs`: silence, background noise, and a 0.2-second clip each return an empty transcript with a real audio duration — never an error, never invented words
- [X] T026 [P] [US1] Write `core/tests/wrong_shape.rs`: 44.1 kHz, stereo, and float input are each rejected naming both shapes; an over-long utterance is rejected up front
- [X] T027 [P] [US1] Write `core/tests/model_errors.rs`: a missing path fails at construction with the path it searched, a corrupt file fails as unusable, and neither fails later on the first utterance
- [X] T028 [P] [US1] Write `core/tests/model_parameters.rs`: load a model and pin its real parameters — multilingual or not, vocabulary size, expected sample rate — so a swapped model fails loudly, the habit edge-ear's CONTRIBUTING requires
- [X] T029 [P] [US1] Write `core/tests/cancellation.rs`: a token fired from another thread stops decoding within roughly one segment and yields the cancelled error; cancelling before the start yields it immediately
- [X] T030 [P] [US1] Write `core/tests/timeout.rs`: a limit shorter than the work produces the timeout error, distinct from cancellation, and never returns later than the limit
- [X] T031 [P] [US1] Write `core/tests/no_network.rs`: with default features, the process opens no socket while transcribing — asserted by watching the process, not by reading the source
- [X] T032 [P] [US1] Write `core/tests/readme.rs`: every code block in `README.md` still compiles and does what the prose around it claims, as edge-ear does

### Implementation for User Story 1

- [X] T033 [US1] Write `core/src/backend/whisper.rs`: load the model in the constructor, verify its real parameters, and refuse a monolingual model when a multilingual one is needed
- [X] T034 [US1] In `core/src/backend/whisper.rs`, build `FullParams` from `Config`: language forced with `set_language` or detected with `set_detect_language`, thread count from `ModelSpec`, then run the decode
- [X] T035 [US1] In `core/src/backend/whisper.rs`, assemble the `Transcript`: joined text, timed segments, settled language, confidence, audio duration, decoding time, and the backend that produced it
- [X] T036 [US1] In `core/src/backend/whisper.rs`, use `set_no_speech_thold` so silence comes back as an empty transcript rather than as noise or an error
- [X] T037 [US1] In `core/src/backend/whisper.rs`, drive `set_abort_callback_safe` from the `CancelToken` and from the deadline, returning cancellation and timeout as separate errors
- [X] T038 [US1] In `core/src/backend/whisper.rs`, map load and decode failures onto the error variants — missing, unusable, and out of resources — with the model size and the shortfall named
- [X] T039 [US1] In `core/src/lib.rs`, wire `EdgeStt::new` to build the local backend, and make an accelerator the build does not support an error at construction rather than a silent fall back to the CPU
- [X] T040 [P] [US1] Write `core/examples/probe_model.rs`, printing a model's real inputs and outputs — the same tool edge-ear tells contributors to run before trusting a model
- [X] T041 [P] [US1] Write `core/examples/transcribe.rs`: read a wav, print the text, the model, the audio duration, and the decoding time
- [X] T042 [US1] Add `--bench` to `core/examples/transcribe.rs`, reporting the three figures an integrator chooses a model size with: decoding time against audio duration, time to first partial, and peak memory
- [X] T043 [US1] Write `core/tests/overhead.rs`: transcription takes no more than 10% longer than the same model through whisper.cpp's own tooling on the same machine and audio — the one speed claim this project makes about itself
- [ ] T044 [US1] Run `--bench` on both reference machines for every supported model size in Korean and English, and record the raw figures in `docs/measurements.md`. The plan schedules this early on purpose: nothing downstream may quote a latency number that did not come out of it

**Checkpoint**: An edge device transcribes speech with no network, no account, and no server.
This is the MVP.

---

## Phase 4: User Story 2 — Remote transcription by configuration (Priority: P2)

**Goal**: The same calling code, pointed at a server by configuration alone.

**Independent Test**: Run the User Story 1 caller code against a stub server; the transcript
comes back the same shape. Then break the stub in each way that matters and confirm the caller
gets the right distinct failure instead of a hang.

### Tests for User Story 2

- [X] T045 [US2] Write `core/tests/support/stub_server.rs`: a test-only server speaking the protocol in [contracts/websocket-protocol.md](./contracts/websocket-protocol.md), with switches for every fault the tests need. This is not the real server, which is User Story 4 — it exists so this story can be finished without it
- [X] T046 [P] [US2] Write `core/tests/remote_basic.rs`: a recording sent to the stub comes back as a transcript reporting the remote backend
- [X] T047 [P] [US2] Write `core/tests/backend_parity.rs`: one body of caller code, run against both backends, yields the same transcript shape and the same text. This test is what keeps the two from drifting apart later
- [X] T048 [P] [US2] Write `core/tests/remote_faults.rs`: host down gives a network error, a wrong credential gives a rejected-credential error, a connection dropped mid-request gives a network error with nothing presented as final, and a stalled server gives a timeout at the configured limit and not later
- [X] T049 [P] [US2] Write `core/tests/remote_config.rs`: building a remote transcriber with no endpoint fails at construction, and no default endpoint string exists anywhere in the crate
- [X] T050 [P] [US2] Write `core/tests/fallback.rs`: with fallback enabled a failing remote is retried locally and the transcript says so; with fallback off — the default — the remote failure surfaces untouched

### Implementation for User Story 2

- [X] T051 [US2] Write `core/src/wire.rs`: the serde types for every message in [contracts/websocket-protocol.md](./contracts/websocket-protocol.md), behind the `remote` feature
- [X] T052 [US2] Write `core/src/backend/remote.rs`: connect, present the bearer credential on the handshake, then send the start frame, the audio as one binary frame, and the end frame
- [X] T053 [US2] In `core/src/backend/remote.rs`, read the replies and turn them into a `Transcript` or the matching error, using the code table in the protocol contract
- [X] T054 [US2] In `core/src/backend/remote.rs`, implement cancellation as a cancel message plus a dropped socket, and keep the connect timeout distinct from the transcription timeout so an unreachable host reads differently from a slow one
- [X] T055 [US2] Write `core/src/fallback.rs`: hold both backends, try the remote, fall back only when the caller asked, and record which one produced the result
- [X] T056 [US2] In `core/src/config.rs`, make `Config::remote` reject a missing endpoint at construction and confirm the credential's `Debug` shows a placeholder
- [X] T057 [US2] In `core/Cargo.toml` and `core/src/lib.rs`, confirm the feature seam: with default features the crate has no async runtime and no socket code, and `cargo tree` shows neither tokio nor tungstenite

**Checkpoint**: One setting switches where transcription happens. Nothing else in the caller
changes.

---

## Phase 5: User Story 3 — Partial transcripts (Priority: P3)

**Goal**: Words appear as they are decoded, the same way on both backends.

**Independent Test**: Transcribe a long recording on each backend; the first partial arrives well
before the final result, every partial is marked non-final, and concatenating them equals the
final text.

### Tests for User Story 3

- [X] T058 [P] [US3] Write `core/tests/partials_local.rs`: partials arrive during decoding, in sequence order with no gaps, and concatenating them equals the final text exactly
- [X] T059 [P] [US3] Write `core/tests/partials_remote.rs`: the same holds through the stub server, with sequence numbers preserved end to end
- [X] T060 [P] [US3] Write `core/tests/partials_parity.rs`: both backends deliver partials through the same caller-facing mechanism, so a caller using them can still switch by configuration alone
- [X] T061 [P] [US3] Write `core/tests/partials_terminal.rs`: no partial arrives after the final result, a failure or a cancellation after partials tells the caller the transcript will not be completed, and a caller that asked for nothing receives no partial and pays no cost
- [X] T062 [P] [US3] Write `core/tests/partials_latency.rs`: a partial reaches the caller within 50 ms of the decoder finishing that segment, and is never held back to be batched with the next

### Implementation for User Story 3

- [X] T063 [US3] In `core/src/backend/whisper.rs`, drive partials from `set_segment_callback_safe`, emitting append-only partials with a sequence number and the segment's timing
- [X] T064 [US3] In `core/src/lib.rs`, plumb `transcribe_with`'s callback through, invoked on the calling thread and never after the call returns, while `transcribe` registers no callback with the decoder at all
- [X] T065 [US3] In `core/src/wire.rs` and `core/src/backend/remote.rs`, carry partials over the socket one message each, preserving sequence numbers so a client can detect a gap, and never batching them
- [X] T066 [US3] In `core/src/backend/whisper.rs` and `core/src/backend/remote.rs`, enforce the terminal rules: nothing after the final result, and a clear not-completed signal when a failure or cancellation follows partials already delivered
- [X] T067 [P] [US3] Add `--partials` to `core/examples/transcribe.rs`, printing text as it decodes

**Checkpoint**: Every story so far works with and without progressive results.

---

## Phase 6: User Story 4 — The transcription server (Priority: P4)

**Goal**: An operator starts one server on a capable machine and points several devices at it.

**Independent Test**: Start the server, submit from several clients at once, and confirm each
gets its own correct transcript with partials while a disconnecting client disturbs nobody.

### Tests for User Story 4

- [X] T068 [P] [US4] Write `server/tests/transcribe_roundtrip.rs`: a client gets the same transcript shape the on-device backend produces, partials included
- [X] T069 [P] [US4] Write `server/tests/concurrent_clients.rs`: eight clients at once each receive their own correct transcript, with no transcript reaching the wrong client
- [X] T070 [P] [US4] Write `server/tests/rejects_bad_credential.rs`: a missing or wrong credential is refused at the handshake, nothing is transcribed, and the refusal is distinguishable from a server error
- [X] T071 [P] [US4] Write `server/tests/at_capacity.rs`: past capacity a client is told its queue position or refused explicitly, and is never left without an answer
- [X] T072 [P] [US4] Write `server/tests/client_vanishes.rs`: a client disappearing mid-utterance stops that decoding and frees what it held, without affecting the others
- [X] T073 [P] [US4] Write `server/tests/health.rs`: alive answers immediately, ready answers only once the model has loaded
- [X] T074 [P] [US4] Write `server/tests/no_retention.rs`: after a request completes no audio or transcript remains on disk or in memory, and no transcribed text appears in the logs unless it was turned on

### Implementation for User Story 4

- [X] T075 [US4] Write `server/src/main.rs`: clap arguments for model path, bind address, credential file, capacity, and the explicit flag that opens transcription to unauthenticated callers — refusing to start without one or the other
- [X] T076 [P] [US4] Write `server/src/auth.rs`: check the bearer credential on the handshake, before any audio is read
- [X] T077 [P] [US4] Write `server/src/health.rs`: the alive route, and the ready route that stays unready until the model has finished loading, so a supervisor does not kill a server that is working
- [X] T078 [US4] Write `server/src/session.rs`: one client's request — its identifier, its state machine as drawn in [data-model.md](./data-model.md#transcriptionsession-server-only), its cancellation token, and its partial counter
- [X] T079 [US4] Write `server/src/capacity.rs`: a bounded queue that reports position on acceptance and refuses explicitly when full
- [X] T080 [US4] Write `server/src/ws.rs`: the WebSocket route handling start, audio, end, and cancel, and emitting accepted, partial, final, error, and cancelled exactly as the protocol contract specifies
- [X] T081 [US4] In `server/src/ws.rs`, bridge to the blocking core with `spawn_blocking`, sharing one loaded `EdgeStt` across sessions
- [X] T082 [US4] In `server/src/ws.rs`, map every core error onto its protocol code, keeping a dead host, a bad credential, and a full server distinguishable, and keeping transcribed text out of every message
- [X] T083 [US4] In `server/src/ws.rs` and `server/src/session.rs`, drop everything a request held when it ends, and validate audio shape with the same core code rather than a second implementation

**Checkpoint**: All four stories work. The remote backend now has a real server to talk to.

---

## Phase 7: Language Bindings

**Purpose**: The C and Python surfaces from [contracts/bindings.md](./contracts/bindings.md).

They come after the stories rather than inside them because they add no behaviour of their own —
building them per story would mean rewriting the same two files four times. Pull this phase
earlier if a C or Python integrator is waiting on the MVP.

- [X] T084 [P] Write `capi/src/error.rs`: zero for success and a distinct **negative** code per error variant, with a thread-local `edge_stt_last_error()` — edge-ear's convention exactly, not a new one
- [X] T085 [P] Write `capi/src/convert.rs`: transcripts and segments across the boundary, with strings owned by the object they came from
- [X] T086 Write `capi/src/lib.rs`: `edge_stt_new`, `edge_stt_free`, `edge_stt_load_model`, `edge_stt_transcribe`, `edge_stt_cancel`, and the transcript accessors — no name repeating the prefix — each catching unwinding so no panic crosses into C, and each carrying `@brief`, `@param`, `@return`, and `@see` in its doc comment so cbindgen writes a documented header
- [X] T087 Write `capi/src/partials.rs`: `edge_stt_on_partial` with user data, called on the transcribing thread and never after the call returns, mirroring `edge_ear_on_event`
- [X] T088 Write `capi/cbindgen.toml` in edge-ear's form — `documentation_style = "doxy"`, an include guard, and a header block stating the error and memory rules — generate `capi/include/edge_stt.h` from it, and write `capi/tests/header_is_current.rs`, which regenerates and fails when the committed header has drifted
- [X] T089 [P] Write `capi/tests/surface.c` and `capi/tests/c_surface.rs`: the C surface compiles, links, transcribes, and frees without leaking
- [X] T090 [P] Write `capi/examples/transcribe.c`, the C twin of the Rust example
- [X] T091 Write `py/src/lib.rs`: `EdgeStt`, `Transcript`, one exception class per error variant under a common base, the GIL released around decoding, and samples accepted from any buffer without a copy where the layout matches
- [X] T092 In `py/src/lib.rs`, add `transcribe_stream` as a generator yielding partials, with the final transcript available once it is exhausted
- [X] T093 [P] Write `py/tests/test_transcribe.py`: the documented Python examples run, exceptions are catchable by class, and a numpy `int16` array is accepted directly

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T094 [P] Finish `README.md`: what it does, what it does not do, the model table, the C++ toolchain requirement, and the Rust, C, and Python examples the `readme.rs` test checks
- [X] T095 [P] Publish `docs/measurements.md`: the full table from both reference machines, every model size, Korean and English — the measurements an integrator picks a size from, stated as figures rather than as a promise
- [X] T096 [P] Fill in `THIRD-PARTY-LICENSES` for every model referenced in the documentation, with licence, checksum, and where it came from
- [X] T097 Write `core/tests/soak.rs`: a thousand consecutive utterances each produce a transcript or a typed failure, none are lost, and memory at the end is within 5% of memory after the first hundred
- [X] T098 Write `.github/workflows/ci.yml`: clippy with warnings denied, a formatting check, and the full suite, on Linux and macOS
- [X] T099 Add a job to `.github/workflows/ci.yml` running the offline suite inside a network namespace with no interfaces, so the no-network claim is verified by the machine on every change rather than by a reviewer
- [X] T100 [P] Write `capi/Doxyfile` in edge-ear's form — reading only `capi/include`, output to `capi/docs`, C-optimised, undocumented symbols warned about rather than extracted — and check the generated documentation renders
- [X] T101 Walk `specs/001-pcm-transcription-service/quickstart.md` top to bottom on a clean machine and fix whatever has drifted
- [X] T102 Sweep the tree for planning-document numbering: no `FR-`, `SC-`, `US`, or task identifier may appear in any source file, comment, or commit message. edge-ear's CONTRIBUTING asks for the sentence instead, and the traceability lives in these documents

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies
- **Foundational (Phase 2)**: needs Setup — blocks every user story
- **User Story 1 (Phase 3)**: needs Foundational. Depends on no other story
- **User Story 2 (Phase 4)**: needs Foundational. Testable on its own against the stub server, so it does **not** wait for User Story 4
- **User Story 3 (Phase 5)**: needs Foundational, and needs the backend it is adding partials to — the local half needs Phase 3, the remote half needs Phase 4
- **User Story 4 (Phase 6)**: needs Foundational and the local backend from Phase 3, since the server runs it. Independent of Phases 4 and 5, except that relaying partials needs Phase 5
- **Bindings (Phase 7)**: needs the core surface to have stopped moving — after Phase 5
- **Polish (Phase 8)**: after the stories that are wanted

### The one dependency worth watching

User Story 3 is the only story that reaches into two others. Partials on the local backend touch
`core/src/backend/whisper.rs`, which Phase 3 writes; partials over the wire touch
`core/src/backend/remote.rs` and `core/src/wire.rs`, which Phase 4 writes. Scheduling Phase 5
before either is finished means editing files that are still being written.

### Within each story

- Tests are written first and must fail before the implementation lands
- Types before backends, backends before the surface that exposes them
- The story is finished, and its checkpoint verified, before the next priority starts

### Parallel Opportunities

- Phase 1: T002 through T007 all together — six different files
- Phase 2: T009, T010, T011 together, then T012, T013, T015, T016, T017 together, then the three test files T020, T021, T022 together. T014 waits for T013 because it is the same file
- Phase 3: all nine test files T024–T032 together. Then T040 and T041 together
- Phase 4: T046 through T050 together, once T045's stub exists
- Phase 5: all five test files T058–T062 together
- Phase 6: all seven test files T068–T074 together; then T076 and T077 together
- Phase 7: T084 and T085 together, then T089, T090 together; T093 alongside them
- Across stories: once Phase 3 lands, User Story 2 and User Story 4 can proceed at the same time in different files

---

## Parallel Example: User Story 1

```bash
# All nine test files at once — different files, no shared state:
Task: "Write core/tests/minimal_usage.rs"
Task: "Write core/tests/silence.rs"
Task: "Write core/tests/wrong_shape.rs"
Task: "Write core/tests/model_errors.rs"
Task: "Write core/tests/model_parameters.rs"
Task: "Write core/tests/cancellation.rs"
Task: "Write core/tests/timeout.rs"
Task: "Write core/tests/no_network.rs"
Task: "Write core/tests/readme.rs"

# Then the two examples, once the backend exists:
Task: "Write core/examples/probe_model.rs"
Task: "Write core/examples/transcribe.rs"
```

The implementation tasks T033 through T039 are **not** parallel — they are seven edits to
`core/src/backend/whisper.rs`, in order.

---

## Implementation Strategy

### MVP first — User Story 1 only

1. Phase 1: Setup
2. Phase 2: Foundational
3. Phase 3: User Story 1
4. **Stop and validate**: transcribe on a machine with networking disabled, then run T044's
   benchmark so the numbers in `docs/measurements.md` are real before anyone quotes them
5. At this point edge-stt is already useful — an edge device turning speech into text with no
   server and no account

### Incremental delivery

1. Setup and Foundational → the API exists
2. User Story 1 → on-device transcription. **MVP**
3. User Story 2 → the same code reaches a server, tested against a stub
4. User Story 3 → words appear as they are decoded, on both backends
5. User Story 4 → a real server to point at, replacing the stub in the integration story
6. Bindings → C and Python integrators
7. Polish → the soak test, the measurement table, and the offline check running in CI

### Parallel team strategy

After Phase 2, three people can work at once:

- **A**: User Story 1, the local backend — everyone else waits on this one for their backend
- **B**: User Story 2, the remote client and the stub server — needs nothing from A but the
  shared types
- **C**: Phase 7's C API scaffolding against the types from Phase 2

Once A finishes, C picks up User Story 4 — the server runs A's backend — and User Story 3 is
split between whoever owns the two backend files.

---

## Notes

- `[P]` means different files with no unfinished dependency
- Tests needing a real model file are `#[ignore]`d and find it through `EDGE_STT_MODEL_DIR`,
  mirroring edge-ear's `EDGE_EAR_WAKE_DIR`. The default suite runs without any model
- Before each pull request: `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo fmt --all --check`, `cargo test --workspace`
- Comments are few and at most **two lines**, and say why rather than what
- Commit messages are English, wrapped, and at most **ten lines**. Documentation and test
  changes go in their own commits, apart from the implementation they belong to
- No identifier from this document, or from spec.md or plan.md, belongs in the code. Write the
  sentence instead. T102 checks
