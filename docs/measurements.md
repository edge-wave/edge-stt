# Measurements

Which model size to run is your decision. These are the numbers to
decide with. Reproduce them on your own board:

```bash
cargo run --release --example transcribe -- --bench MODEL sample.wav
```

## What has actually been measured

One machine so far, and the figure below is a warning rather than a
guide.

| Machine | Build | Model | Audio | Decoding | Real-time factor |
|---|---|---|---|---|---|
| Apple Silicon laptop | default features, no GPU | `ggml-tiny-q5_1` | 2.71 s | 296 s | **109x** |

That is not what a tiny model costs on Apple Silicon. It is what
whisper.cpp costs when it has been built without a GPU backend and
without the processor's own vector instructions:

```
whisper_backend_init_gpu: no GPU found
whisper_backend_init: using BLAS backend
```

The default `whisper-rs` build passes `-DGGML_METAL=OFF`, and turning
on the `metal` feature did not change what the running binary reported.
Until that is sorted out, **no figure from this machine means
anything**, and none should be quoted.

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
