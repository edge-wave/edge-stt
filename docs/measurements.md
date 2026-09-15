# Measurements

Which model size to run is your decision. These are the numbers to
decide with. Reproduce them on your own board:

```bash
cargo run --release --example transcribe -- --bench MODEL sample.wav
```

## What has actually been measured

Both halves of the reference pair, taken on the same two recordings:
every model on disk, both languages, every path each machine has.

### Apple M4 Pro, both paths, both languages

12 cores, 48 GB, macOS. 5.66 s of English and 5.32 s of Korean, the
language named rather than detected. **Each cell is the median of five
runs**, which is not fussiness: a single pair of runs on this machine
produced a language gap that five runs say is not there. Peak resident
set is what `/usr/bin/time -l` reported for the whole process, model
included, and moves too little between runs to need a median.

| Model | Path | Language | Decoding | Real-time factor | Peak RSS | What came back |
|---|---|---|---|---|---|---|
| `tiny-q5_1` | processor | English | 180 ms | 0.03x | 157 MB | right |
| `tiny-q5_1` | processor | Korean | 204 ms | 0.04x | 157 MB | right |
| `base-q5_1` | processor | English | 297 ms | 0.05x | 226 MB | right |
| `base-q5_1` | processor | Korean | 303 ms | 0.06x | 225 MB | one word wrong |
| `small-q5_1` | processor | English | 757 ms | 0.13x | 466 MB | right |
| `small-q5_1` | processor | Korean | 759 ms | 0.14x | 466 MB | right |
| `tiny-q5_1` | Metal | English | 100 ms | 0.02x | 121 MB | right |
| `tiny-q5_1` | Metal | Korean | 98 ms | 0.02x | 121 MB | right |
| `base-q5_1` | Metal | English | 128 ms | 0.02x | 166 MB | right |
| `base-q5_1` | Metal | Korean | 133 ms | 0.03x | 166 MB | one word wrong |
| `small-q5_1` | Metal | English | 256 ms | 0.05x | 355 MB | right |
| `small-q5_1` | Metal | Korean | 256 ms | 0.05x | 355 MB | right |

**The ladder is not monotone in a language it was not tuned for.**
`base` misheard one Korean word — "불어서" as "부러서" — on both paths
here and on the board as well, where the smaller `tiny` and the larger
`small` both got it right on all three. A bigger model is a better bet,
not a guarantee, and the only way to know for your language is to run
yours.

**An earlier version of this table said Korean costs more than English
on the processor. It does not, and the retraction is the lesson.** That
claim rested on one pair of runs on a machine carrying other work — 255
against 452 ms. Five runs a side on a quiet machine put `base` at 297 ms
in English and 303 ms in Korean, well inside each other's spread, and
the board did not reproduce a gap either. One run of a thing this noisy
measures the machine's queue as much as the model.

Read it for what it is. This is the reference host, not the reference
device — a Raspberry Pi has no Metal and none of these numbers carry
over to it. The audio is speech synthesised by macOS, which is cleaner
and more evenly paced than any microphone will hand you. And the first
run after a boot pays for warming the GPU up: `tiny` on Metal took
5.64 s to load cold and 63 ms warm, which is why the cold figure is not
in the table.

### Raspberry Pi 4, the device half

Model B rev 1.4, four cores at 1.8 GHz, 4 GB, 64-bit Raspberry Pi OS,
no accelerator to ask for. The same two recordings, the same models, the
warm run of each pair. Peak resident set is the kernel's own figure for
the process. The board was cooled below 58 °C before every model, which
matters — see the heat section below.

| Model | Language | Decoding | Real-time factor | Peak RSS | What came back |
|---|---|---|---|---|---|
| `tiny-q5_1` | English | 4.43 s | 0.78x | 169 MB | right |
| `tiny-q5_1` | Korean | 4.51 s | 0.85x | 169 MB | right |
| `base-q5_1` | English | 9.80 s | 1.73x | 241 MB | right |
| `base-q5_1` | Korean | 9.99 s | 1.88x | 241 MB | one word wrong |
| `small-q5_1` | English | 34.33 s | 6.07x | 480 MB | right |
| `small-q5_1` | Korean | 34.75 s | 6.54x | 480 MB | right |

