# Measurements

Which model size to run is your decision. These are the numbers to
decide with. Reproduce them on your own board:

```bash
cargo run --release --example transcribe -- --bench MODEL sample.wav
```

## What has actually been measured

Nothing usable. One figure was taken and it is void.

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

Metal was not compiled in, even with the `metal` feature turned on.
That is worth chasing on its own account, but it is not what produced
the 109x above, and the two should not be confused.

**Measure on a quiet machine.** Check first:

```bash
uptime          # load average well under the core count
sysctl -n hw.ncpu
```

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
