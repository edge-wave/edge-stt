# Feature Specification: PCM Transcription Service

**Feature Branch**: `001-pcm-transcription-service`

**Created**: 2026-08-28

**Status**: Draft

**Input**: User description: "edge-ear takes the front line of the edge client: it runs on-device models and produces wake-up events. What is missing is an STT service that turns the recorded PCM into real text. That service may run on-device, and depending on a client option the caller may instead fetch the result from a remote server. Using a well-known open speech model is the first goal."

**Clarified 2026-08-28**: progressive (partial) transcripts are in scope for the first release, on both backends. The remote transcription server is delivered by this project, not assumed to exist.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Turn a captured utterance into text, on the device (Priority: P1)

A program has just received a finished recording from its audio front end: someone said a wake word, spoke, and stopped. The program hands that raw audio to edge-stt and gets back the words that were said. Nothing leaves the machine.

**Why this priority**: This is the whole reason the component exists, and it is the only story that stands alone — with just this, an edge device can go from sound to text with no server, no account, and no network. Everything else is a variation on it.

**Independent Test**: Feed a known recording of spoken audio to the on-device path on a machine with networking disabled, and check the returned text against the expected transcript.

**Acceptance Scenarios**:

1. **Given** a configured on-device transcriber and a 5-second recording of clear speech, **When** the caller asks for a transcript, **Then** the spoken words are returned as text within the latency budget, and no network connection is attempted.
2. **Given** a recording that contains only silence or background noise, **When** the caller asks for a transcript, **Then** an empty transcript is returned as a normal result, not as an error.
3. **Given** the caller has not supplied the required model files, **When** the transcriber is created, **Then** it fails immediately with an error naming what is missing, rather than failing later on the first utterance.

---

### User Story 2 - Get the same text from a remote server instead (Priority: P2)

The device is too small to run a large model well, or the operator wants the accuracy of a bigger model running on a host machine. The operator changes one setting; the program's code does not change. Audio now goes to a server the operator chose, and the transcript comes back the same shape as before.

**Why this priority**: It is what makes the component usable across a range of hardware, but it is worthless without Story 1's contract to conform to. It also introduces the failure modes — unreachable host, credentials, timeouts — that the on-device path does not have.

**Independent Test**: Point the remote setting at a stub server, run the same caller code used to test Story 1, and confirm the transcript comes back and matches — then kill the stub mid-request and confirm the caller gets a clear, typed failure rather than a hang.

**Acceptance Scenarios**:

1. **Given** a transcriber configured for a remote endpoint, **When** the caller submits a recording, **Then** the transcript is returned through the same result shape as the on-device path.
2. **Given** the remote endpoint is unreachable, **When** the caller submits a recording, **Then** the caller receives a network failure distinguishable from a rejected-audio failure, within the configured timeout.
3. **Given** no endpoint has been configured, **When** the caller selects the remote backend, **Then** creation fails with a clear error — the component never falls back to a built-in or default server address.
4. **Given** the operator has enabled fallback, **When** the remote endpoint fails, **Then** the request is retried on the on-device backend and the result records which backend produced it.

---

### User Story 3 - Watch the words appear while they are still being decoded (Priority: P3)

Rather than waiting for the whole utterance to finish decoding, the caller receives the transcript in pieces as they become available, so a display or a downstream reply can start moving. This works the same way whether the words are being decoded on the device or on a server.

**Why this priority**: It changes perceived responsiveness a great deal and nothing else — the final transcript is identical either way — so it can be built and shipped after Stories 1 and 2 without reworking them. It is in the first release, but it is the slice that can slip last.

**Independent Test**: Submit a 20-second recording to each backend in turn, record the arrival time of each partial transcript, and confirm the first one arrives well before the final one and that the partials converge on the final transcript.

**Acceptance Scenarios**:

1. **Given** a caller that asked for progressive results, **When** a long recording is transcribed on the device, **Then** partial transcripts arrive as segments are decoded and the last one equals the final result.
2. **Given** the same caller pointed at the remote backend, **When** the same recording is transcribed, **Then** partial transcripts arrive the same way, through the same caller-facing mechanism.
3. **Given** a caller that did not ask for progressive results, **When** a recording is transcribed, **Then** behaviour is exactly as in Story 1 — one final transcript, no extra cost.
4. **Given** partial transcripts have already been delivered, **When** the transcription then fails or is cancelled, **Then** the caller is told the transcript is not final and no partial is presented as a completed result.