**Only the smallest model keeps up with speech.** `tiny` decodes a
finished utterance in about four fifths of the time it took to say it.
`base` takes nearly twice as long as the speaking, and `small` six
times. A device running anything above `tiny` falls further behind with
every utterance, and no amount of buffering fixes a decoder that is
slower than the speaker.

**The penalty grows with the model.** Against the host's processor path,
same audio, same files: `tiny` is 22–25x slower here, `base` 33x, and
`small` about 45x. The board does not simply scale the host down by a
constant — it punishes the larger model harder, which is the opposite of
what a "just use a smaller model" rule of thumb assumes about the size
of the saving.

**Every model said the same words as the host, to the character** —
including `base`'s Korean mistake and `small`'s choice to split the
English sentence in two. The board is slower, not worse. Whatever a
model gets wrong, it gets wrong everywhere, so correctness can be
decided on the fast machine and only the cost has to be measured on the
slow one.

**What a model costs to load, once.** 155 ms for `tiny`, 195 ms for
`base`, 390 ms for `small` — but `small` took 4.32 s the first time it
was read from the card, against 390 ms once the page cache held it. A
process that loads the model per request pays that first figure.

### What a caller actually waits for, over the wire

Everything above times a decode. This times a **caller**: `stream_client`
pushes a recording at a server at the rate a microphone would produce
it, and stamps every reply from the moment the stream opened. Server and
client both on the M4 Pro, so the link costs nothing here — this is the
floor the network is later added to. Language named on the server,
median of five runs.

| Model | Language | Audio ran | Final arrived | After the speaker stopped |
|---|---|---|---|---|
| `tiny-q5_1` | English | 5.66 s | 6.10 s | 0.44 s |
| `tiny-q5_1` | Korean | 5.32 s | 5.88 s | 0.56 s |
| `base-q5_1` | English | 5.66 s | 6.35 s | 0.69 s |
| `base-q5_1` | Korean | 5.32 s | 5.97 s | 0.65 s |
| `small-q5_1` | English | 5.66 s | 6.96 s | 1.30 s |
| `small-q5_1` | Korean | 5.32 s | 6.78 s | 1.46 s |

**The last column is the one to budget with.** A caller cannot be
answered before they stop talking, so what a server costs is the wait
after that — boundary detection, the decode, and getting the words back.
It is larger than the decode alone: `small` decodes in 757 ms here and
answers a caller in 1.30 s.

**Against the board deciding for itself**, from the device table above:
the Pi needs 4.43 s after the speaker stops with `tiny`, its only model
that keeps up with speech at all. A host answers in 1.30 s while running
`small`, three rungs up. Handing the audio to a server is not a
compromise on this pair of machines — it is faster *and* better, and the
only thing it costs is the link.

**Name the language on the server.** The same `small` run with detection
left open answered in 2.39 s against 1.55 s named — most of a second,
spent deciding something the deployment already knew.

#### With the caller on the board and the recogniser on the host

The same runs again with `stream_client` on the Raspberry Pi and the
server on the M4 Pro, over tailscale on one LAN — a direct path, not a
relay, at 5.9 to 31.8 ms round trip. Median of five.

| Model | Language | Caller on the board | Caller on the host | The link added |
|---|---|---|---|---|
| `tiny-q5_1` | English | 0.40 s | 0.44 s | −0.04 s |
| `tiny-q5_1` | Korean | 0.62 s | 0.56 s | +0.06 s |
| `base-q5_1` | English | 0.68 s | 0.69 s | −0.01 s |
| `base-q5_1` | Korean | 0.68 s | 0.65 s | +0.03 s |
| `small-q5_1` | English | 1.45 s | 1.30 s | +0.15 s |
| `small-q5_1` | Korean | 1.42 s | 1.46 s | −0.04 s |

**The link costs nothing this method can see.** The differences straddle
zero and every one of them is smaller than the spread between repeats of
the same cell — half a second, for `small` in English. That is not a
surprise once said plainly: the audio is 32 kB a second and it is
already uploaded by the time the speaker stops, because it went up while
they were still talking. Only the answer has to come back, and that is
one message over one round trip.

