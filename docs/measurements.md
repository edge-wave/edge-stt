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

## What the library promises about speed

Only that it costs almost nothing on top of the model: within 10% of
the same model run through whisper.cpp's own binary on the same machine
and the same audio. `cargo test -p edge-stt-core --test overhead --
--ignored` checks it.

Everything else -- how fast that model is on your board, in your
language -- is yours to measure and yours to choose. The library will
not refuse a combination for being slow, and every transcript carries
what it cost.
