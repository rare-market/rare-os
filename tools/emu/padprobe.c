// SDL's view of the pads, as SDL-based emulators get it: every joystick with its index, name and
// GUID, then each game controller button and axis event as it happens (and the raw joystick
// button numbers, for emulators that bind those). Rumbles the first controller once at start.
// Build: cc -O2 -o padprobe padprobe.c $(sdl2-config --cflags --libs)
// Run:   ./padprobe [seconds]
#include <SDL.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    int secs = argc > 1 ? atoi(argv[1]) : 30;
    if (SDL_Init(SDL_INIT_GAMECONTROLLER | SDL_INIT_JOYSTICK | SDL_INIT_HAPTIC | SDL_INIT_EVENTS) != 0) {
        fprintf(stderr, "SDL_Init: %s\n", SDL_GetError());
        return 1;
    }
    SDL_version v;
    SDL_GetVersion(&v);
    printf("SDL %d.%d.%d\n", v.major, v.minor, v.patch);
    SDL_PumpEvents();
    SDL_Delay(300);
    SDL_PumpEvents();
    int n = SDL_NumJoysticks();
    printf("joysticks %d\n", n);
    SDL_GameController *first = NULL;
    for (int i = 0; i < n; i++) {
        char g[64];
        SDL_JoystickGetGUIDString(SDL_JoystickGetDeviceGUID(i), g, sizeof g);
        printf("  %d %s guid=%s controller=%d\n", i, SDL_JoystickNameForIndex(i), g, SDL_IsGameController(i));
        if (SDL_IsGameController(i)) {
            SDL_GameController *c = SDL_GameControllerOpen(i);
            if (c) {
                char *m = SDL_GameControllerMapping(c);
                printf("     mapping %s\n", m ? m : "-");
                SDL_free(m);
                if (!first) first = c;
            }
        } else {
            SDL_JoystickOpen(i);
        }
    }
    if (first) printf("rumble %d\n", SDL_GameControllerRumble(first, 0x8000, 0x8000, 300));
    fflush(stdout);
    Uint32 end = SDL_GetTicks() + secs * 1000;
    SDL_Event ev;
    while (SDL_GetTicks() < end) {
        while (SDL_PollEvent(&ev)) {
            switch (ev.type) {
            case SDL_CONTROLLERBUTTONDOWN:
                printf("dev %d button %s down\n", ev.cbutton.which, SDL_GameControllerGetStringForButton(ev.cbutton.button));
                break;
            case SDL_CONTROLLERAXISMOTION:
                if (ev.caxis.value > 16000 || ev.caxis.value < -16000)
                    printf("dev %d axis %s %d\n", ev.caxis.which, SDL_GameControllerGetStringForAxis(ev.caxis.axis), ev.caxis.value);
                break;
            case SDL_JOYBUTTONDOWN:
                printf("dev %d joybutton %d down\n", ev.jbutton.which, ev.jbutton.button);
                break;
            case SDL_JOYHATMOTION:
                if (ev.jhat.value) printf("dev %d hat %d value %d\n", ev.jhat.which, ev.jhat.hat, ev.jhat.value);
                break;
            case SDL_JOYAXISMOTION:
                if (ev.jaxis.value > 16000 || ev.jaxis.value < -16000)
                    printf("dev %d joyaxis %d %d\n", ev.jaxis.which, ev.jaxis.axis, ev.jaxis.value);
                break;
            }
            fflush(stdout);
        }
        SDL_Delay(5);
    }
    return 0;
}