So the deployment the earlier figures kept pointing at costs what the
server costs. A board that decides for itself waits 4.43 s with the only
model that keeps up with speech there; the same board as a thin client
waits 1.42 s and gets `small`.

**What this table does not include, and it dominates everything in it.**
The client sends `close_stream` the moment the recording ends, so the
utterance closes at once. A live microphone has no such signal: the
boundary detector waits out `pause_tolerance_ms` of silence first —
three seconds by default, five times the largest number in the table.
Tuning that is worth more to a caller than the choice of model.

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

### Where a pass ends, and why the closing word is held back

A pass cuts its buffer wherever the last chunk landed, so its last word
is half a word as often as not — four seconds of Korean ended in "정"
and four of English in a dangling "ask". Trimming that by timing is the
obvious answer and it cannot be done. `probe_window` now prints the
segments a pass returned, and a short buffer comes back as exactly one:

| Language | Buffered | Bound | Segments | Span |
|---|---|---|---|---|
| Korean | 1 s | 200 | 1 | 0.00–2.00 |
| Korean | 2 s | 200 | 1 | 0.00–4.00 |
| Korean | 4 s | 400 | 1 | 0.00–4.16 |
| English | 1 s | 200 | 1 | 0.00–1.00 |
| English | 2 s | 200 | 1 | 0.00–2.00 |
| English | 4 s | 400 | 1 | 0.00–4.00 |

The single segment spans the whole buffer, and it ends at the buffer's
own edge or past it — where the model put a timestamp inside the padded
window it was handed. Nothing there says where speech stopped, so there
is no timing to trim by. The live recogniser holds its closing word back
instead and shows it once a later pass has heard past it, which costs a
caption one word of lag and never shows a broken one. `base-q5_1`, Apple
M4 Pro, processor.

### Priming a pass with the last one, and why it is not done

Wrappers around this recogniser commonly feed the text so far back in as
a prompt. `probe_window --carry-prompt` does exactly that over a growing
buffer, and it is worse in both languages. `base-q5_1`, Apple M4 Pro,
processor, each pass bounded as the live recogniser bounds it:

| Buffered | Plain | Primed with the previous pass |
|---|---|---|
| 3 s, English | "...ask not what your country can" | "what your country can. And so my fellow Americans ask not what your country can." |
| 4 s, English | "...can do for you, ask" | "country can do for you." |
| 4 s, Korean | "오늘 날씨가 아주 맑고 ... 산책하기에 정" | "불어서, 산책하기에 정..." |

The mechanism is plain in the output: a prompt tells the decoder to
*continue* that text, but a pass here re-recognises the utterance from
its beginning. The two instructions fight, and what comes back is the
sentence duplicated, or its opening thrown away — which a replacing
interim would show as a caption jumping backwards. It costs more too:
three seconds of Korean went from 118 ms to 1.59 s, and two of English
from 236 ms to 1.29 s.

Priming belongs to designs that hand the recogniser only the newest
audio each time. This one hands it everything, so the context a prompt
would supply is already in the audio.

### Window passes, Raspberry Pi 4, processor

The same passes on the device this project targets. A Raspberry Pi 4
Model B rev 1.4, four cores at 1.8 GHz, 4 GB, 64-bit Raspberry Pi OS.
English, `probe_window`, no accelerator — there is none to ask for.

At the library's default bound, four seconds buffered:

| Model | Host | Device | Ratio |
|---|---|---|---|
| `tiny-q5_1` | 141 ms | 4.87 s | 35x |
| `base-q5_1` | 245 ms | 13.4 s | 55x |
| `small-q5_1` | 655 ms | 53.6 s | 80x, and throttled |

Bounded to twice the audio's own frame count, which is the setting the
section above says to use:

