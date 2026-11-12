// SPDX-License-Identifier: GPL-3.0-only
#include "encore_capture.h"
#include "platform.hpp"
#include <obs.hpp>
#include <obs-hotkey.h>
#include <obs-module.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

namespace fs = std::filesystem;
void encore_register_diagnostic_sources();
namespace {
thread_local std::string last_error;
struct Engine;
struct OutputContext { Engine *engine; obs_output_t *output; };

struct Engine {
    ec_packet_callback callback;
    void *context;
    std::string runtime;
    std::string reply;
    std::string error;
    std::string warnings[3];
    std::string encoder_name;
    encore::Application target;
    obs_data_t *config = nullptr;
    obs_source_t *video = nullptr;
    obs_source_t *audio[3]{};
    obs_scene_t *scene = nullptr;
    obs_output_t *output = nullptr;
    obs_encoder_t *encoders[4]{};
    obs_hotkey_id hotkey = OBS_INVALID_HOTKEY_ID;
    std::atomic<bool> save_requested{false};
    std::atomic<bool> encoder_failed{false};
    bool configured = false;
    bool core_started = false;
    uint64_t epoch = 0;
    uint32_t width = 0, height = 0;
    uint32_t fps = 60;
    std::chrono::steady_clock::time_point next_poll{};

    ~Engine() {
        clear_capture();
        if (hotkey != OBS_INVALID_HOTKEY_ID) obs_hotkey_unregister(hotkey);
        if (config) obs_data_release(config);
        if (core_started) obs_shutdown();
        encore::platform_shutdown();
    }

    void stop_output() {
        if (output) {
            obs_output_force_stop(output);
            obs_output_release(output);
            output = nullptr;
        }
        for (auto &encoder : encoders) {
            if (encoder) obs_encoder_release(encoder);
            encoder = nullptr;
        }
        width = height = 0;
        ++epoch;
        if (callback) {
            ec_packet reset{}; reset.stream = UINT32_MAX;
            callback(context, &reset);
        }
    }

    void clear_capture() {
        stop_output();
        if (!core_started) return;
        if (video && encore::platform_is_wayland() && config) {
            const std::string directory = obs_data_get_string(config, "state_dir");
            if (!directory.empty()) {
                obs_source_save(video);
                OBSDataAutoRelease settings = obs_source_get_settings(video);
                obs_data_save_json_safe(settings, (fs::path(directory) / "capture-source.json").c_str(), "tmp", nullptr);
            }
        }
        for (unsigned i = 0; i < 4; ++i) obs_set_output_source(i, nullptr);
        if (scene) { obs_scene_release(scene); scene = nullptr; }
        if (video) { obs_source_release(video); video = nullptr; }
        for (auto &source : audio) {
            if (source) obs_source_release(source);
            source = nullptr;
        }
        target = {};
    }

    void reset_video(uint32_t source_width, uint32_t source_height) {
        uint32_t out_height = static_cast<uint32_t>(obs_data_get_int(config, "height"));
        out_height = out_height == 0 ? source_height : std::min(out_height, source_height);
        out_height = std::max(2u, out_height & ~1u);
        uint32_t out_width = std::max(2u, static_cast<uint32_t>(
            std::llround(static_cast<double>(source_width) * out_height / source_height)) & ~1u);
        obs_video_info info{};
        std::string graphics = encore::platform_graphics_module();
        if (!runtime.empty()) {
            auto candidate = fs::path(runtime) / "lib" / graphics;
            if (fs::exists(candidate)) graphics = candidate.string();
        }
        info.graphics_module = graphics.c_str();
        info.fps_num = fps; info.fps_den = 1;
        info.base_width = source_width; info.base_height = source_height;
        info.output_width = out_width; info.output_height = out_height;
        info.output_format = VIDEO_FORMAT_NV12;
        info.gpu_conversion = true;
        info.colorspace = VIDEO_CS_709;
        info.range = VIDEO_RANGE_PARTIAL;
        info.scale_type = OBS_SCALE_BICUBIC;
        const int result = obs_reset_video(&info);
        if (result != OBS_VIDEO_SUCCESS) throw std::runtime_error("Could not initialize native GPU capture (code " + std::to_string(result) + ")");
    }

