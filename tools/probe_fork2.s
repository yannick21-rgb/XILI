; probe_fork2.s — teste fork SANS wait4 : le parent ecrit, l'enfant ecrit,
; puis le parent ecrit "P: fin". Si l'enfant ne s'execute pas, on ne
; verra que "P1: parent" et "P: fin".

global _start

%define SYS_WRITE      1
%define SYS_FORK       57
%define SYS_GETPID     39
%define SYS_EXIT       60

section .text
_start:
    mov rax, SYS_FORK
    syscall
    mov r12, rax
    cmp rax, 0
    jl .fail
    je .child

    ; parent
    lea rsi, [rel m1]
    mov rdx, m1_len
    call write

    ; attendre un peu (laisse le scheduler tourner)
    mov rcx, 100000000
.spin:
    loop .spin

    lea rsi, [rel m3]
    mov rdx, m3_len
    call write
    jmp .done

.child:
    lea rsi, [rel m2]
    mov rdx, m2_len
    call write
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall

.fail:
    lea rsi, [rel mf]
    mov rdx, mf_len
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
m1:     db "P1: parent", 10
m1_len  equ $ - m1
m2:     db "P2: enfant", 10
m2_len  equ $ - m2
m3:     db "P3: fin parent", 10
m3_len  equ $ - m3
mf:     db "P: ECHEC fork", 10
mf_len  equ $ - mf
