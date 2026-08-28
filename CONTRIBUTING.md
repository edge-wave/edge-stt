# Contributing

## Say the thing, not its number

Planning documents number what they list. Those numbers mean nothing
outside them and go stale the moment one is rewritten.

So no number, label, or shorthand from a planning document belongs in
code, comments, commit messages, or anything here. Write the complete
sentence instead.

```
Bad:   // Requirement 23
Good:  // Whisper never revises a segment it has already handed over.
```

## Comments

Few, and short. Two lines is the limit. A comment says why, not what
the line above already says.

## Commit messages

English, wrapped, ten lines at most. Documentation and test changes go
in their own commits, apart from the implementation they belong to.

## Models

No model weights live here. The caller supplies them, for the same
licensing reason edge-ear ships no wake word.

Nothing is referenced from the documentation without checking what it
is licensed under and writing that down in `THIRD-PARTY-LICENSES`, with
a checksum and where it came from.

Models fail quietly when fed the wrong shape. Print a model's real
parameters before trusting them:

```bash
cargo run --example probe_model -- path/to/ggml-base-q5_0.bin
```

Pin what you learn in a test.

## The C API is generated

`capi/include/edge_stt.h` comes out of cbindgen and is committed as
generated. Do not edit it. Write the Rust doc comment instead, with
`@brief`, `@param`, `@return`, and `@see`, and regenerate:

```bash
cbindgen --config capi/cbindgen.toml --crate edge-stt-capi \
    --output capi/include/edge_stt.h
```

`capi/Doxyfile` reads that header alone, so the C documentation is
generated from the same source and cannot drift from it.

## Before opening a pull request

```bash
cargo clippy --workspace --all-targets --features full -- -D warnings
cargo fmt --all --check
cargo test --workspace
```

`--all-features` is not the check to run: it turns on the CUDA and
Vulkan accelerators, which need toolchains most machines do not have.
`full` is whisper plus remote, which builds everywhere.

Tests needing a real model file are marked ignored and do not run by
default:

```bash
EDGE_STT_MODEL_DIR=~/models/whisper cargo test --workspace -- --ignored
```
