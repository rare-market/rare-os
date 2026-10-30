// Pad tester for the Nintendo DS (melonDS): the keys and the touch point, every frame.
#include <nds.h>
#include <stdio.h>
#define K(m, n) ((k & (m)) ? n " " : "")
int main(void) {
    consoleDemoInit();
    while (1) {
        scanKeys();
        u32 k = keysHeld();
        touchPosition t;
        touchRead(&t);
        iprintf("\x1b[1;0HRARE PAD TEST DS\n\n");
        iprintf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s                    \n", K(KEY_A, "A"), K(KEY_B, "B"), K(KEY_X, "X"), K(KEY_Y, "Y"), K(KEY_L, "L"), K(KEY_R, "R"),
                K(KEY_START, "START"), K(KEY_SELECT, "SELECT"), K(KEY_UP, "UP"), K(KEY_DOWN, "DOWN"), K(KEY_LEFT, "LEFT"), K(KEY_RIGHT, "RIGHT"));
        iprintf("\nTOUCH %s %3d %3d       \n", (k & KEY_TOUCH) ? "yes" : "no ", t.px, t.py);
        swiWaitForVBlank();
    }
}