    std::vector<std::string> video_encoders() {
        std::vector<std::string> hardware, software;
        const char *id = nullptr;
        for (size_t i = 0; obs_enum_encoder_types(i, &id); ++i) {
            const char *codec = obs_get_encoder_codec(id);
            if (!codec || std::string(codec) != "h264") continue;
            std::string name(id);
            if (name.find("software") != std::string::npos || name == "obs_x264" || name.find("videotoolbox.videoencoder.ave.avc") != std::string::npos)
                software.push_back(name);
            else hardware.push_back(name);
        }
        hardware.insert(hardware.end(), software.begin(), software.end());
        return hardware;
    }

    void start_output() {
        const auto sw = obs_source_get_width(video), sh = obs_source_get_height(video);
        if (!sw || !sh) return; // Portal permission or source selection is still pending.
        stop_output();
        reset_video(sw & ~1u, sh & ~1u);
        OBSDataAutoRelease settings = obs_data_create();
        obs_data_set_int(settings, "bitrate", std::clamp<int64_t>(
            static_cast<int64_t>(sw) * sh * fps * 20000 / (1920LL * 1080 * 60), 8000, 80000));
        obs_data_set_string(settings, "rate_control", "CBR");
        obs_data_set_string(settings, "preset", "veryfast");
        obs_data_set_int(settings, "keyint_sec", 1);
        obs_data_set_bool(settings, "bframes", false);
        OBSDataAutoRelease output_settings = obs_data_create();
        obs_data_set_int(output_settings, "engine", reinterpret_cast<intptr_t>(this));
        output = obs_output_create("encore_packets", "Encore RAM replay", output_settings, nullptr);
        if (!output) throw std::runtime_error("Could not create the replay output");
        for (unsigned i = 0; i < 3; ++i) {
            OBSDataAutoRelease aac = obs_data_create();
            obs_data_set_int(aac, "bitrate", 192);
            encoders[i + 1] = obs_audio_encoder_create("ffmpeg_aac", ("Encore audio " + std::to_string(i)).c_str(), aac, i, nullptr);
            if (!encoders[i + 1]) throw std::runtime_error("AAC encoder unavailable; check the OBS runtime");
            obs_encoder_set_audio(encoders[i + 1], obs_get_audio());
            obs_output_set_audio_encoder(output, encoders[i + 1], i);
        }
        for (const auto &id : video_encoders()) {
            auto encoder = obs_video_encoder_create(id.c_str(), "Encore H.264", settings, nullptr);
            if (!encoder) continue;
            obs_encoder_set_video(encoder, obs_get_video());
            obs_output_set_video_encoder(output, encoder);
            if (obs_output_start(output)) {
                encoders[0] = encoder;
                encoder_name = id;
                width = sw; height = sh;
                encoder_failed.store(false);
                error.clear();
                return;
            }
            obs_output_set_video_encoder(output, nullptr);
            obs_encoder_release(encoder);
        }
        throw std::runtime_error("No working H.264 encoder was found; check GPU drivers and OBS runtime plugins");
    }

    void configure_hotkey() {
        if (hotkey == OBS_INVALID_HOTKEY_ID) hotkey = obs_hotkey_register_frontend(
            "Encore.Save", "Save Encore replay", [](void *data, obs_hotkey_id, obs_hotkey_t *, bool pressed) {
                if (pressed) static_cast<Engine *>(data)->save_requested.store(true);
            }, this);
        std::string text = obs_data_get_string(config, "hotkey");
        obs_key_combination_t combination{};
        size_t offset = 0;
        while (true) {
            auto end = text.find('+', offset);
            const auto part = text.substr(offset, end == std::string::npos ? end : end - offset);
            if (part == "Ctrl" || part == "Control") combination.modifiers |= INTERACT_CONTROL_KEY;
            else if (part == "Alt") combination.modifiers |= INTERACT_ALT_KEY;
            else if (part == "Shift") combination.modifiers |= INTERACT_SHIFT_KEY;
            else if (part == "Super" || part == "Command" || part == "Meta") combination.modifiers |= INTERACT_COMMAND_KEY;
            else combination.key = obs_key_from_name(("OBS_KEY_" + part).c_str());
            if (end == std::string::npos) break;
            offset = end + 1;
        }
        if (combination.key == OBS_KEY_NONE) throw std::runtime_error("This save hotkey is not supported by the native backend");
        obs_hotkey_load_bindings(hotkey, &combination, 1);
        obs_hotkey_enable_background_press(true);
    }

