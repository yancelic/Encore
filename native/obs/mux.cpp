// SPDX-License-Identifier: GPL-3.0-only
#include "encore_capture.h"
extern "C" {
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/mem.h>
}
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
void require_av(int result, const char *operation) {
    if (result >= 0) return;
    char message[AV_ERROR_MAX_STRING_SIZE]{};
    av_strerror(result, message, sizeof(message));
    throw std::runtime_error(std::string(operation) + ": " + message);
}
struct FormatCloser {
    void operator()(AVFormatContext *context) const {
        if (context->pb) avio_closep(&context->pb);
        avformat_free_context(context);
    }
};
struct PacketCloser { void operator()(AVPacket *packet) const { av_packet_free(&packet); } };
}

extern "C" int ec_mux(const char *path, const char *game, const ec_format *formats,
                      const ec_packet *packets, size_t count, char *error, size_t error_size) {
    try {
        if (!path || !formats || !packets || !count) throw std::runtime_error("Empty replay snapshot");
        AVFormatContext *raw = nullptr;
        require_av(avformat_alloc_output_context2(&raw, nullptr, "mp4", path), "Create MP4 muxer");
        if (!raw) throw std::runtime_error("MP4 muxer unavailable");
        std::unique_ptr<AVFormatContext, FormatCloser> context(raw);
        const char *names[] = {"Video", "Game", "Chat", "Microphone"};
        for (unsigned i = 0; i < 4; ++i) {
            const auto &input = formats[i];
            if (!input.extra_data || !input.extra_data_size || input.extra_data_size > 1024 * 1024)
                throw std::runtime_error("Missing or invalid encoder configuration");
            AVStream *stream = avformat_new_stream(raw, nullptr);
            if (!stream) throw std::bad_alloc();
            stream->time_base = AVRational{1, 10000000};
            AVCodecParameters *codec = stream->codecpar;
            codec->codec_type = i == 0 ? AVMEDIA_TYPE_VIDEO : AVMEDIA_TYPE_AUDIO;
            codec->codec_id = i == 0 ? AV_CODEC_ID_H264 : AV_CODEC_ID_AAC;
            if (i == 0) { codec->width = input.width; codec->height = input.height; }
            else {
                if (!input.sample_rate || !input.channels) throw std::runtime_error("Invalid audio format");
                codec->sample_rate = input.sample_rate;
                av_channel_layout_default(&codec->ch_layout, input.channels);
            }
            codec->extradata = static_cast<uint8_t *>(av_mallocz(input.extra_data_size + AV_INPUT_BUFFER_PADDING_SIZE));
            if (!codec->extradata) throw std::bad_alloc();
            codec->extradata_size = static_cast<int>(input.extra_data_size);
            std::memcpy(codec->extradata, input.extra_data, input.extra_data_size);
            av_dict_set(&stream->metadata, "title", names[i], 0);
            av_dict_set(&stream->metadata, "handler_name", names[i], 0);
        }
        if (game && *game) {
            const std::string comment = std::string("encore_game:") + game;
            av_dict_set(&raw->metadata, "comment", comment.c_str(), 0);
        }
        require_av(avio_open(&raw->pb, path, AVIO_FLAG_WRITE), "Open clip file");
        require_av(avformat_write_header(raw, nullptr), "Write MP4 header");
        int64_t base = std::numeric_limits<int64_t>::max();
        std::vector<const ec_packet *> ordered;
        ordered.reserve(count);
        for (size_t i = 0; i < count; ++i) {
            const auto &packet = packets[i];
            if (packet.stream >= 4 || !packet.data || !packet.size || packet.size > 32 * 1024 * 1024 || packet.duration <= 0)
                throw std::runtime_error("Invalid replay packet");
            base = std::min(base, std::min(packet.pts, packet.dts));
            ordered.push_back(&packet);
        }
        // Stable ordering preserves each encoder's decode order, including B-frames.
        std::stable_sort(ordered.begin(), ordered.end(), [](auto a, auto b) { return a->dts < b->dts; });
        std::unique_ptr<AVPacket, PacketCloser> packet(av_packet_alloc());
        if (!packet) throw std::bad_alloc();
        for (auto input : ordered) {
            require_av(av_new_packet(packet.get(), static_cast<int>(input->size)), "Allocate MP4 packet");
            std::memcpy(packet->data, input->data, input->size);
            packet->stream_index = input->stream;
            packet->pts = input->pts - base;
            packet->dts = input->dts - base;
            packet->duration = input->duration;
            if (input->keyframe) packet->flags |= AV_PKT_FLAG_KEY;
            av_packet_rescale_ts(packet.get(), AVRational{1, 10000000}, raw->streams[input->stream]->time_base);
            require_av(av_interleaved_write_frame(raw, packet.get()), "Write MP4 packet");
        }
        require_av(av_write_trailer(raw), "Finalize MP4 clip");
        return 1;
    } catch (const std::exception &exception) {
        if (error && error_size) std::snprintf(error, error_size, "%s", exception.what());
        return 0;
    }
}
