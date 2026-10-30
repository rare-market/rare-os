// Pad tester for the PSP (PPSSPP): buttons and the analog nub, every frame.
#include <pspkernel.h>
#include <pspdebug.h>
#include <pspctrl.h>
#include <pspdisplay.h>
PSP_MODULE_INFO("RarePadTest", 0, 1, 0);
#define K(m, n) ((b & (m)) ? n " " : "")
int main(void) {
    pspDebugScreenInit();
    sceCtrlSetSamplingCycle(0);
    sceCtrlSetSamplingMode(PSP_CTRL_MODE_ANALOG);
    SceCtrlData pad;
    while (1) {
        sceCtrlReadBufferPositive(&pad, 1);
        unsigned b = pad.Buttons;
        pspDebugScreenSetXY(0, 2);
        pspDebugScreenPrintf("RARE PAD TEST PSP\n\n");
        pspDebugScreenPrintf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s                  \n", K(PSP_CTRL_CROSS, "CROSS"), K(PSP_CTRL_CIRCLE, "CIRCLE"), K(PSP_CTRL_SQUARE, "SQUARE"), K(PSP_CTRL_TRIANGLE, "TRIANGLE"),
                             K(PSP_CTRL_LTRIGGER, "L"), K(PSP_CTRL_RTRIGGER, "R"), K(PSP_CTRL_START, "START"), K(PSP_CTRL_SELECT, "SELECT"), K(PSP_CTRL_UP, "UP"), K(PSP_CTRL_DOWN, "DOWN"),
                             K(PSP_CTRL_LEFT, "LEFT"), K(PSP_CTRL_RIGHT, "RIGHT"));
        pspDebugScreenPrintf("NUB %3d %3d        \n", pad.Lx, pad.Ly);
        sceDisplayWaitVblankStart();
    }
    return 0;
}