    void configure(obs_data_t *settings) {
        clear_capture();
        if (config) obs_data_release(config);
        config = obs_data_create(); obs_data_apply(config, settings);
        fps = static_cast<uint32_t>(std::clamp<int64_t>(obs_data_get_int(config, "fps"), 1, 240));
        reset_video(1920, 1080);
        configure_hotkey();
        configured = true;
        error.clear();
        next_poll = {};
    }

    void create_audio(unsigned track, const std::string &selection, bool app) {
        if (selection.empty() || selection == "off") return;
        OBSDataAutoRelease settings = obs_data_create();
        encore::platform_audio_settings(settings, selection, app);
        const char *id = encore::platform_audio_source(track == 2, app);
        audio[track] = obs_source_create(id, ("Encore " + std::to_string(track)).c_str(), settings, nullptr);
        if (!audio[track]) { warnings[track] = "Could not open selected audio source"; return; }
        obs_source_set_audio_mixers(audio[track], 1u << track);
        obs_set_output_source(track + 1, audio[track]);
        warnings[track].clear();
    }

    void create_capture(const encore::Application &application) {
        clear_capture();
        target = application;
        OBSDataAutoRelease settings = obs_data_create();
        if (encore::platform_is_wayland()) {
            const auto file = fs::path(obs_data_get_string(config, "state_dir")) / "capture-source.json";
            OBSDataAutoRelease saved = obs_data_create_from_json_file(file.c_str());
            if (saved) obs_data_apply(settings, saved);
        }
        const bool diagnostic = obs_data_get_bool(config, "diagnostic_source");
        if (!diagnostic) encore::platform_video_settings(settings, target);
        video = obs_source_create(diagnostic ? "encore_test_video" : encore::platform_video_source(), "Encore selected window", settings, nullptr);
        if (!video) throw std::runtime_error("Could not capture the selected window. Check Screen Recording permissions or the desktop portal.");
        obs_source_set_audio_mixers(video, 0);
        scene = obs_scene_create_private("Encore scene");
        if (!scene || !obs_scene_add(scene, video)) throw std::runtime_error("Could not create the capture scene");
        obs_set_output_source(0, obs_scene_get_source(scene));
        if (diagnostic) {
            for (unsigned i = 0; i < 3; ++i) {
                OBSDataAutoRelease tone = obs_data_create();
                obs_data_set_int(tone, "frequency", 440 + 220 * i);
                audio[i] = obs_source_create("encore_test_tone", ("Diagnostic tone " + std::to_string(i)).c_str(), tone, nullptr);
                if (!audio[i]) throw std::runtime_error("Could not create diagnostic audio");
                obs_source_set_audio_mixers(audio[i], 1u << i);
                obs_set_output_source(i + 1, audio[i]);
            }
            return;
        }
        const bool app_audio = std::string(obs_data_get_string(config, "audio_mode")) == "Applications";
        if (app_audio) {
            if (obs_data_get_bool(config, "game_audio_enabled")) create_audio(0, target.id, true);
            create_audio(1, obs_data_get_string(config, "chat_application"), true);
        } else {
            std::string game = obs_data_get_string(config, "audio_device");
            create_audio(0, game.empty() ? "default" : game, false);
            create_audio(1, obs_data_get_string(config, "chat_device"), false);
        }
        create_audio(2, obs_data_get_string(config, "mic_device"), false);
    }

