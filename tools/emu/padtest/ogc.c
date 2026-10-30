// Pad tester for GameCube (and Wii with -DHW_RVL): prints what the console reads from pad 1
// (and Wiimote 1 with its Nunchuk on Wii) every frame, and rumbles while A is held.
// Built by tools/emu/padtest/build.sh in devkitpro/devkitppc.
#include <gccore.h>
#include <stdio.h>
#ifdef HW_RVL
#include <wiiuse/wpad.h>
#endif

static void *xfb;
static GXRModeObj *rmode;

#define B(mask, name) ((b & (mask)) ? name " " : "")

int main(void) {
    VIDEO_Init();
    PAD_Init();
#ifdef HW_RVL
    WPAD_Init();
    WPAD_SetDataFormat(WPAD_CHAN_0, WPAD_FMT_BTNS_ACC_IR);
    WPAD_SetVRes(WPAD_CHAN_0, 640, 480);
#endif
    rmode = VIDEO_GetPreferredMode(NULL);
    xfb = MEM_K0_TO_K1(SYS_AllocateFramebuffer(rmode));
    console_init(xfb, 20, 20, rmode->fbWidth, rmode->xfbHeight, rmode->fbWidth * VI_DISPLAY_PIX_SZ);
    VIDEO_Configure(rmode);
    VIDEO_SetNextFramebuffer(xfb);
    VIDEO_SetBlack(FALSE);
    VIDEO_Flush();
    VIDEO_WaitVSync();
    while (1) {
        PAD_ScanPads();
        u16 b = PAD_ButtonsHeld(0);
        printf("\x1b[2;0H");
        printf("RARE PAD TEST  GC PAD 1                                   \n\n");
        printf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s                              \n", B(PAD_BUTTON_A, "A"), B(PAD_BUTTON_B, "B"), B(PAD_BUTTON_X, "X"), B(PAD_BUTTON_Y, "Y"), B(PAD_TRIGGER_Z, "Z"),
               B(PAD_BUTTON_START, "START"), B(PAD_TRIGGER_L, "L"), B(PAD_TRIGGER_R, "R"), B(PAD_BUTTON_UP, "UP"), B(PAD_BUTTON_DOWN, "DOWN"), B(PAD_BUTTON_LEFT, "LEFT"), B(PAD_BUTTON_RIGHT, "RIGHT"));
        printf("STICK %4d %4d   C %4d %4d   LA %3d  RA %3d          \n", PAD_StickX(0), PAD_StickY(0), PAD_SubStickX(0), PAD_SubStickY(0), PAD_TriggerL(0), PAD_TriggerR(0));
        PAD_ControlMotor(0, (b & PAD_BUTTON_A) ? PAD_MOTOR_RUMBLE : PAD_MOTOR_STOP);
#ifdef HW_RVL
        WPAD_ScanPads();
        u32 w = WPAD_ButtonsHeld(0);
        WPADData *d = WPAD_Data(0);
#undef B
#define B(mask, name) ((w & (mask)) ? name " " : "")
        printf("\nWIIMOTE 1                                               \n");
        printf("BTN: %s%s%s%s%s%s%s%s%s%s%s%s%s                         \n", B(WPAD_BUTTON_A, "A"), B(WPAD_BUTTON_B, "B"), B(WPAD_BUTTON_1, "1"), B(WPAD_BUTTON_2, "2"), B(WPAD_BUTTON_MINUS, "MINUS"),
               B(WPAD_BUTTON_PLUS, "PLUS"), B(WPAD_BUTTON_HOME, "HOME"), B(WPAD_BUTTON_UP, "UP"), B(WPAD_BUTTON_DOWN, "DOWN"), B(WPAD_BUTTON_LEFT, "LEFT"), B(WPAD_BUTTON_RIGHT, "RIGHT"),
               B(WPAD_NUNCHUK_BUTTON_C, "NUN-C"), B(WPAD_NUNCHUK_BUTTON_Z, "NUN-Z"));
        u32 type = 0;
        s32 probe = WPAD_Probe(0, &type);
        expansion_t ex;
        WPAD_Expansion(0, &ex);
        if (probe == WPAD_ERR_NONE && type == WPAD_EXP_NUNCHUK)
            printf("NUNCHUK %3d %3d                    \n", ex.nunchuk.js.pos.x, ex.nunchuk.js.pos.y);
        else
            printf("NUNCHUK none (probe %d type %u)     \n", (int)probe, (unsigned)type);
        if (d && d->ir.valid)
            printf("IR %4d %4d                        \n", (int)d->ir.x, (int)d->ir.y);
        else
            printf("IR none                            \n");
        printf("ACC %4d %4d %4d               \n", d ? d->accel.x : 0, d ? d->accel.y : 0, d ? d->accel.z : 0);
        WPAD_Rumble(0, (w & WPAD_BUTTON_A) ? 1 : 0);
#endif
        VIDEO_WaitVSync();
    }
}
