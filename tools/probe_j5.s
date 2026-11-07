; probe_j5.s — sonde J5-B : socket UDP (socket/bind/sendto/recvfrom).
global _start
%define SYS_WRITE   1
%define SYS_SOCKET  41
%define SYS_BIND    49
%define SYS_SENDTO  44
%define SYS_RECVFROM 45
%define SYS_EXIT    60
%define AF_INET     2
%define SOCK_DGRAM  2

section .text
_start:
    ; socket(AF_INET, SOCK_DGRAM, 0)
    mov rax, SYS_SOCKET
    mov rdi, AF_INET
    mov rsi, SOCK_DGRAM
    xor rdx, rdx
    syscall
    test rax, rax
    jl .fail
    mov r12, rax                 ; r12 = sock fd
    ; bind(10.0.2.15:12345)
    mov rax, SYS_BIND
    mov rdi, r12
    lea rsi, [rel sa_me]
    mov rdx, 16
    syscall
    test rax, rax
    jl .fail
    ; sendto("hello-udp", 9) -> 10.0.2.2:53
    mov rax, SYS_SENDTO
    mov rdi, r12
    lea rsi, [rel payload]
    mov rdx, 9
    xor r10, r10
    lea r8, [rel sa_dst]
    mov r9, 16
    syscall
    cmp rax, 9
    jne .fail
    ; recvfrom : EAGAIN (-11) attendu (pas de réponse) ou données
    mov rax, SYS_RECVFROM
    mov rdi, r12
    lea rsi, [rel buf]
    mov rdx, 64
    mov r10, 0x40                ; MSG_DONTWAIT
    xor r8, r8
    xor r9, r9
    syscall
    cmp rax, -11
    je .ok1
    test rax, rax
    jg .ok1
    jmp .fail
.ok1:
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel mok]
    mov rdx, mok_len
    syscall
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall
.fail:
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel mf]
    mov rdx, mf_len
    syscall
    mov rax, SYS_EXIT
    mov rdi, 1
    syscall

section .rodata
mok: db "probe_j5: OK", 10
mok_len equ $ - mok
mf: db "probe_j5: ECHEC", 10
mf_len equ $ - mf
payload: db "hello-udp"
sa_me: dw 2
    dw 0x3930                  ; port 12345 en ordre réseau (bytes 30 39)
    db 10, 0, 2, 15
    dq 0
sa_dst: dw 2
    dw 0x3500                  ; port 53 en ordre réseau (bytes 00 35)
    db 10, 0, 2, 2
    dq 0
section .bss
buf: resb 64
