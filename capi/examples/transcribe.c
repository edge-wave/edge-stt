/**
 * @file
 * @brief The C twin of the Rust example: a model, a recording, a line
 *        of text.
 *
 * cc capi/examples/transcribe.c -Icapi/include -Ltarget/debug \
 *     -ledge_stt_capi -o transcribe
 */
#include <stdio.h>
#include <stdlib.h>

#include "edge_stt.h"

static void on_partial(const edge_stt_partial *partial, void *user) {
    (void)user;
    printf("%s", partial->text);
    fflush(stdout);
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: transcribe <model> <raw 16k mono s16 file>\n");
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

    FILE *audio = fopen(argv[2], "rb");
    if (!audio) {
        fprintf(stderr, "cannot open %s\n", argv[2]);
        edge_stt_free(stt);
        return 1;
    }
    fseek(audio, 0, SEEK_END);
    long bytes = ftell(audio);
    fseek(audio, 0, SEEK_SET);
    int16_t *samples = malloc((size_t)bytes);
    size_t count = fread(samples, sizeof(int16_t), (size_t)bytes / sizeof(int16_t), audio);
    fclose(audio);

    edge_stt_set_partial_cb(stt, on_partial, NULL);

    edge_stt_transcript_h out = NULL;
    int code = edge_stt_transcribe(stt, samples, count, 16000, &out);
    if (code != 0) {
        fprintf(stderr, "\n%s\n", edge_stt_get_last_error());
    } else {
        printf("\n%s\n", edge_stt_transcript_get_text(out));
        printf("%llu ms of audio in %llu ms\n",
               (unsigned long long)edge_stt_transcript_get_audio_ms(out),
               (unsigned long long)edge_stt_transcript_get_processing_ms(out));
        edge_stt_transcript_free(out);
    }

    free(samples);
    edge_stt_free(stt);
    return code == 0 ? 0 : 1;
}
