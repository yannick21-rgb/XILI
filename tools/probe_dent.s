; probe_dent.s — teste openat /proc puis getdents64, avec marqueurs
; intermediaires pour localiser l'echec.

global _start

%define SYS_WRITE      1
%define SYS_OPENAT     257
%define SYS_GETDENTS64 217
%define SYS_CLOSE      3
%define SYS_EXIT       60

section .text
_start:
    ; openat(AT_FDCWD, "/proc", O_RDONLY)
    mov rax, SYS_OPENAT
    mov rdi, -100
    lea rsi, [rel proc_path]
    xor rdx, rdx
    syscall
    cmp rax, 0
    jl .fail_open
    mov r13, rax

    lea rsi, [rel m1]
    mov rdx, m1_len
    call write

    ; getdents64(fd, stack_buf, 0x1000)
    sub rsp, 0x1000
    mov rax, SYS_GETDENTS64
    mov rdi, r13
    mov rsi, rsp
    mov rdx, 0x1000
    syscall
    mov r14, rax
    add rsp, 0x1000
    cmp r14, 0
    jl .fail_dents

    lea rsi, [rel m2]
    mov rdx, m2_len
    call write

    ; close
    mov rax, SYS_CLOSE
    mov rdi, r13
    syscall

    lea rsi, [rel m3]
    mov rdx, m3_len
    call write
    jmp .done

.fail_open:
    lea rsi, [rel mf1]
    mov rdx, mf1_len
    call write
    jmp .done
.fail_dents:
    lea rsi, [rel mf2]
    mov rdx, mf2_len
    call write
.done:
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall

write:
    mov rax, SYS_WRITE
    mov rdi, 1
    syscall
    ret

section .rodata
m1:     db "D1: openat OK", 10
m1_len  equ $ - m1
m2:     db "D2: getdents64 OK", 10
m2_len  equ $ - m2
m3:     db "D3: close OK", 10
m3_len  equ $ - m3
mf1:    db "D: ECHEC openat", 10
mf1_len equ $ - mf1
mf2:    db "D: ECHEC getdents64", 10
mf2_len equ $ - mf2
proc_path: db "/proc", 0
