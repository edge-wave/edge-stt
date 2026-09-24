/**
 * @file
 * @brief Every promise the C surface makes, checked from C. Needs no
 *        model: a handle without one must say so rather than crash.
 */
#include <stdio.h>
#include <string.h>

#include "edge_stt.h"

#define CHECK(what)                                                                    \
    do {                                                                               \
        if (!(what)) {                                                                 \
            fprintf(stderr, "failed at line %d: %s\n", __LINE__, #what);               \
            return 1;                                                                  \
        }                                                                              \
    } while (0)

int main(void) {
    /* A null handle is an error, never a crash. */
    CHECK(edge_stt_load_model(NULL, "anything") == EDGE_STT_NULL_ARGUMENT);
    CHECK(edge_stt_cancel(NULL) == EDGE_STT_NULL_ARGUMENT);
    edge_stt_free(NULL);

    edge_stt_h stt = edge_stt_new();
    CHECK(stt != NULL);

    /* Transcribing before a model is loaded says exactly that. */
    int16_t samples[16000] = {0};
    edge_stt_transcript_h out = NULL;
    CHECK(edge_stt_transcribe(stt, samples, 16000, 16000, &out) == EDGE_STT_NO_MODEL);
    CHECK(out == NULL);
    CHECK(strlen(edge_stt_get_last_error()) > 0);

    /* The wrong sample rate is refused, naming what was expected. */
    CHECK(edge_stt_transcribe(stt, samples, 16000, 44100, &out) == EDGE_STT_UNSUPPORTED_AUDIO);
    CHECK(strstr(edge_stt_get_last_error(), "16000") != NULL);

    /* Null samples are refused. */
    CHECK(edge_stt_transcribe(stt, NULL, 0, 16000, &out) == EDGE_STT_NULL_ARGUMENT);

    /* A model that is not there is found out at load, not at use. */
    CHECK(edge_stt_load_model(stt, "/no/such/model.bin") == EDGE_STT_MODEL_MISSING);
    CHECK(strstr(edge_stt_get_last_error(), "/no/such/model.bin") != NULL);

    /* Settings take before a model is loaded. */
    CHECK(edge_stt_set_language(stt, "ko") == EDGE_STT_OK);
    CHECK(edge_stt_set_language(stt, NULL) == EDGE_STT_OK);
    CHECK(edge_stt_set_timeout(stt, 5000) == EDGE_STT_OK);
    CHECK(edge_stt_set_partial_cb(stt, NULL, NULL) == EDGE_STT_OK);

    /* Cancelling is safe whether or not anything is running. */
    CHECK(edge_stt_cancel(stt) == EDGE_STT_OK);

    /* Accessors on a null transcript give nothing rather than crash. */
    CHECK(edge_stt_transcript_get_text(NULL) == NULL);
    CHECK(edge_stt_transcript_get_language(NULL) == NULL);
    CHECK(edge_stt_transcript_get_segment_count(NULL) == 0);
    CHECK(edge_stt_transcript_get_segment_text(NULL, 0) == NULL);
    CHECK(edge_stt_transcript_get_audio_ms(NULL) == 0);
    edge_stt_transcript_free(NULL);

    /* A null session handle is an error, never a crash. */
    CHECK(edge_stt_session_push(NULL, samples, 16000) == EDGE_STT_NULL_ARGUMENT);
    CHECK(edge_stt_session_close(NULL) == EDGE_STT_NULL_ARGUMENT);
    CHECK(edge_stt_session_set_transcript_cb(NULL, NULL, NULL) == EDGE_STT_NULL_ARGUMENT);
    CHECK(edge_stt_session_set_partial_cb(NULL, NULL, NULL) == EDGE_STT_NULL_ARGUMENT);
    edge_stt_session_free(NULL);

    /* Opening a session before a model is loaded says exactly that. */
    edge_stt_session_opts missing_vad = {0};
    missing_vad.struct_size = sizeof(missing_vad);
    missing_vad.vad_model = "/no/such/vad.bin";
    CHECK(edge_stt_session_new(stt, &missing_vad) == NULL);
    CHECK(edge_stt_session_new(stt, NULL) == NULL);
    edge_stt_session_opts unsized = {0};
    CHECK(edge_stt_session_new(stt, &unsized) == NULL);
    CHECK(strlen(edge_stt_get_last_error()) > 0);

    /* Boundaries come from a detector or from the caller, never both. */
    edge_stt_session_opts both = {0};
    both.struct_size = sizeof(both);
    both.vad_model = "/no/such/vad.bin";
    both.caller_boundaries = 1;
    CHECK(edge_stt_session_new(stt, &both) == NULL);
    CHECK(strstr(edge_stt_get_last_error(), "caller_boundaries") != NULL);
    edge_stt_session_opts both_ways = {0};
    both_ways.struct_size = sizeof(both_ways);
    both_ways.detect_boundaries = 1;
    both_ways.caller_boundaries = 1;
    CHECK(edge_stt_session_new(stt, &both_ways) == NULL);
    CHECK(strstr(edge_stt_get_last_error(), "caller_boundaries") != NULL);

    /* A server is named at connect and first reached when asked for something. */
    edge_stt_h remote = edge_stt_new();
    CHECK(edge_stt_set_credential(remote, NULL) == EDGE_STT_OK);
    CHECK(edge_stt_set_connect_timeout(remote, 2000) == EDGE_STT_OK);
    CHECK(edge_stt_set_fallback_model(remote, NULL) == EDGE_STT_OK);
    CHECK(edge_stt_connect(remote, NULL) == EDGE_STT_NULL_ARGUMENT);
    CHECK(edge_stt_connect(remote, "ws://127.0.0.1:1/api/v1/transcribe") == EDGE_STT_OK);
    CHECK(edge_stt_transcribe(remote, samples, 16000, 16000, &out) == EDGE_STT_NETWORK);
    edge_stt_session_opts caller = {0};
    caller.struct_size = sizeof(caller);
    caller.caller_boundaries = 1;
    CHECK(edge_stt_session_new(remote, &caller) == NULL);
    CHECK(strlen(edge_stt_get_last_error()) > 0);
    edge_stt_free(remote);

    edge_stt_free(stt);
    printf("the C surface holds\n");
    return 0;
}
