"""What Session promises, checked from Python.

Run after `maturin develop`. Tests needing real model files look for
EDGE_STT_MODEL_DIR and EDGE_STT_VAD_MODEL, the same way the Rust
tests do.
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


def vad_model_path():
    path = os.environ.get("EDGE_STT_VAD_MODEL")
    if not path:
        pytest.skip("set EDGE_STT_VAD_MODEL to a ggml Silero VAD file")
    return path


def test_opening_a_session_needs_a_vad_model_that_exists():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        with pytest.raises(edge_stt.ModelMissingError):
            stt.open_session(vad_model="/no/such/vad.bin")


def test_a_session_is_a_context_manager_and_closing_twice_is_fine():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        with stt.open_session(vad_model=vad_model_path()) as session:
            silence = b"\x00\x00" * 1_600
            for _ in range(3):
                assert session.push(silence) is None
        # __exit__ already closed it; a second close is a no-op, not an error.
        assert session.close() is None


def test_only_one_session_may_be_open_at_a_time():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        first = stt.open_session(vad_model=vad_model_path())
        with pytest.raises(edge_stt.InvalidValueError):
            stt.open_session(vad_model=vad_model_path())
        first.close()
        # Freed by closing the first: a second one can now be opened.
        stt.open_session(vad_model=vad_model_path()).close()
