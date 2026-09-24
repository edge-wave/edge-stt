"""What Session promises, checked from Python.

Run after `maturin develop`. Tests needing real model files look for
EDGE_STT_MODEL_DIR and EDGE_STT_VAD_MODEL, the same way the Rust
tests do.
"""

import os
import pathlib
import wave

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


def sample_wav_samples():
    path = os.environ.get("EDGE_STT_SAMPLE_WAV")
    if not path:
        pytest.skip("set EDGE_STT_SAMPLE_WAV to a 16 kHz mono 16-bit recording of speech")
    with wave.open(path, "rb") as wav:
        return wav.readframes(wav.getnframes())


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
        assert session.close() == []


def test_push_and_close_both_deliver_partials_for_real_speech():
    # A short clip like this rarely hits a natural pause before the
    # loop runs out of audio, so the actual decode -- and its
    # partials -- usually happens inside close(), not any push().
    # Both are wired the same way, so on_partial is passed to both.
    samples = sample_wav_samples()
    seen = []
    with edge_stt.EdgeStt(model=model_path()) as stt:
        with stt.open_session(vad_model=vad_model_path()) as session:
            chunk = 1_600 * 2  # bytes: 1600 samples * 2 bytes/sample
            transcript = None
            for start in range(0, len(samples), chunk):
                found = session.push(
                    samples[start : start + chunk], on_partial=seen.append
                )
                if found is not None:
                    transcript = found
            closed = session.close(on_partial=seen.append)
            transcript = closed[-1] if closed else transcript

    # One long push loop could in principle span more than one utterance,
    # so seq is only checked per-partial, not for a single 0..n run.
    assert seen, "a real recording of speech should produce at least one partial"
    assert all(isinstance(p, edge_stt.Partial) and p.seq >= 0 for p in seen)
    assert transcript is not None
    assert transcript.text.strip() != ""


def test_several_sessions_may_be_open_at_once():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        first = stt.open_session(vad_model=vad_model_path())
        second = stt.open_session(vad_model=vad_model_path())
        first.close()
        second.close()


def test_boundaries_come_from_a_vad_model_or_the_caller_not_both():
    with edge_stt.EdgeStt(model=model_path()) as stt:
        with pytest.raises(edge_stt.InvalidValueError):
            stt.open_session(vad_model="/no/such/vad.bin", caller_boundaries=True)


def test_a_caller_bounded_session_delivers_only_at_close():
    samples = sample_wav_samples()
    with edge_stt.EdgeStt(model=model_path()) as stt:
        with stt.open_session(caller_boundaries=True) as session:
            chunk = 1_600 * 2
            for start in range(0, len(samples), chunk):
                assert session.push(samples[start : start + chunk]) is None
            transcripts = session.close()

    assert len(transcripts) == 1
    transcript = transcripts[0]
    assert transcript.audio_duration == pytest.approx(len(samples) / 32_000)
    assert transcript.text.strip() != ""
