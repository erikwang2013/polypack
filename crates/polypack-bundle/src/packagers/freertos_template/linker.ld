/* PolyPack FreeRTOS sample board (T-M6-07): mps2-an385 memory map.
 * Code/data are linked at address 0 (QEMU loads the ELF there); RAM is the
 * 4 MiB ZBT SRAM bank. The vector table must stay at 0x00000000 (VTOR reset). */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x00000000, LENGTH = 4M
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 4M
}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .text : {
        KEEP(*(.vectors))
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > FLASH

    /* 引擎归档引用 __aeabi_unwind_cpp_pr0，-lgcc 的 unwind-arm.o 需要异常索引符号。 */
    .ARM.exidx : {
        __exidx_start = .;
        *(.ARM.exidx .ARM.exidx.*)
        __exidx_end = .;
    } > FLASH

    .data : {
        _sdata = .;
        *(.data .data.*)
        _edata = .;
    } > RAM AT > FLASH
    _sidata = LOADADDR(.data);

    .bss (NOLOAD) : {
        _sbss = .;
        *(.bss .bss.* COMMON)
        _ebss = .;
    } > RAM
}
