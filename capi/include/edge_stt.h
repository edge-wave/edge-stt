/**
 * @file
 * @brief edge-stt: turn a recording into text.
 *
 * Generated from the Rust source by cbindgen; do not
 * edit. Regenerate with:
 *     cbindgen --config capi/cbindgen.toml --crate edge-stt-capi \
 *         --output capi/include/edge_stt.h
 *
 * Errors: every fallible call returns int. 0 is success, negative names
 * the failure, and edge_stt_get_last_error() describes the most recent one
 * on this thread.
 *
 * Memory: the caller owns the samples it passes in. Anything the
 * library hands back through an out-parameter is freed with its own
 * _free. Strings belong to the object they came from and die with it.
 *
 * Audio: 16000 Hz, one channel, 16-bit signed. Nothing else is taken,
 * and nothing is converted on your behalf.
 *
 * Threads: a handle is usable from several threads, and cancelling
 * from another thread is the point of edge_stt_cancel. edge_stt_free
 * must not run alongside another call on the same handle.
 */


#ifndef EDGE_STT_H
#define EDGE_STT_H

#include <stddef.h>
#include <stdint.h>

/**
 * What went wrong. Zero is success; everything else is negative, one
 * value for each failure the core reports.
 */
enum edge_stt_error
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : int32_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
    /**
     * It worked.
     */
    EDGE_STT_OK = 0,
    /**
     * A pointer that must not be null was null.
     */
    EDGE_STT_NULL_ARGUMENT = -1,
    /**
     * The audio is not 16000 Hz mono 16-bit.
     */
    EDGE_STT_UNSUPPORTED_AUDIO = -2,
    /**
     * The recording is longer than this transcriber accepts.
     */
    EDGE_STT_AUDIO_TOO_LONG = -3,
    /**
     * No model file at that path.
     */
    EDGE_STT_MODEL_MISSING = -4,
    /**
     * The file is not a model this backend can use.
     */
    EDGE_STT_MODEL_UNUSABLE = -5,
    /**
     * Not enough memory or compute for the model chosen.
     */
    EDGE_STT_INSUFFICIENT_RESOURCES = -6,
    /**
     * The server could not be reached.
     */
    EDGE_STT_NETWORK = -7,
    /**
     * The server refused the credential.
     */
    EDGE_STT_CREDENTIAL_REJECTED = -8,
    /**
     * The server has no room right now.
     */
    EDGE_STT_SERVER_AT_CAPACITY = -9,
    /**
     * The server failed on its own account.
     */
    EDGE_STT_SERVER_ERROR = -10,
    /**
     * The time limit ran out.
     */
    EDGE_STT_TIMEOUT = -11,
    /**
     * The caller cancelled it.
     */
    EDGE_STT_CANCELLED = -12,
    /**
     * A setting is outside what it allows.
     */
    EDGE_STT_INVALID_VALUE = -13,
    /**
     * This build does not carry that backend.
     */
    EDGE_STT_BACKEND_UNAVAILABLE = -14,
    /**
     * No model has been loaded into this handle yet.
     */
    EDGE_STT_NO_MODEL = -15,
    /**
     * Something in the library gave way.
     */
    EDGE_STT_INTERNAL = -99,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum edge_stt_error edge_stt_error;
#else
typedef int32_t edge_stt_error;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * What a handle points to. Opaque on the C side, which only ever
 * names the pointer to this: `edge_stt_h`.
 */
typedef struct edge_stt_handle edge_stt_handle;

/**
 * What a transcript handle points to. Opaque on the C side, which
 * only ever names the pointer to this: `edge_stt_transcript_h`. Owns
 * its strings so the caller never has to free one separately.
 */
typedef struct edge_stt_transcript_handle edge_stt_transcript_handle;

/**
 * The handle a C caller holds.
 */
typedef edge_stt_handle *edge_stt_h;

/**
 * What a partial callback is handed. Everything in it is borrowed for
 * the length of the call.
 */
typedef struct {
    /**
     * Counts from zero, one per partial, so a gap is detectable.
     */
    uint32_t seq;
    /**
     * One when this replaces what came before, zero when it extends.
     */
    int32_t replaces;
    /**
     * The new words. Never the whole transcript unless replaces is one.
     */
    const char *text;
    /**
     * Where these words start, in milliseconds from the beginning.
     */
    uint64_t start_ms;
    /**
     * Where they end, in milliseconds from the beginning.
     */
    uint64_t end_ms;
} edge_stt_partial;

/**
 * Called on the thread that called edge_stt_transcribe, never after
 * that call has returned.
 */
typedef void (*edge_stt_partial_cb)(const edge_stt_partial *partial, void *user);

/**
 * The handle a C caller holds for one transcript.
 */
typedef edge_stt_transcript_handle *edge_stt_transcript_h;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * @brief The message behind the last failing call on this thread.
 *
 * @return The message, borrowed until the next call on this thread
 *         fails. Empty when nothing has failed yet.
 */
const char *edge_stt_get_last_error(void);

/**
 * @brief Make a handle. Load a model into it before transcribing.
 *
 * @return The handle. Making one cannot fail.
 * @see edge_stt_load_model, edge_stt_free
 */
edge_stt_h edge_stt_new(void);

/**
 * @brief Release the handle and everything it owns.
 *
 * @param[in] stt the handle, or NULL, which does nothing
 * @see edge_stt_new
 */
void edge_stt_free(edge_stt_h stt);

/**
 * @brief Set the language before loading a model, or leave it unset
 *        to let the model decide.
 *
 * @param[in] stt the handle
 * @param[in] language a tag such as "ko", or NULL to detect
 * @return #EDGE_STT_OK, or a negative #edge_stt_error.
 */
