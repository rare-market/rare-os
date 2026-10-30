// Pad tester for the Switch (Ryujinx, Citron): player 1 / handheld keys and both sticks.
#include <switch.h>
#include <stdio.h>
#define K(m, n) ((k & (m)) ? n " " : "")
int main(void) {
    consoleInit(NULL);
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    PadState pad;
    padInitializeDefault(&pad);
    HidVibrationDeviceHandle vh[2];
    hidInitializeVibrationDevices(vh, 2, HidNpadIdType_No1, HidNpadStyleTag_NpadFullKey);
    while (appletMainLoop()) {
        padUpdate(&pad);
        u64 k = padGetButtons(&pad);
        HidAnalogStickState l = padGetStickPos(&pad, 0), r = padGetStickPos(&pad, 1);
        printf("\x1b[2;1HRARE PAD TEST SWITCH\n\n");
        printf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s                 \n", K(HidNpadButton_A, "A"), K(HidNpadButton_B, "B"), K(HidNpadButton_X, "X"), K(HidNpadButton_Y, "Y"),
               K(HidNpadButton_L, "L"), K(HidNpadButton_R, "R"), K(HidNpadButton_ZL, "ZL"), K(HidNpadButton_ZR, "ZR"), K(HidNpadButton_Plus, "PLUS"), K(HidNpadButton_Minus, "MINUS"),
               K(HidNpadButton_StickL, "LSTICK"), K(HidNpadButton_StickR, "RSTICK"), K(HidNpadButton_Up, "UP"), K(HidNpadButton_Down, "DOWN"), K(HidNpadButton_Left, "LEFT"), K(HidNpadButton_Right, "RIGHT"));
        printf("LSTICK %6d %6d  RSTICK %6d %6d        \n", l.x, l.y, r.x, r.y);
        HidVibrationValue v = {.amp_low = (k & HidNpadButton_A) ? 1.0f : 0, .freq_low = 160, .amp_high = (k & HidNpadButton_A) ? 1.0f : 0, .freq_high = 320};
        HidVibrationValue vv[2] = {v, v};
        hidSendVibrationValues(vh, vv, 2);
        consoleUpdate(NULL);
    }
    consoleExit(NULL);
    return 0;
}