    void tick() {
        encore::platform_pump();
        if (!configured) return;
        if (encoder_failed.exchange(false)) { stop_output(); error = "The encoder stopped. Check GPU drivers and restart recording."; }
        const auto now = std::chrono::steady_clock::now();
        if (now < next_poll) return;
        next_poll = now + std::chrono::milliseconds(250);
        if (obs_data_get_bool(config, "diagnostic_source")) {
            if (!video) create_capture({"diagnostic", "Diagnostic source"});
        } else if (encore::platform_is_wayland()) {
            if (std::string(obs_data_get_string(config, "mode")) == "ActiveWindow")
                throw std::runtime_error("Wayland requires selecting a window through the system sharing dialog; following arbitrary focused windows is unavailable.");
            if (!video) create_capture({"selected-window", "Selected window"});
        } else {
            const auto applications = encore::platform_applications();
            const bool follow_focus = std::string(obs_data_get_string(config, "mode")) == "ActiveWindow";
            OBSDataArrayAutoRelease games = obs_data_get_array(config, "games");
            auto is_game = [&](const std::string &id) {
                for (size_t i = 0; games && i < obs_data_array_count(games); ++i) {
                    OBSDataAutoRelease item = obs_data_array_item(games, i);
                    if (id == obs_data_get_string(item, "value")) return true;
                }
                return false;
            };
            // Config.games is a JSON string array; obs_data arrays contain objects.
            // The Rust bridge supplies game_targets as [{value: application_id}].
            games = obs_data_get_array(config, "game_targets");
            const encore::Application *chosen = nullptr;
            for (const auto &application : applications) {
                if (follow_focus ? !application.focused : !is_game(application.id)) continue;
                if (!chosen || application.window == target.window || application.focused) chosen = &application;
                if (!follow_focus && application.window == target.window) break;
            }
            if (!chosen || chosen->minimized) {
                if (video) clear_capture();
                return;
            }
            if (!video || target.window != chosen->window || target.id != chosen->id) create_capture(*chosen);
        }
        if (!video) return;
        const auto sw = obs_source_get_width(video), sh = obs_source_get_height(video);
        if (!sw || !sh) { if (output) stop_output(); return; }
        if (!output || width != sw || height != sh) start_output();
    }

    obs_data_t *status() {
        auto *data = obs_data_create();
        obs_data_set_bool(data, "recording", output && obs_output_active(output));
        obs_data_set_bool(data, "save_requested", save_requested.exchange(false));
        obs_data_set_int(data, "epoch", static_cast<long long>(epoch));
        obs_data_set_string(data, "target", target.id.c_str());
        obs_data_set_string(data, "error", error.c_str());
        obs_data_set_string(data, "encoder", encoder_name.c_str());
        for (unsigned i = 0; i < 3; ++i) obs_data_set_string(data, ("audio_warning_" + std::to_string(i)).c_str(), warnings[i].c_str());
        return data;
    }
};

const char *output_name(void *) { return "Encore bounded RAM packets"; }
void *output_create(obs_data_t *settings, obs_output_t *output) {
    return new OutputContext{reinterpret_cast<Engine *>(static_cast<intptr_t>(obs_data_get_int(settings, "engine"))), output};
}
void output_destroy(void *data) { delete static_cast<OutputContext *>(data); }
bool output_start(void *data) {
    auto *output = static_cast<OutputContext *>(data)->output;
    if (!obs_output_can_begin_data_capture(output, 0) || !obs_output_initialize_encoders(output, 0)) return false;
    obs_output_begin_data_capture(output, 0);
    return true;
}
void output_stop(void *data, uint64_t) { obs_output_end_data_capture(static_cast<OutputContext *>(data)->output); }
void output_packet(void *data, encoder_packet *packet) {
    auto *engine = static_cast<OutputContext *>(data)->engine;
    if (!packet) { engine->encoder_failed.store(true); return; }
    if (!engine->callback || packet->timebase_den <= 0 || packet->timebase_num <= 0) return;
    ec_packet view{};
    view.stream = packet->type == OBS_ENCODER_VIDEO ? 0 : static_cast<uint32_t>(packet->track_idx + 1);
    const long double scale = static_cast<long double>(packet->timebase_num) * 10000000 / packet->timebase_den;
    view.pts = static_cast<int64_t>(std::llround(packet->pts * scale));
    view.dts = static_cast<int64_t>(std::llround(packet->dts * scale));
    view.duration = packet->type == OBS_ENCODER_VIDEO ? static_cast<int64_t>(10000000 / engine->fps)
        : static_cast<int64_t>(obs_encoder_get_frame_size(packet->encoder) * 10000000ULL / obs_encoder_get_sample_rate(packet->encoder));
    view.data = packet->data; view.size = packet->size; view.keyframe = packet->keyframe;
    engine->callback(engine->context, &view);
}

void load_modules(const fs::path &runtime) {
    if (!runtime.empty()) {
        obs_add_data_path((runtime / "share/obs/libobs").c_str());
        obs_add_module_path((runtime / "lib/obs-plugins/%module%.so").c_str(), (runtime / "share/obs/obs-plugins/%module%").c_str());
        const auto plugins = runtime / "Contents/PlugIns";
        if (fs::is_directory(plugins)) {
            obs_add_data_path((runtime / "Contents/Frameworks/libobs.framework/Resources").c_str());
            for (const auto &entry : fs::directory_iterator(plugins)) {
                if (entry.path().extension() != ".plugin") continue;
                const auto name = entry.path().stem();
                obs_module_t *module = nullptr;
                if (obs_open_module(&module, (entry.path() / "Contents/MacOS" / name).c_str(), (entry.path() / "Contents/Resources").c_str()) == MODULE_SUCCESS)
                    obs_init_module(module);
            }
        }
    }
    obs_load_all_modules();
    obs_post_load_modules();
}
}

