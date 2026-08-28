# Contract: client-to-server protocol

**Endpoint**: `GET /api/v1/transcribe` (WebSocket upgrade)
**Also**: `GET /healthz`, `GET /readyz`

A port of the shape echo-vinci proved over Tailscale, with the audio moved from a hex string
into a binary frame. Every message that concerns a request carries its `request_id`, so a client
can have more than one in flight without correlating by arrival order.

## Handshake

```http
GET /api/v1/transcribe HTTP/1.1
Upgrade: websocket
Authorization: Bearer <credential>
```

- The credential is the one the operator configured. A missing or wrong one is refused at the
  handshake with `401`, before any audio is read, and the server transcribes nothing.
- A server started without a credential refuses to start, unless the operator explicitly passed
  the flag that opens it to unauthenticated callers.

## A request

**1. Client → server** (text frame, JSON):

```json
{
  "type": "start",
  "request_id": "8f14e45f-ceea-467a-9b17-1f0a0f9f24c1",
  "format": { "sample_rate": 16000, "channels": 1, "sample_type": "i16" },
  "language": "ko",
  "want_partials": true
}
```

`language` may be omitted, which asks the server to detect it.

**2. Client → server** (binary frame): the raw samples, little-endian, exactly as described by
`format`. One frame per utterance in this release; the framing leaves room for chunked upload
later.

**3. Client → server** (text frame): `{"type": "end", "request_id": "..."}`

**4. Server → client**, in order:

```json
{"type": "accepted",  "request_id": "...", "queue_position": 0}
{"type": "partial",   "request_id": "...", "seq": 0, "kind": "append",
                      "text": "안녕하세요", "start_ms": 0, "end_ms": 1200}
{"type": "partial",   "request_id": "...", "seq": 1, "kind": "append",
                      "text": " 오늘 날씨 어때요", "start_ms": 1200, "end_ms": 2900}
{"type": "final",     "request_id": "...",
                      "text": "안녕하세요 오늘 날씨 어때요",
                      "language": "ko", "confidence": 0.97,
                      "audio_duration_ms": 2900, "processing_time_ms": 810,
                      "segments": [ ... ]}
```

**Guarantees**

- `accepted` always precedes any other message for a request, and carries the queue position —
  `0` meaning work started immediately. This is how a client learns it is waiting rather than
  being ignored.
- `partial` is sent only when the client asked for it, with `seq` from 0 increasing by one so a
  gap is detectable.
- Concatenating every `append` partial in `seq` order equals `final.text`.
- Exactly one terminal message per request: `final`, `error`, or `cancelled`. Nothing about that
  request follows it.
- `final.processing_time_ms` is the server's own decoding time, not including transfer, so a
  client can tell a slow network from a slow server.

## Cancelling

Client → server: `{"type": "cancel", "request_id": "..."}`

The server stops decoding, replies `{"type": "cancelled", "request_id": "..."}`, and frees what
the request held. Closing the socket has the same effect on every request in flight on it — a
client that vanishes must not leave the server decoding for nobody.

## Errors

```json
{"type": "error", "request_id": "...", "code": "at_capacity",
 "message": "8 requests already decoding", "retry_after_ms": 2000}
```

| `code` | Maps to | Client should |
|---|---|---|
| `unsupported_audio` | `UnsupportedAudio` | Fix the caller. Never retry |
| `audio_too_long` | `AudioTooLong` | Never retry |
| `at_capacity` | `ServerAtCapacity` | Retry after `retry_after_ms`, or fall back locally |
| `model_unavailable` | `ServerError` | Retry later; the operator has a problem |
| `internal` | `ServerError` | Retry once |

`401` at the handshake maps to `CredentialRejected`. A dropped connection maps to `Network`.
The two are never conflated, because a dead host and a bad token need different responses.

**Rule**: no error message contains transcribed text.

## Health

| Route | Answers | Meaning |
|---|---|---|
| `GET /healthz` | `200` once the process is up | The process is alive |
| `GET /readyz` | `200` only once the model has finished loading | It can actually transcribe now |

The two are separate because loading `large-v3` takes long enough that a supervisor would
otherwise kill a server that was working correctly.

## What the server does not do

- It does not keep audio or transcripts after a request ends, unless the operator turned that on.
- It does not log transcribed text unless the operator turned that on.
- It does not accept audio in a shape the library would reject; validation is the same code.
