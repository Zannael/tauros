@ Tauros probe ROM (original, MIT): the EMU-001 test cartridge. No Nintendo logo or
@ other third-party bytes; mGBA boots it with its HLE BIOS, which skips the logo check.
@ Mode 3 screen: blue, red while A is held; channel 1 plays a square tone.
@ The bytes are embedded in tests/mgba_core.rs; rebuild with devkitARM:
@   arm-none-eabi-as -o probe.o probe.s && arm-none-eabi-objcopy -O binary probe.o probe.gba
@ (312 bytes, sha256 6565b93304580d6cc645c5d3920d649faebf3cbffc54602b979c1c74ffee8f6d)
    .arm
    .global _start
_start:
    b start
    .fill 0xAC - 4, 1, 0
    .ascii "TAUR"
    .ascii "01"
    .byte 0x96
    .fill 0xC0 - 0xB3, 1, 0
start:
    mov r0, #0x04000000
    mov r1, #0x80
    strh r1, [r0, #0x84]
    ldr r1, =0x1177
    strh r1, [r0, #0x80]
    mov r1, #2
    strh r1, [r0, #0x82]
    ldr r1, =0xF080
    strh r1, [r0, #0x62]
    ldr r1, =0x86D6
    strh r1, [r0, #0x64]
    ldr r1, =0x0403
    strh r1, [r0]
    add r7, r0, #0x130
loop:
    ldrh r3, [r7]
    tst r3, #1
    ldreq r4, =0x001F001F
    ldrne r4, =0x7C007C00
    mov r6, #0x06000000
    mov r5, #0x12C00
fill:
    str r4, [r6], #4
    subs r5, r5, #4
    bne fill
    b loop
    .ltorg