---

### User Story 4 - Run the transcription server (Priority: P4)

An operator has one capable machine on the network and several small devices. They start the edge-stt server on that machine, point the devices at it, and get transcripts from a larger model than any of the devices could run.

**Why this priority**: It is the other half of Story 2, and Story 2 can be developed and tested against a stub without it — but the pair is what makes remote transcription real for an operator rather than a promise. It also carries the operational surface (concurrency, credentials, capacity) that nothing else does.

**Independent Test**: Start the server on a host machine, submit recordings from several clients at once, and confirm each gets its own correct transcript with partials, and that a client disconnecting does not disturb the others.

**Acceptance Scenarios**:

1. **Given** a running server with model files available, **When** a client submits a recording, **Then** the server returns the same transcript shape the on-device backend produces, including partials.
2. **Given** several clients submitting at once, **When** the server is at capacity, **Then** it tells waiting clients they are queued rather than dropping them or failing silently.
3. **Given** a client that presents no credential or a wrong one, **When** it submits a recording, **Then** the server rejects it with a reason distinguishable from a server-side error, and transcribes nothing.
4. **Given** a client that disappears mid-utterance, **When** the server notices, **Then** it abandons that transcription and frees its resources without affecting other clients.

---

### Edge Cases

- **Silence, noise, or no speech at all**: returns an empty transcript, never an error, and never invented words.
- **Very short audio** (under ~0.3 s): handled the same as silence rather than rejected.
- **Very long audio** (minutes): either transcribed in full or rejected up front against a stated maximum duration — never truncated silently.
- **Wrong audio shape** (unexpected sample rate, channel count, or sample width): rejected at submission with an error stating what was expected and what arrived.
- **Model files missing, corrupt, or the wrong model**: detected when the transcriber or the server starts, not on the first utterance.
- **Not enough memory or compute for the chosen model size**: fails with an error naming the model and the shortfall, rather than being killed by the OS mid-utterance.
- **Remote endpoint reachable but rejecting**: bad credentials, rate limiting, at-capacity, and server errors are each distinguishable by the caller.
- **Network drops mid-request**: the caller learns within the timeout; partials already delivered are marked non-final and no half-transcript is presented as complete.
- **Partial transcripts that shrink or reorder** as the model revises its decoding: the caller is given a coherent replace-or-append rule, not contradictory fragments.
- **Caller cancels** (user walked away, wake word was a false positive): an in-flight transcription stops, on the server too if that is where it is running, and stops consuming resources.
- **Utterances arriving faster than they can be transcribed**: back-pressure is visible to the caller; utterances are not silently dropped.
- **Server at capacity or restarted mid-utterance**: clients are told which, and can decide to wait, fall back on-device, or give up.
- **Two languages inside one utterance**: produces a best-effort single transcript with the dominant language reported.

## Requirements *(mandatory)*

### Functional Requirements

**Core transcription**

- **FR-001**: The service MUST accept a complete recorded utterance as raw PCM audio and return the words spoken in it as text.
- **FR-002**: The service MUST accept, without conversion by the caller, the audio shape that the project's audio front end produces by default — 16 kHz, single channel, 16-bit signed samples.
- **FR-003**: The service MUST validate the shape of submitted audio and reject anything it cannot handle with an error that names both the expected and the received shape.
- **FR-004**: A transcript result MUST carry, alongside the text, the language the model settled on, a confidence indication, the audio duration it covers, and which backend produced it.
- **FR-005**: The service MUST let the caller state the expected language, and MUST detect the language itself when the caller does not.
- **FR-006**: The service MUST return an empty transcript, not an error, when the audio contains no recognisable speech.

**Choosing where transcription happens**

- **FR-007**: The caller MUST be able to choose between on-device and remote transcription through configuration alone, with no change to the code that submits audio or reads results.
- **FR-008**: The on-device backend MUST work with no network access whatsoever, and MUST make no network connection of any kind.
- **FR-009**: The remote backend MUST send audio only to an endpoint the caller supplied, using credentials the caller supplied. The service MUST NOT contain a default or fallback endpoint address.
- **FR-010**: The caller MUST be able to enable falling back to the on-device backend when the remote one fails, and this MUST be off unless asked for.
- **FR-011**: The service MUST report which backend produced each transcript, so a caller can tell a local result from a remote one.

**Progressive results**

