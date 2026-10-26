ENTRY(_start)

SECTIONS
{
    . = 1M;

    .multiboot : ALIGN(8)
    {
        KEEP(*(.multiboot))
    }

    .text : ALIGN(4096)
    {
        *(.text .text.*)
    }

    .rodata : ALIGN(4096)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4096)
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.comment)
        *(.note*)
    }
}
