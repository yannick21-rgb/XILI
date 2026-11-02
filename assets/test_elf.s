/* Minimal ELF64 test program for XILI kernel — J3.5 cpio initrd test.
 *
 * Exercises: sys_write, sys_read, sys_openat, sys_close, sys_brk,
 * sys_getpid, sys_exit.
 * Writes the J3.3 + J3.4 + J3.5 markers, then opens "hello.txt" from
 * the initrd, reads it, and writes the file content to stdout.
 *
 * Compiles: gcc -nostdlib -nostartfiles -static -no-pie -o test_elf test_elf.s
 */
.section .text
.global _start

_start:
    /* --- J3.3 marker (backward compat) --- */
    mov    $1, %rax
    mov    $1, %rdi
    lea    msg_j33(%rip), %rsi
    mov    $21, %rdx
    syscall

    /* --- J3.4 marker --- */
    mov    $1, %rax
    mov    $1, %rdi
    lea    msg_j34(%rip), %rsi
    mov    $24, %rdx
    syscall

    /* --- J3.5 marker --- */
    mov    $1, %rax
    mov    $1, %rdi
    lea    msg_j35(%rip), %rsi
    mov    $32, %rdx
    syscall

    /* --- sys_read(0, buf, 6) — EOF expected (no serial input) --- */
    mov    $0, %rax
    mov    $0, %rdi
    lea    buf(%rip), %rsi
    mov    $6, %rdx
    syscall
    /* rax = 0 (EOF) — expected */

    /* --- sys_openat(AT_FDCWD, "hello.txt", O_RDONLY, 0) --- */
    mov    $257, %rax
    mov    $-100, %rdi
    lea    path(%rip), %rsi
    xor    %rdx, %rdx
    xor    %r10, %r10
    syscall
    /* rax = fd (should be 3) */
    mov    %rax, %r12

    /* --- sys_read(fd, buf, 64) — read file content --- */
    mov    $0, %rax
    mov    %r12, %rdi
    lea    buf(%rip), %rsi
    mov    $64, %rdx
    syscall
    /* rax = bytes read (should be 24) */
    mov    %rax, %r13

    /* --- sys_write(1, buf, bytes_read) — echo file content --- */
    mov    $1, %rax
    mov    $1, %rdi
    lea    buf(%rip), %rsi
    mov    %r13, %rdx
    syscall

    /* --- sys_close(fd) --- */
    mov    $3, %rax
    mov    %r12, %rdi
    syscall

    /* --- sys_brk(0) — query current break --- */
    mov    $12, %rax
    xor    %rdi, %rdi
    syscall
    /* rax = 0x60000000 */

    /* --- sys_getpid() --- */
    mov    $39, %rax
    syscall
    /* rax = 1 */

    /* --- sys_exit(0) --- */
    mov    $60, %rax
    xor    %rdi, %rdi
    syscall

    /* Fallback: if exit returns, try exit_group --- */
    mov    $231, %rax
    xor    %rdi, %rdi
    syscall

    /* If still alive, halt --- */
    hlt
    jmp    .

.section .rodata
msg_j33:
    .ascii "J3.3: ELF charge OK\n"
msg_j34:
    .ascii "J3.4: multi-syscall ELF\n"
msg_j35:
    .ascii "J3.5: cpio initrd openat/read\n"
path:
    .asciz "hello.txt"

.section .bss
buf:
    .skip 64