extern "C" void *ec_create(const char *runtime, ec_packet_callback packet, void *context) {
    try {
        auto engine = std::make_unique<Engine>();
        engine->runtime = runtime ? runtime : ""; engine->callback = packet; engine->context = context;
        if (!encore::platform_init(engine->error)) throw std::runtime_error(engine->error);
        if (!obs_startup("en-US", nullptr, nullptr)) throw std::runtime_error("Could not start libobs");
        engine->core_started = true;
        obs_audio_info audio{48000, SPEAKERS_STEREO};
        if (!obs_reset_audio(&audio)) throw std::runtime_error("Could not initialize 48 kHz audio");
        obs_output_info info{};
        info.id = "encore_packets"; info.get_name = output_name;
        info.flags = OBS_OUTPUT_AV | OBS_OUTPUT_ENCODED | OBS_OUTPUT_MULTI_TRACK_AUDIO;
        info.create = output_create; info.destroy = output_destroy;
        info.start = output_start; info.stop = output_stop; info.encoded_packet = output_packet;
        info.encoded_video_codecs = "h264"; info.encoded_audio_codecs = "aac";
        obs_register_output(&info);
        encore_register_diagnostic_sources();
        load_modules(engine->runtime);
        return engine.release();
    } catch (const std::exception &error) { last_error = error.what(); return nullptr; }
}

extern "C" const char *ec_request(void *handle, const char *json) {
    auto *engine = static_cast<Engine *>(handle);
    try {
        OBSDataAutoRelease request = obs_data_create_from_json(json);
        if (!request) throw std::runtime_error("Invalid engine request");
        const std::string action = obs_data_get_string(request, "action");
        if (action == "configure") {
            OBSDataAutoRelease config = obs_data_get_obj(request, "config");
            if (!config) throw std::runtime_error("Missing configuration");
            engine->configure(config);
        } else if (action == "select_source") {
            engine->clear_capture();
            engine->next_poll = {};
        } else if (action == "applications") {
            OBSDataAutoRelease result = obs_data_create();
            OBSDataArrayAutoRelease items = obs_data_array_create();
            for (const auto &application : encore::platform_applications()) {
                OBSDataAutoRelease item = obs_data_create();
                obs_data_set_string(item, "exe", application.id.c_str());
                obs_data_set_string(item, "title", application.title.c_str());
                obs_data_array_push_back(items, item);
            }
            obs_data_set_array(result, "applications", items);
            engine->reply = obs_data_get_json(result);
            return engine->reply.c_str();
        }
        OBSDataAutoRelease status = engine->status();
        engine->reply = obs_data_get_json(status);
    } catch (const std::exception &error) {
        engine->error = error.what();
        OBSDataAutoRelease status = engine->status();
        engine->reply = obs_data_get_json(status);
    }
    return engine->reply.c_str();
}

extern "C" void ec_tick(void *handle) {
    auto *engine = static_cast<Engine *>(handle);
    try { engine->tick(); } catch (const std::exception &error) { engine->error = error.what(); }
}

extern "C" int ec_get_format(void *handle, uint32_t stream, ec_format *format) {
    auto *engine = static_cast<Engine *>(handle);
    if (stream >= 4 || !format || !engine->encoders[stream]) return 0;
    auto *encoder = engine->encoders[stream];
    uint8_t *extra = nullptr; size_t size = 0;
    if (!obs_encoder_get_extra_data(encoder, &extra, &size) || !extra || !size) return 0;
    *format = {};
    format->extra_data = extra; format->extra_data_size = size;
    if (stream == 0) { format->width = obs_encoder_get_width(encoder); format->height = obs_encoder_get_height(encoder); }
    else { format->sample_rate = obs_encoder_get_sample_rate(encoder); format->channels = 2; }
    return 1;
}
extern "C" void ec_destroy(void *engine) { delete static_cast<Engine *>(engine); }
extern "C" const char *ec_last_error(void) { return last_error.c_str(); }
