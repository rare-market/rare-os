// Pad tester for the Nintendo 3DS (Azahar, Lime3DS): keys, Circle Pad, C-Stick, every frame.
#include <3ds.h>
#include <stdio.h>
#define K(m, n) ((k & (m)) ? n " " : "")
int main(void) {
    gfxInitDefault();
    consoleInit(GFX_TOP, NULL);
    while (aptMainLoop()) {
        hidScanInput();
        u32 k = hidKeysHeld();
        circlePosition cp, cs;
        hidCircleRead(&cp);
        hidCstickRead(&cs);
        printf("\x1b[2;1HRARE PAD TEST 3DS\n\n");
        printf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s%s%s                 \n", K(KEY_A, "A"), K(KEY_B, "B"), K(KEY_X, "X"), K(KEY_Y, "Y"), K(KEY_L, "L"), K(KEY_R, "R"),
               K(KEY_ZL, "ZL"), K(KEY_ZR, "ZR"), K(KEY_START, "START"), K(KEY_SELECT, "SELECT"), K(KEY_DUP, "UP"), K(KEY_DDOWN, "DOWN"), K(KEY_DLEFT, "LEFT"), K(KEY_DRIGHT, "RIGHT"));
        printf("CIRCLE %4d %4d  CSTICK %4d %4d        \n", cp.dx, cp.dy, cs.dx, cs.dy);
        gfxFlushBuffers();
        gfxSwapBuffers();
        gspWaitForVBlank();
    }
    gfxExit();
    return 0;
}
