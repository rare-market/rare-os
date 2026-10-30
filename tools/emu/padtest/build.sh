#!/bin/bash
# Builds the pad testers with the homebrew toolchains in Docker (on the build machine).
set -e
cd "$(dirname "$0")"
mkdir -p out
docker run --rm -v "$PWD":/src -w /src devkitpro/devkitppc bash -c '
set -e
export PATH=$DEVKITPPC/bin:$DEVKITPRO/tools/bin:$PATH
O=$DEVKITPRO/libogc
powerpc-eabi-gcc -O2 -DGEKKO -mogc -mcpu=750 -meabi -mhard-float -I$O/include ogc.c -L$O/lib/cube -logc -lm -o out/gc.elf
elf2dol out/gc.elf out/padtest-gc.dol
powerpc-eabi-gcc -O2 -DGEKKO -DHW_RVL -mrvl -mcpu=750 -meabi -mhard-float -I$O/include ogc.c -L$O/lib/wii -lwiiuse -lbte -logc -lm -o out/wii.elf
elf2dol out/wii.elf out/padtest-wii.dol
'
# DS, 3DS and Switch with devkitPro's own application templates.
for t in nds:nds/templates/arm9:devkitarm 3ds:3ds/templates/application:devkitarm switch:switch/templates/application:devkita64; do
    IFS=: read name tpl img <<<"$t"
    src=$( [ $name = nds ] && echo nds.c || ([ $name = 3ds ] && echo ctr.c || echo nx.c) )
    docker run --rm -v "$PWD":/src -w /tmp devkitpro/$img bash -c "
set -e
cp -r \$DEVKITPRO/examples/$tpl /tmp/p && cd /tmp/p && rm -f source/*.c source/*.cpp && cp /src/$src source/main.c
make -j8 TARGET=padtest >/dev/null
cp padtest.nds padtest.3dsx padtest.nro /src/out/ 2>/dev/null || true
for f in padtest.nds padtest.3dsx padtest.nro; do [ -f \$f ] && cp \$f /src/out/\$f; done; true"
done
docker run --rm -v "$PWD":/src -w /tmp pspdev/pspdev sh -c '
set -e
mkdir p && cd p && cp /src/psp.c main.c
cat > Makefile <<MK
TARGET = padtest
OBJS = main.o
LIBS = -lpspctrl -lpspdisplay
EXTRA_TARGETS = EBOOT.PBP
PSP_EBOOT_TITLE = Rare Pad Test
PSPSDK=\$(shell psp-config --pspsdk-path)
include \$(PSPSDK)/lib/build.mak
MK
make >/dev/null
mkdir -p /src/out/psp && cp EBOOT.PBP /src/out/psp/ && cp padtest.prx /src/out/psp/ 2>/dev/null; cp padtest.elf /src/out/padtest-psp.elf'
ls -la out out/psp
