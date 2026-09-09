/**
 * @file
 * @brief The C twin of the live caption example: words on screen while
 *        the speaker is still talking.
 *
 * Every interim carries the whole utterance so far and arrives with
 * `replaces` set, so a caller redraws the line rather than appending
 * to it. The finished utterance arrives separately and is the answer.
 *
 * cc capi/examples/live_caption.c -Icapi/include -Ltarget/debug \
 *     -ledge_stt_capi -o live_caption
 */
#include <stdio.h>
#include <stdlib.h>

#include "edge_stt.h"

/* A tenth of a second of 16 kHz audio, the shape a capture callback
 * usually hands over. */
#define CHUNK 1600

static void on_partial(const edge_stt_partial *partial, void *user) {
    (void)user;
    printf("\r\033[K  ... %s", partial->text);
    fflush(stdout);
}

static void on_transcript(edge_stt_transcript_h transcript, void *user) {
    (void)user;
    printf("\r\033[K  === %s\n", edge_stt_transcript_get_text(transcript));
    edge_stt_transcript_free(transcript);
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr,
                "usage: live_caption <model> <vad model> "
                "<raw 16k mono s16 file>\n");
        return 1;
    }

    edge_stt_h stt = edge_stt_new();
    if (!stt) {
        fprintf(stderr, "%s\n", edge_stt_get_last_error());
        return 1;
    }
    if (edge_stt_load_model(stt, argv[1]) != 0) {
        fprintf(stderr, "%s\n", edge_stt_get_last_error());
        edge_stt_free(stt);
        return 1;
    }

    FILE *audio = fopen(argv[3], "rb");
    if (!audio) {
        fprintf(stderr, "cannot open %s\n", argv[3]);
        edge_stt_free(stt);
        return 1;
    }
    fseek(audio, 0, SEEK_END);
    long bytes = ftell(audio);
    fseek(audio, 0, SEEK_SET);
    int16_t *samples = malloc((size_t)bytes);
    size_t count = fread(samples, sizeof(int16_t), (size_t)bytes / sizeof(int16_t), audio);
    fclose(audio);

    /* Zeroed first, so anything not set here takes its default. */
    edge_stt_session_opts opts = {0};
    opts.struct_size = sizeof(opts);
    opts.vad_model = argv[2];
    opts.live_interims = 1;
    edge_stt_session_h session = edge_stt_session_new(stt, &opts);
    if (!session) {
        fprintf(stderr, "%s\n", edge_stt_get_last_error());
        free(samples);
        edge_stt_free(stt);
        return 1;
    }
    edge_stt_session_set_partial_cb(session, on_partial, NULL);
    edge_stt_session_set_transcript_cb(session, on_transcript, NULL);

    int code = 0;
    for (size_t at = 0; at < count; at += CHUNK) {
        size_t left = count - at;
        size_t take = left < CHUNK ? left : CHUNK;
        code = edge_stt_session_push(session, samples + at, take);
        if (code != 0) {
            fprintf(stderr, "\n%s\n", edge_stt_get_last_error());
            break;
        }
    }
    if (code == 0) {
        code = edge_stt_session_close(session);
        if (code != 0) {
            fprintf(stderr, "\n%s\n", edge_stt_get_last_error());
        }
    }

    edge_stt_session_free(session);
    free(samples);
    edge_stt_free(stt);
    return code == 0 ? 0 : 1;
}