- **FR-012**: The service MUST deliver partial transcripts as they are decoded to callers that ask for them, and the final partial MUST match the final transcript.
- **FR-013**: Progressive delivery MUST be opt-in. A caller that does not ask for it MUST see the single-result behaviour of FR-001 with no added cost.
- **FR-014**: Progressive delivery MUST work identically on both backends, through the same caller-facing mechanism, so that FR-007's promise holds for callers that use it.
- **FR-015**: Each partial MUST state whether it replaces or extends what came before, and MUST be marked non-final, so a caller never mistakes one for a completed transcript.
- **FR-016**: When a transcription fails or is cancelled after partials have been delivered, the caller MUST be told the transcript will not be completed.

**The transcription server**

- **FR-017**: The project MUST deliver a server that accepts utterances from remote callers and returns transcripts, so that an operator does not have to supply their own.
- **FR-018**: The server MUST produce the same transcript shape as the on-device backend, including partials, so that the two are interchangeable from the caller's point of view.
- **FR-019**: The server MUST serve several clients at once, keeping each client's utterances and transcripts separate.
- **FR-020**: The server MUST require a credential the operator configured, and MUST refuse to start with transcription open to unauthenticated callers unless the operator explicitly chose that.
- **FR-021**: The server MUST make its capacity limit explicit: when it cannot take more work, waiting clients MUST be told they are queued or refused, never left without an answer.
- **FR-022**: The server MUST stop work for a client that has disconnected or cancelled, and free the resources that work held.
- **FR-023**: The server MUST expose whether it is alive and ready to transcribe — including whether its model finished loading — so an operator can supervise it.
- **FR-024**: The server MUST NOT retain audio or transcripts after a request completes, unless the operator explicitly configured it to.

**Models**

- **FR-025**: Both the on-device backend and the server MUST run a publicly available open-weight speech recognition model, not a proprietary or account-gated service.
- **FR-026**: The project MUST NOT ship model weights. The operator supplies them, and the service MUST state plainly which model files it needs and where it looked for them.
- **FR-027**: The caller or operator MUST be able to select the model size, the number of threads, and — where the build provides one — the accelerator, at construction. These are the knobs that decide the accuracy-against-speed trade on a given board, and the choice belongs to whoever deploys it, not to this project.
- **FR-028**: The service MUST NOT refuse a combination of model size and hardware on the grounds that it will be slow. It reports what it cost (FR-036) and lets the integrator decide whether that is acceptable on their board.

**Failure, timing, and lifecycle**

- **FR-029**: Every failure MUST be distinguishable by cause: unusable audio, missing or unusable model, insufficient resources, network failure, rejected credentials, server at capacity, remote server error, timeout, and cancellation.
- **FR-030**: The caller MUST be able to set a time limit for a transcription, and MUST be told when that limit is reached instead of waiting indefinitely.
- **FR-031**: The caller MUST be able to cancel an in-flight transcription and have the resources it holds released, including on the server when that is where it is running.
- **FR-032**: Model loading MUST be a separate, observable step from transcription, so the cost is paid at startup rather than on the first spoken word.
- **FR-033**: The service MUST handle utterances submitted back-to-back without losing any of them, and MUST make queue pressure visible to the caller rather than dropping work silently.

**Privacy and observability**

- **FR-034**: The service MUST NOT write audio or transcripts to disk, and MUST NOT send them anywhere, except where the caller or operator explicitly configured it to.
- **FR-035**: Diagnostic output MUST NOT include transcribed text or audio content unless that is turned on explicitly.
- **FR-036**: The service MUST report, per transcription, how long it took and how that compares to the audio's duration, so an operator can tell whether the hardware is keeping up.

### Key Entities

