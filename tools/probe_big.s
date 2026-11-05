; probe_big.s — write + remplissage du .text pour reproduire la taille
; de probe_min (0xae octets). Si ca echoue, le probleme est la taille
; du segment LOAD (ou le nombre de pages).

global _start

section .text
_start:
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, msg_len
    syscall

    ; remplir pour atteinde ~0xae octets de .text
    times 200 nop

    mov rax, 60
    xor rdi, rdi
    syscall

section .rodata
msg: db 'B: OK', 10
msg_len equ $ - msg
