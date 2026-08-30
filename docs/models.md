# Models

edge-stt ships no weights. You choose the file, and the device and the
server should not be given the same one.

## Why the two ends differ

A server has memory, cooling, and an accelerator, and it answers for
several clients from one loaded model. A device has one core budget,
one thermal envelope, and one speaker to answer. The same file cannot
be right for both: the size a server barely notices will not fit a
Raspberry Pi, and the size a Pi runs comfortably is beneath what a
server should be spending its accuracy budget on.

So the model is part of the deployment, not part of the code. Nothing
in edge-stt refuses a combination for being slow, and every transcript
carries what it cost, so a wrong choice reports itself rather than
hiding.

## Where the file goes

Never inside the repository. `models/`, `*.bin`, `*.gguf`, and `*.ggml`
are ignored anywhere in the tree, and continuous integration fails if a
file shaped like weights is ever tracked. That is not a rule to
remember; it is checked.

Point `EDGE_STT_MODEL_DIR` at wherever you keep them. The tests and the
fetch script both read it, the same way edge-ear finds wake words.

```bash
export EDGE_STT_MODEL_DIR=~/models/whisper
```

## Getting one

```bash
scripts/fetch-model.sh --list
scripts/fetch-model.sh base-q5_1
```

The script downloads from the whisper.cpp model repository and checks
the file against the SHA-256 written down in `THIRD-PARTY-LICENSES`. A
file that hashes differently is deleted rather than kept. A model the
record does not name is refused: add it there — origin, licence, and
checksum — before fetching it.

## The ladder

| Model | On disk | Speaks | Usually belongs |
|---|---|---|---|
| `tiny-q5_1` | 31 MB | any | a device, when answering fast matters more than answering well |
| `base-q5_1` | 57 MB | any | a device; the place to start |
| `small-q5_1` | 181 MB | any | a device that can afford it, and the smallest that handles Korean |
| `medium-q5_0` | 514 MB | any | a server |
| `large-v3-turbo-q5_0` | 547 MB | any | a server, when accuracy is worth the wait |
| `large-v3-q5_0` | 1.0 GB | any | a server with room |
| `tiny.en-q5_1` `base.en-q5_1` `small.en-q5_1` | as above | English only | a device in an English-only product |
| `base` `small` | 141 MB, 465 MB | any | neither; they are here so you can measure what quantisation cost you |

How much memory each needs while decoding is not quoted here, because
this project has not measured it. [measurements.md](measurements.md)
says what has been taken and what has not.

## Quantisation

Every model the record pins is quantised except the last two. `q5_1` and
`q5_0` are a bit over a third the size of the half-precision original —
`base` is 141 MB and `base-q5_1` is 57 MB — and that is most of why a
device can run them at all.

What quantisation costs in accuracy depends on your audio, and this
project has not measured it either. `base` and `small` are pinned
unquantised so that you can, on your own recordings, before deciding.

## English-only models

The `.en` files transcribe English and nothing else. Ask one for another
language and the transcriber refuses to build:

```
the model at ggml-base.en-q5_1.bin cannot be used: speaks only
English, but ko was asked for
```

That refusal only happens when you have set a language. Leave the
language unset and an English-only model takes Korean audio without
complaint and writes down what it would be if it were English. On a
device, set the language.

## Korean

On a Pi-class board the sizes that beat real time — `tiny` and `base` —
are poor at Korean, and the smallest size that transcribes Korean
acceptably, `small`, does not beat real time there. That is the model's
own arithmetic and no amount of implementation moves it.

There are three honest answers, and which one is right is yours to pick:

- Run `small` on the device and accept that a sentence takes longer to
  transcribe than it took to say.
- Keep `base` on the device for English and send Korean to a server.
- Wait for the board. A Jetson-class device with an accelerator changes
  the arithmetic; a Pi 4 does not.

Whichever you pick, set the language rather than leaving it to
detection. A short clip and a small model guess badly, and a wrong guess
costs more than the setting saves. Measured here, `tiny-q5_1` given 2.7
seconds of English:

```
whisper_full_with_state: auto-detected language: ko (p = 0.714381)
다켓 브라운 폭스 점프스 오버 덜해지톡
```

The words are "the quick brown fox jumps over the lazy dog". Nothing
went wrong that an error could report: the model was asked to guess, it
guessed Korean with some confidence, and then it did what it was told.

## Running it on the device

Default features are the on-device build: whisper.cpp and no network
code at all. The accelerator is a feature too, and asking at runtime for
one the build does not carry is an error rather than a quiet fall back
to the processor.

```bash
cargo build --release                    # processor only
cargo build --release --features metal   # Apple silicon
cargo build --release --features cuda    # Jetson and other NVIDIA boards
cargo build --release --features vulkan  # everything else with a GPU
```

Release, always. whisper.cpp compiled for debugging is unusably slow and
will make you think the model is at fault.

Building it in is half of it. The accelerator is also asked for at
construction, and a `ModelSpec` that does not name one asks for the
processor — so a build carrying Metal will decode on the processor, at
processor speed, and say nothing about it. Name it:

```rust
ModelSpec::at(path).with_accelerator(Accelerator::Metal)
```

Check the board before believing anything else about it. This loads the
model and nothing more, so it answers in seconds:

```bash
cargo run --release --features metal --example probe_model -- $MODEL metal
```

`use gpu = 1` and a `Metal total size` line mean it is really there.
`no GPU found` means it is not, and then the size of the model is not
your problem yet.

One thread per core is the default and is right for a device that owns
itself. On a board sharing its cores with the rest of a product, say so
with `ModelSpec::with_threads`: whisper.cpp synchronises its thread pool
with spinning barriers, so asking for more threads than you will get
costs more than the missing cores do.

## Before trusting a file

Models fail quietly when fed the wrong shape. Print what one actually
is before believing anything it says:

```bash
cargo run --release --example probe_model -- $EDGE_STT_MODEL_DIR/ggml-base-q5_1.bin
```