| Model | Audio | Bound | Device | What came back |
|---|---|---|---|---|
| `base-q5_1` | 4 s | 1500 | 11.26 s | "And so my fellow Americans ask" |
| `base-q5_1` | 4 s | 800 | 5.19 s | same |
| `base-q5_1` | 4 s | 400 | 2.21 s | same |
| `base-q5_1` | 4 s | 200 | 17.49 s | worse, and eight times slower |
| `tiny-q5_1` | 4 s | 1500 | 6.35 s | "And so my fellow Americans! Ask!" |
| `tiny-q5_1` | 4 s | 400 | 1.13 s | "And so my fellow Americans asked" |
| `tiny-q5_1` | 4 s | 200 | 11.81 s | "...ask the soldiers", which nobody said |
| `tiny-q5_1` | 2 s | 400 | 1.06 s | "and so my fellow Americans" |

**The bound behaves identically on both machines.** The floor sits at
the same place, the saving is the same four fifths, and the collapse
below it is the same collapse — only the penalty is larger here, eight
times rather than the twenty the host showed as a multiple of a much
smaller number. Where to set the bound is a property of the model, not
of the board, which is worth knowing: it can be decided once.

**What the device costs is another matter.** A pass at the safe bound is
1.1 s with the smallest model and 2.2 s with the one this project tells
people to start with. Whatever recognises a growing utterance on this
board produces an interim about once a second at best, and only with the
smallest model. Read the ratios above rather than the host table when
deciding whether that is enough.

### Heat is not a footnote here

One sweep — twenty-seven passes across three models — took the board
from 61 to 84 degrees and into throttling:

```
before  temp=61.3'C  throttled=0x0       arm=1.8 GHz
after   temp=83.7'C  throttled=0xe0008   arm=1.58 GHz
```

`0xe0008` is the soft temperature limit active now, with frequency
capping and throttling recorded as having happened. The `small` figures
above were taken inside that state, and drift within a single group
shows it arriving: the same measurement repeated three times gave 48.7,
51.0 and 52.0 seconds.

This matters more for repeated recognition than for anything else this
file measures. Decoding a finished utterance is a burst. Recognising a
growing one is sustained load by definition, so throttling is not the
exception there, it is the operating condition — and every device figure
above is the optimistic end of what a long session would see.

**Cooling between runs is what keeps a table honest, and it is cheap.**
The device table above waited for the board to fall below 58 °C before
each model; each wait cost 15 to 150 seconds, and only the `small` runs
still latched the soft-temperature bit. Six of the same decodes run
back to back with no waiting ended at 82.3 °C with `throttled=0xf0008`
— the soft limit *active*, not merely recorded. The words were the same
either way. Timings taken in that state are not.

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
would suggest. `ModelSpec::with_threads` exists for this, and the
server's `--threads` carries it to every session it opens; the default
of one thread per core is right for a device that owns itself and wrong
for one that shares. Two live sessions on one machine are exactly the
sharing case: each asks for the whole processor, and they contend for
every node until an operator divides the cores between them. The size
of that is easy to underrate — one test binary running two live
sessions at once had not finished after thirty minutes on twelve cores,
and finished in 1.9 s once each session was given half of them.

## Still to measure

Both halves of the reference pair are above. Two things are still
missing, and neither is a matter of reading:

- **The sizes a server would run.** `medium` and `large-v3` are on
  neither machine's disk, so the ladder stops at `small`.
- **Memory under several sessions at once.** The figures above are one
  session; a server holding one model and several live states is the
  case the capacity limit permits and nobody has weighed.
- **A link that is not a LAN.** The device-to-host leg was measured on
  one local network with a direct path. What a relayed or distant link
  adds is not known, and it is the case where the answer's single round
  trip stops being free.

The window-pass figures cover both machines, but only in English and
only for the three smallest models. Korean window passes on the board
are still not measured, though whole-utterance decoding now is.

## What the library promises about speed

Only that it costs almost nothing on top of the model: within 10% of
the same model run through whisper.cpp's own binary on the same machine
and the same audio. `cargo test -p edge-stt-core --test overhead --
--ignored` checks it.

Everything else -- how fast that model is on your board, in your
language -- is yours to measure and yours to choose. The library will
not refuse a combination for being slow, and every transcript carries
what it cost.
