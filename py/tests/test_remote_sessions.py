"""A server named from Python, and sessions it runs.

The round trip needs a running server at EDGE_STT_SERVER and a recording
at EDGE_STT_SAMPLE_WAV; everything else needs neither.
"""

import os
import time
import wave

import pytest

import edge_stt

NOWHERE = "ws://127.0.0.1:1/api/v1/transcribe"


def test_exactly_one_of_model_and_server_is_required():
    with pytest.raises(edge_stt.InvalidValueError):
        edge_stt.EdgeStt()
    with pytest.raises(edge_stt.InvalidValueError):
        edge_stt.EdgeStt(model="/no/such/model.bin", server=NOWHERE)


def test_server_settings_go_with_a_server():
    with pytest.raises(edge_stt.InvalidValueError):
        edge_stt.EdgeStt(model="/no/such/model.bin", credential="secret")


def test_a_server_is_named_now_and_reached_when_asked():
    stt = edge_stt.EdgeStt(server=NOWHERE, connect_timeout=2.0)
    assert stt.backend == "remote"
    with pytest.raises(edge_stt.NetworkError):
        stt.transcribe(b"\x00\x00" * 16_000)
    with pytest.raises(edge_stt.NetworkError):
        stt.open_session(caller_boundaries=True)


def test_boundaries_come_from_the_caller_or_a_detector_not_both():
    stt = edge_stt.EdgeStt(server=NOWHERE)
    with pytest.raises(edge_stt.InvalidValueError):
        stt.open_session(caller_boundaries=True, detect_boundaries=True)


def test_a_caller_bounded_session_against_a_server_streams_and_closes_once():
    server = os.environ.get("EDGE_STT_SERVER")
    path = os.environ.get("EDGE_STT_SAMPLE_WAV")
    if not server or not path:
        pytest.skip("set EDGE_STT_SERVER and EDGE_STT_SAMPLE_WAV")
    with wave.open(path, "rb") as recording:
        samples = recording.readframes(recording.getnframes())

    stt = edge_stt.EdgeStt(server=server)
    seen = []
    with stt.open_session(caller_boundaries=True, live_interims=True) as session:
        chunk = 1_600 * 2
        for start in range(0, len(samples), chunk):
            found = session.push(samples[start : start + chunk], on_partial=seen.append)
            assert found is None
            time.sleep(0.1)
        closed = session.close()

    assert seen, "no interim arrived while the recording was being pushed"
    assert len(closed) == 1
    assert closed[0].text.strip() != ""
