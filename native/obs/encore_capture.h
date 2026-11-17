#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* All timestamps use Encore's existing 100 ns clock. Payloads are borrowed only
 * for the duration of each call. No OBS or FFmpeg objects cross this boundary. */
typedef struct ec_packet {
    uint32_t stream;
    int64_t pts, dts, duration;
    const uint8_t *data;
    size_t size;
    uint8_t keyframe;
} ec_packet;

typedef struct ec_format {
    uint32_t width, height, sample_rate, channels;
    const uint8_t *extra_data;
    size_t extra_data_size;
} ec_format;

typedef void (*ec_packet_callback)(void *context, const ec_packet *packet);

/* Engine functions are called on the main thread. ec_mux can run on the bounded
 * save worker while capture continues, independently of the engine lifetime. */
void *ec_create(const char *runtime_root, ec_packet_callback packet, void *context);
const char *ec_request(void *engine, const char *json);
void ec_tick(void *engine);
int ec_get_format(void *engine, uint32_t stream, ec_format *format);
void ec_destroy(void *engine);
const char *ec_last_error(void);
int ec_mux(const char *path, const char *game, const ec_format *formats,
           const ec_packet *packets, size_t packet_count, char *error, size_t error_size);

#ifdef __cplusplus
}
#endif
