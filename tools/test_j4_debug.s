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
msg: db 'test_j4: OK', 10
msg_len equ $ - msg
