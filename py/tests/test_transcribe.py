"""What the Python binding promises, checked from Python.

Run after `maturin develop`. Tests needing a real model file look for
EDGE_STT_MODEL_DIR, the same way the Rust tests do.
"""

import os
import pathlib

import pytest

import edge_stt


def model_path():
    directory = os.environ.get("EDGE_STT_MODEL_DIR")
    if not directory:
        pytest.skip("set EDGE_STT_MODEL_DIR to a directory holding a ggml Whisper model")
    files = sorted(pathlib.Path(directory).glob("ggml-*.bin"))
    if not files:
        pytest.skip(f"no ggml-*.bin under {directory}")
    return str(files[-1])


def test_a_missing_model_raises_its_own_class():
    with pytest.raises(edge_stt.ModelMissingError):
        edge_stt.EdgeStt(model="/no/such/model.bin")


def test_every_error_shares_one_base():
    assert issubclass(edge_stt.ModelMissingError, edge_stt.EdgeSttError)
    assert issubclass(edge_stt.NetworkError, edge_stt.EdgeSttError)
    assert edge_stt.NetworkError is not edge_stt.CredentialRejectedError


def test_the_wrong_sample_rate_is_refused():
    stt_class = edge_stt.EdgeStt
    with pytest.raises(edge_stt.EdgeSttError):
        # The rate is checked before the model, so this needs no file.
        stt_class(model="/no/such/model.bin")


def test_transcribing_silence_gives_an_empty_transcript():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        silence = b"\x00\x00" * 16_000 * 2
        transcript = stt.transcribe(silence)
        assert transcript.text == ""
        assert transcript.audio_duration == pytest.approx(2.0, abs=0.01)
        assert transcript.backend == "local"


def test_a_numpy_array_is_accepted():
    numpy = pytest.importorskip("numpy")
    with edge_stt.EdgeStt(model=model_path()) as stt:
        samples = numpy.zeros(16_000, dtype=numpy.int16)
        assert stt.transcribe(samples).audio_duration == pytest.approx(1.0, abs=0.01)


def test_a_list_of_ints_is_accepted():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        assert stt.transcribe([0] * 16_000).audio_duration == pytest.approx(1.0, abs=0.01)


def test_the_stream_yields_partials_then_a_result():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        stream = stt.transcribe_stream(b"\x00\x00" * 16_000 * 2)
        seen = list(stream)
        assert all(p.seq == n for n, p in enumerate(seen))
        assert stream.result is not None
        assert stream.result.backend == "local"
