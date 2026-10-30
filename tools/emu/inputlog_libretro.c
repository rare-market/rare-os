// A libretro core that only logs what RetroArch gives it on port 1: every RetroPad button, the
// analog sticks and the analog L2/R2, one line per change, to $RARE_INPUT_LOG (or stderr).
// A press of the RetroPad A button also asks for rumble, so RetroArch's rumble path is proven.
// This is how every RetroArch core sees the pad, so it proves the autoconfig for all of them.
// Build: cc -O2 -shared -fPIC -o inputlog_libretro.so inputlog_libretro.c
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define RETRO_API_VERSION 1
#define RETRO_DEVICE_JOYPAD 1
#define RETRO_DEVICE_ANALOG 5
#define RETRO_DEVICE_INDEX_ANALOG_LEFT 0
#define RETRO_DEVICE_INDEX_ANALOG_RIGHT 1
#define RETRO_DEVICE_INDEX_ANALOG_BUTTON 2
#define RETRO_DEVICE_ID_ANALOG_X 0
#define RETRO_DEVICE_ID_ANALOG_Y 1
#define RETRO_ENVIRONMENT_SET_PIXEL_FORMAT 10
#define RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME 18
#define RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE 23
#define RETRO_PIXEL_FORMAT_XRGB8888 1

struct retro_system_info { const char *library_name, *library_version, *valid_extensions; bool need_fullpath, block_extract; };
struct retro_game_geometry { unsigned base_width, base_height, max_width, max_height; float aspect_ratio; };
struct retro_system_timing { double fps, sample_rate; };
struct retro_system_av_info { struct retro_game_geometry geometry; struct retro_system_timing timing; };
struct retro_game_info { const char *path; const void *data; size_t size; const char *meta; };
enum retro_rumble_effect { RETRO_RUMBLE_STRONG = 0, RETRO_RUMBLE_WEAK = 1 };
typedef bool (*retro_set_rumble_state_t)(unsigned port, enum retro_rumble_effect effect, uint16_t strength);
struct retro_rumble_interface { retro_set_rumble_state_t set_rumble_state; };

typedef bool (*env_t)(unsigned, void *);
typedef void (*video_t)(const void *, unsigned, unsigned, size_t);
typedef void (*poll_t)(void);
typedef int16_t (*state_t)(unsigned, unsigned, unsigned, unsigned);

static env_t env;
static video_t video;
static poll_t poll_cb;
static state_t state;
static FILE *out;
static struct retro_rumble_interface rumble;
static uint32_t fb[320 * 240];
static int16_t last[16 + 6];

static const char *names[16] = {"B", "Y", "SELECT", "START", "UP", "DOWN", "LEFT", "RIGHT", "A", "X", "L", "R", "L2", "R2", "L3", "R3"};

static void logf_(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vfprintf(out ? out : stderr, fmt, ap);
    va_end(ap);
    fflush(out ? out : stderr);
}

unsigned retro_api_version(void) { return RETRO_API_VERSION; }
void retro_set_environment(env_t cb) {
    env = cb;
    bool yes = true;
    env(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &yes);
}
void retro_set_video_refresh(video_t cb) { video = cb; }
void retro_set_audio_sample(void *cb) { (void)cb; }
void retro_set_audio_sample_batch(void *cb) { (void)cb; }
void retro_set_input_poll(poll_t cb) { poll_cb = cb; }
void retro_set_input_state(state_t cb) { state = cb; }
void retro_init(void) {
    const char *p = getenv("RARE_INPUT_LOG");
    out = p ? fopen(p, "a") : NULL;
    memset(last, 0, sizeof last);
}
void retro_deinit(void) {
    if (out) fclose(out);
}
void retro_get_system_info(struct retro_system_info *i) {
    memset(i, 0, sizeof *i);
    i->library_name = "Rare input log";
    i->library_version = "1";
    i->valid_extensions = "";
}
void retro_get_system_av_info(struct retro_system_av_info *i) {
    memset(i, 0, sizeof *i);
    i->geometry.base_width = i->geometry.max_width = 320;
    i->geometry.base_height = i->geometry.max_height = 240;
    i->geometry.aspect_ratio = 4.0f / 3.0f;
    i->timing.fps = 60.0;
    i->timing.sample_rate = 48000.0;
}
void retro_set_controller_port_device(unsigned p, unsigned d) { (void)p; (void)d; }
void retro_reset(void) {}
bool retro_load_game(const struct retro_game_info *g) {
    (void)g;
    unsigned fmt = RETRO_PIXEL_FORMAT_XRGB8888;
    env(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &fmt);
    if (!env(RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE, &rumble)) rumble.set_rumble_state = NULL;
    logf_("loaded rumble=%d\n", rumble.set_rumble_state != NULL);
    return true;
}
bool retro_load_game_special(unsigned t, const struct retro_game_info *g, size_t n) { (void)t; (void)g; (void)n; return false; }
void retro_unload_game(void) {}
unsigned retro_get_region(void) { return 0; }
size_t retro_serialize_size(void) { return 0; }
bool retro_serialize(void *d, size_t s) { (void)d; (void)s; return false; }
bool retro_unserialize(const void *d, size_t s) { (void)d; (void)s; return false; }
void *retro_get_memory_data(unsigned id) { (void)id; return NULL; }
size_t retro_get_memory_size(unsigned id) { (void)id; return 0; }
void retro_cheat_reset(void) {}
void retro_cheat_set(unsigned i, bool e, const char *c) { (void)i; (void)e; (void)c; }

void retro_run(void) {
    poll_cb();
    for (int b = 0; b < 16; b++) {
        int16_t v = state(0, RETRO_DEVICE_JOYPAD, 0, b) ? 1 : 0;
        if (v != last[b]) {
            logf_("%s %s\n", names[b], v ? "down" : "up");
            if (b == 8 && rumble.set_rumble_state) {
                rumble.set_rumble_state(0, RETRO_RUMBLE_STRONG, v ? 0xffff : 0);
                rumble.set_rumble_state(0, RETRO_RUMBLE_WEAK, v ? 0xffff : 0);
            }
        }
        last[b] = v;
    }
    // Sticks and analog triggers: logged when they cross half way, so a log reads as presses.
    struct { unsigned idx, id; const char *name; } ax[6] = {
        {RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_X, "LX"},
        {RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_ID_ANALOG_Y, "LY"},
        {RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_ID_ANALOG_X, "RX"},
        {RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_ID_ANALOG_Y, "RY"},
        {RETRO_DEVICE_INDEX_ANALOG_BUTTON, 12, "L2 analog"},
        {RETRO_DEVICE_INDEX_ANALOG_BUTTON, 13, "R2 analog"},
    };
    for (int a = 0; a < 6; a++) {
        int16_t raw = state(0, RETRO_DEVICE_ANALOG, ax[a].idx, ax[a].id);
        int16_t v = raw > 16000 ? 1 : raw < -16000 ? -1 : 0;
        if (v != last[16 + a]) logf_("%s %s (%d)\n", ax[a].name, v > 0 ? "+" : v < 0 ? "-" : "rest", raw);
        last[16 + a] = v;
    }
    video(fb, 320, 240, 320 * 4);
}