- **Utterance**: One complete recording handed over for transcription — the audio samples, their shape, and when they were captured. Produced upstream by the audio front end; edge-stt does not capture audio itself.
- **Transcript**: What was said, as text, plus the language, a confidence indication, the covered duration, and the backend that produced it. Optionally broken into timed segments.
- **Partial Transcript**: An interim, explicitly non-final view of a Transcript, delivered while decoding is still in progress, carrying whether it replaces or extends the previous one.
- **Backend**: A way of turning an Utterance into a Transcript — on-device or remote. Interchangeable from the caller's point of view.
- **Model Bundle**: The operator-supplied files a backend needs, identified by model family and size.
- **Transcription Session**: One client's in-flight request on the server — its utterance, its stream of partials, its credential, and its cancellation state.
- **Transcription Failure**: A typed reason a transcript could not be produced, carrying enough detail for the caller to decide whether to retry, fall back, or give up.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the reference edge device with networking physically disabled, a 5-second utterance of clear speech is turned into text, correctly, with no manual intervention.
- **SC-002**: The service adds almost nothing to the model's own cost: on the same machine, with the same model and the same audio, transcription takes no more than 10% longer than running that model through its reference tooling. Absolute speed is a property of the model size and the hardware, both of which the integrator chooses — this criterion is about what edge-stt itself contributes.
- **SC-003**: Word error rate on a held-out set of clean Korean and English utterances is within 2 percentage points of the same open model run through its own reference tooling — that is, the service adds no accuracy loss of its own.
- **SC-004**: Switching a running program between on-device and remote transcription requires changing configuration only; the same test suite, including its progressive-result tests, passes unmodified against both backends.
- **SC-005**: A partial reaches the caller as soon as the decoder produces it — within 50 milliseconds of the decoder finishing that segment, on both backends, and never held back to be batched with the next one. How long the decoder takes to produce its first segment is the model's and the board's business, measured under SC-012, not something this service promises.
- **SC-006**: Across the progressive-result test set, the last partial equals the final transcript in 100% of trials, and no partial is ever delivered after the final result.
- **SC-007**: In fault-injection testing — endpoint down, credentials rejected, connection dropped mid-utterance, server at capacity, response delayed past the limit — the caller receives the correct, distinct failure in 100% of trials, and never hangs past the configured time limit.
- **SC-008**: The server sustains 8 concurrent clients on the reference host with no utterance lost, no transcript delivered to the wrong client, and per-utterance latency no worse than twice the single-client figure.
- **SC-009**: Over an 8-hour run of 1,000 consecutive utterances, every utterance produces either a transcript or a typed failure, none are lost, and memory use at the end is within 5% of memory use after the first hundred — measured on the device and on the server.
- **SC-010**: An integrator who has the model files can go from nothing to a first transcript, and separately from nothing to a running server serving one, by following the documentation alone, each in under 15 minutes, without reading the source.
- **SC-011**: The on-device path is observed making zero network connections across the full test suite, verified by monitoring at the operating-system level rather than by inspection of the code.
- **SC-012**: For every model size the project supports, measured figures are published for each reference machine — decoding time against audio duration, time to first partial, and peak memory — for both Korean and English. An integrator picks a size from a table of measurements, not from a promise.
- **SC-013**: An integrator can reproduce those measurements on their own board with one shipped command, in under 10 minutes, and get the same three figures for their hardware.

## Assumptions

- **Audio arrives from the project's own front end.** Utterances come from edge-ear's end-of-speech output, so 16 kHz mono 16-bit is the shape that must work. Resampling or channel mixing on behalf of the caller is out of scope for the first release; audio in another shape is rejected rather than converted.
- **The open model family is the Whisper family.** It is the best-known open speech model, it has mature on-device runtimes, and the existing echo-vinci prototype already runs it server-side. The spec does not bind the implementation to a particular runtime, only to an open-weight model.
- **Korean and English are the languages that must work.** Others are welcome to the extent the chosen model handles them, but they are not a release gate.
- **The operator supplies model files**, exactly as edge-ear requires for wake-word models, and for the same licensing reason.
- **The client half is a library embedded in the caller's program**, following edge-ear's shape — a core with bindings for other languages — rather than a daemon the operator runs separately.
- **The server is a separate deliverable in this same project**, built on that library so that both backends run the same model through the same code and cannot drift apart.
- **The client-to-server transport carries partial results**, since FR-014 requires progressive delivery over the network; the specific protocol is chosen during planning, informed by the WebSocket flow the echo-vinci prototype already proved out.
- **The server runs on a trusted network** — a home LAN or a private overlay network, as echo-vinci does over Tailscale. Exposure to the public internet is not a first-release goal, which is why FR-020 asks for a shared credential rather than per-user accounts.
- **Transcription only.** Wake-word detection, voice activity detection, audio capture, and playback belong to edge-ear; understanding the text and replying belong to whatever the caller builds on top.
- **Speaker identification, diarisation, and word-level timestamps are out of scope** for the first release.
- **Punctuation and capitalisation are best-effort**, taken as the model produces them, and are not a correctness criterion.
- **A "reference edge device" and a "reference host" exist for benchmarking** — a single-board computer of the class edge-ear targets, and a desktop-class machine of the class echo-vinci's host server runs on. The exact hardware is fixed during planning, not here.
