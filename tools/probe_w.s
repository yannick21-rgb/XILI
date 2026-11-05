; probe_w.s — teste uniquement write, sans autre syscall.
; Si ca marche apres execve, le probleme est dans les syscalls.
; Si ca echoue, le probleme est le binaire (taille .text, rodata).

global _start

section .text
_start:
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, msg_len
    syscall

    mov rax, 60
    xor rdi, rdi
    syscall

section .rodata
msg: db 'W: OK', 10
msg_len equ $ - msg