int32_t edge_stt_set_language(edge_stt_h stt, const char *language);

/**
 * @brief Give up on a transcription that takes longer than this.
 *
 * @param[in] stt the handle
 * @param[in] milliseconds the limit, or zero for none
 * @return #EDGE_STT_OK, or a negative #edge_stt_error.
 */
int32_t edge_stt_set_timeout(edge_stt_h stt, uint64_t milliseconds);

/**
 * @brief Load a model, which is when a missing or unusable file is
 *        found out rather than on the first spoken word.
 *
 * @param[in] stt the handle
 * @param[in] path the ggml file. You supply it; nothing is downloaded
 * @return #EDGE_STT_OK, or a negative #edge_stt_error.
 *         edge_stt_get_last_error() says which path was searched.
 * @see edge_stt_new
 */
int32_t edge_stt_load_model(edge_stt_h stt, const char *path);

/**
 * @brief Ask to be told about words as they are decoded.
 *
 * @param[in] stt the handle
 * @param[in] callback called on the transcribing thread, never after
 *            edge_stt_transcribe returns. NULL turns partials off
 * @param[in] user handed back to the callback untouched
 * @return #EDGE_STT_OK, or a negative #edge_stt_error.
 * @see edge_stt_transcribe
 */
int32_t edge_stt_on_partial(edge_stt_h stt, edge_stt_partial_cb callback, void *user);

/**
 * @brief Turn a recording into text.
 *
 * @param[in] stt the handle, with a model loaded
 * @param[in] samples 16000 Hz mono 16-bit samples. Borrowed for the
 *            call
 * @param[in] count how many samples
 * @param[in] sample_rate must be 16000; anything else is refused
 * @param[out] out where the transcript is put. Free it with
 *             edge_stt_transcript_free
 * @return #EDGE_STT_OK, or a negative #edge_stt_error.
 * @see edge_stt_transcript_free, edge_stt_cancel
 */
int32_t edge_stt_transcribe(edge_stt_h stt,
                            const int16_t *samples,
                            uintptr_t count,
                            uint32_t sample_rate,
                            edge_stt_transcript_h *out);

/**
 * @brief Stop a transcription that is running, from any thread.
 *
 * @param[in] stt the handle
 * @return #EDGE_STT_OK, or a negative #edge_stt_error. The
 *         transcribing call returns #EDGE_STT_CANCELLED.
 */
int32_t edge_stt_cancel(edge_stt_h stt);

/**
 * @brief The words that were said.
 *
 * @param[in] transcript the transcript
 * @return The text, owned by the transcript, or NULL.
 */
const char *edge_stt_transcript_get_text(edge_stt_transcript_h transcript);

/**
 * @brief The language the model settled on.
 *
 * @param[in] transcript the transcript
 * @return The tag, owned by the transcript, or NULL.
 */
const char *edge_stt_transcript_get_language(edge_stt_transcript_h transcript);

/**
 * @brief How long the audio ran, in milliseconds.
 *
 * @param[in] transcript the transcript
 * @return The duration, or zero.
 */
uint64_t edge_stt_transcript_get_audio_ms(edge_stt_transcript_h transcript);

/**
 * @brief How long transcribing took, in milliseconds. With the audio
 *        duration, this is whether the hardware is keeping up.
 *
 * @param[in] transcript the transcript
 * @return The time taken, or zero.
 */
uint64_t edge_stt_transcript_get_processing_ms(edge_stt_transcript_h transcript);

/**
 * @brief How sure the model is, between zero and one.
 *
 * @param[in] transcript the transcript
 * @return The confidence, or zero.
 */
float edge_stt_transcript_get_confidence(edge_stt_transcript_h transcript);

/**
 * @brief How many timed segments the transcript holds.
 *
 * @param[in] transcript the transcript
 * @return The count, or zero.
 */
uintptr_t edge_stt_transcript_get_segment_count(edge_stt_transcript_h transcript);

/**
 * @brief One segment's words.
 *
 * @param[in] transcript the transcript
 * @param[in] index below edge_stt_transcript_get_segment_count
 * @return The text, owned by the transcript, or NULL when out of range.
 */
const char *edge_stt_transcript_get_segment_text(edge_stt_transcript_h transcript, uintptr_t index);

/**
 * @brief Where one segment starts, in milliseconds from the start.
 *
 * @param[in] transcript the transcript
 * @param[in] index below edge_stt_transcript_get_segment_count
 * @return The offset, or zero when out of range.
 */
uint64_t edge_stt_transcript_get_segment_start_ms(edge_stt_transcript_h transcript,
                                                  uintptr_t index);

/**
 * @brief Where one segment ends, in milliseconds from the start.
 *
 * @param[in] transcript the transcript
 * @param[in] index below edge_stt_transcript_get_segment_count
 * @return The offset, or zero when out of range.
 */
uint64_t edge_stt_transcript_get_segment_end_ms(edge_stt_transcript_h transcript, uintptr_t index);

/**
 * @brief How sure the model is about one segment.
 *
 * @param[in] transcript the transcript
 * @param[in] index below edge_stt_transcript_get_segment_count
 * @return Between zero and one, or zero when out of range.
 */
float edge_stt_transcript_get_segment_confidence(edge_stt_transcript_h transcript, uintptr_t index);

/**
 * @brief Release a transcript.
 *
 * @param[in] transcript the transcript, or NULL, which does nothing
 */
void edge_stt_transcript_free(edge_stt_transcript_h transcript);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* EDGE_STT_H */
