# Measurements

Which model size to run is your decision. These are the numbers to
decide with. Reproduce them on your own board:

```bash
cargo run --release --example transcribe -- --bench MODEL sample.wav
```

## What has actually been measured

One set, on one machine, on the GPU. The processor path on that machine
is still void, for the reason below.

### Apple M4 Pro, Metal, Korean

9.20 s of Korean speech, the language named rather than detected, taking
the second run of each model so the GPU is warm:

| Model | Decoding | Real-time factor | What came back |
|---|---|---|---|
| `tiny-q5_1` | 185 ms | 0.02x | one word wrong |
| `base-q5_1` | 203 ms | 0.02x | right |
| `small-q5_1` | 476 ms | 0.05x | right |

Read it for what it is. This is the reference host, not the reference
device — a Raspberry Pi has no Metal and none of these numbers carry
over to it. The audio is speech synthesised by macOS, which is cleaner
and more evenly paced than any microphone will hand you. And the first
run of a model pays for warming the GPU up: `tiny` took 3.03 s cold and
185 ms warm, which is why the cold figure is not in the table.

### Window passes, Apple M4 Pro, processor

What one pass over a partial utterance costs, taken with
`probe_window`. These are the processor path, not Metal: a `ModelSpec`
that names no accelerator asks for the processor, which is the trap the
reading below is about. Median of repeated runs, machine at a load
average of about four across twelve cores.

| Model | 4 s buffered | 8 s buffered | Language |
|---|---|---|---|
| `tiny-q5_1` | 141 ms | 141 ms | English |
| `base-q5_1` | 245 ms | 245 ms | English |
| `small-q5_1` | 655 ms | 655 ms | English |
| `tiny-q5_1` | 154 ms | 201 ms | Korean |
| `base-q5_1` | 300 ms | 316 ms | Korean |
| `small-q5_1` | 680 ms | 806 ms | Korean |

**The cost barely moves with how much audio is in the buffer.** Whisper
encodes a thirty-second window whatever you hand it, so two seconds of
speech pays almost the same as eight. Anything that recognises a growing
utterance repeatedly should budget per pass, not per second of audio.

The same passes on Metal, `base-q5_1`, bounded to 400 frames: 23 ms at
four seconds and 65 ms at eight, against 72 ms and 159 ms on the
processor. Roughly three times, and the same shape.

### Bounding the encoder, and the cliff under it

`audio_ctx` bounds how much of that thirty-second window is encoded.
Tightening it is most of what makes repeated recognition affordable —
`base-q5_1` on four seconds of English went from 409 ms at the default
to 72 ms, returning the same words.

There is a floor, and it is sharper than it looks. A second of audio is
worth fifty frames, so four seconds is worth two hundred — and two
hundred is exactly where four seconds falls apart:

| Audio | Bound | Cost | What came back |
|---|---|---|---|
| 4 s | 1500 (default) | 409 ms | "And so my fellow Americans ask" |
| 4 s | 800 | 237 ms | same |
| 4 s | 400 | 72 ms | same |
| 4 s | 200 | 1.61 s | "and saw my fellow Americans ask" |
| 4 s | 150 | 2.20 s | "and so my fellow Americans," five times over |
| 8 s | 800 | 159 ms | "...ask not what your country can do for you." |
| 8 s | 600 | 203 ms | same |
| 8 s | 400 | 370 ms | the same sentence three times over |

Below the floor it is **slower as well as worse**. That is the
repetition collapse already described further down this file: whisper
starts repeating itself, and whisper.cpp answers by decoding the window
again at a higher temperature. Tightening too far buys nothing and costs
twice.

Korean behaved the same way with a little more room — four seconds held
together at two hundred frames where English did not, and eight seconds
was still clean at four hundred. Twice the audio's own frame count was
safe everywhere it was tried, in both languages, at every buffer length.

Read these for what they are. One host machine, one quantised model
family, English from whisper.cpp's public sample and Korean synthesised
by macOS. No device-class figure exists yet, and that is the one that
decides whether any of this is affordable where it matters.

### The void reading

| Machine | Build | Model | Audio | Decoding | Real-time factor |
|---|---|---|---|---|---|
| Apple Silicon laptop | default features, no GPU | `ggml-tiny-q5_1` | 2.71 s | 296 s | 109x, **meaningless** |

The machine was carrying a load average of 175 across 12 cores at the
time -- dozens of unrelated processes spinning. A decoder given a
fifteenth of a core tells you about the machine's queue, not about the
model. Do not quote this number or reason from it.

There is one real observation from the same run, unaffected by load:

```
whisper_backend_init_gpu: no GPU found
whisper_backend_init: using BLAS backend
```

That was read as Metal failing to compile in. It is not. The `metal`
feature builds and the library finds the GPU; what the line means is
that nothing had asked for it. An accelerator is requested at
construction, and a `ModelSpec` that does not name one asks for the
processor — so a build carrying Metal decodes on the processor until
told otherwise. Asked properly, the same build says:

```
whisper_init_with_params_no_state: use gpu    = 1
ggml_metal_device_init: GPU name:   Apple M4 Pro
whisper_model_load:        Metal total size =    59.12 MB
```

`--accelerator` on the transcribe example is how you ask, and
`probe_model MODEL metal` answers the question on its own, without
decoding anything. Neither existed when the reading above was taken,
which is why it was misread.

**Measure on a quiet machine.** Check first:

```bash
uptime          # load average well under the core count
sysctl -n hw.ncpu
```

## Two things worth knowing before you measure

Both were found by running the suite on a machine under heavy load,
and both are real.

**Whisper collapses on repetitive audio.** The same model, the same
machine, the same minute:

| Input | Audio | Decoding | Real-time factor | Segments |
|---|---|---|---|---|
| Varied speech | 16.3 s | 1,307 s | 80x | 4 |
| One second repeated twenty times | 20.0 s | 23,029 s | 1,151x | 1 |

Fourteen times slower, and twenty seconds collapsed into one segment.
Whisper starts repeating itself and whisper.cpp answers by decoding the
same window again at a higher temperature, up to six times. Never build
test audio by looping a short clip, and set a timeout if a caller might
hand you a stuck microphone.

**Threads are worth less than they look when the machine is shared.**
Asking for twelve threads on a machine where you get about one core's
worth of time made the process run at 118% CPU, not 1200%.
whisper.cpp's thread pool synchronises with spinning barriers at every
graph node, so oversubscription costs far more than the missing cores
would suggest. `ModelSpec::with_threads` exists for this; the default
of one thread per core is right for a device that owns itself and wrong
for one that shares.

## Still to measure

The two reference machines named in the plan, every model size, Korean
and English, each with the real-time factor, the time to first partial,
and peak memory. The table stays honest by staying empty until then.

The window-pass figures above have the same hole in them: they are the
host, on the processor and on Metal, and say nothing about a board. The
model sizes a server would run are not there either, because they are
not on this machine.

## What the library promises about speed

Only that it costs almost nothing on top of the model: within 10% of
the same model run through whisper.cpp's own binary on the same machine
and the same audio. `cargo test -p edge-stt-core --test overhead --
--ignored` checks it.

Everything else -- how fast that model is on your board, in your
language -- is yours to measure and yours to choose. The library will
not refuse a combination for being slow, and every transcript carries
what it cost.
