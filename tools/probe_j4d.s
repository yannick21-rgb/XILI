; probe_j4d.s — sonde J4-D : eventfd+epoll, timerfd, signalfd, clone,
; /proc/self/stat, /dev/urandom.
global _start
%define SYS_READ       0
%define SYS_WRITE      1
%define SYS_CLOSE      3
%define SYS_GETPID     39
%define SYS_FORK       57
%define SYS_CLONE      56
%define SYS_EXIT       60
%define SYS_WAIT4      61
%define SYS_KILL       62
%define SYS_TIMERFD_SETTIME 86
%define SYS_EPOLL_WAIT 232
%define SYS_EPOLL_CTL  233
%define SYS_SIGNALFD   282
%define SYS_TIMERFD_CREATE 283
%define SYS_EVENTFD2   290
%define SYS_EPOLL_CREATE1 291
%define SYS_OPENAT     257
%define AT_FDCWD       -100
%define EPOLLIN        0x001
%define EPOLL_CTL_ADD  1

section .text
_start:
    ; --- eventfd2(0,0) ---
    mov rax, SYS_EVENTFD2
    xor rdi, rdi
    xor rsi, rsi
    syscall
    test rax, rax
    jl .fail
    mov r12, rax                 ; r12 = eventfd
    ; write 1 (8 octets)
    mov qword [rel one], 1
    mov rax, SYS_WRITE
    mov rdi, r12
    lea rsi, [rel one]
    mov rdx, 8
    syscall
    cmp rax, 8
    jne .fail
    ; --- epoll_create1 + ctl ADD + wait ---
    mov rax, SYS_EPOLL_CREATE1
    xor rdi, rdi
    syscall
    test rax, rax
    jl .fail
    mov r13, rax                 ; r13 = epoll fd
    mov dword [rel ev], EPOLLIN
    mov qword [rel ev+4], 0x1234
    mov rax, SYS_EPOLL_CTL
    mov rdi, r13
    mov rsi, EPOLL_CTL_ADD
    mov rdx, r12
    lea r10, [rel ev]
    syscall
    test rax, rax
    jl .fail
    mov rax, SYS_EPOLL_WAIT
    mov rdi, r13
    lea rsi, [rel evout]
    mov rdx, 1
    xor r10, r10
    syscall
    cmp rax, 1
    jne .fail
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m1]
    mov rdx, m1_len
    syscall

    ; --- timerfd : settime 5 ticks, poll read jusqu'a expiration ---
    mov rax, SYS_TIMERFD_CREATE
    xor rdi, rdi
    xor rsi, rsi
    syscall
    test rax, rax
    jl .fail
    mov r14, rax
    mov qword [rel itv], 5       ; value sec=0
    mov qword [rel itv+8], 0     ; value nsec=0 -> on met sec=0,nsec=50000000 ci-dessous
    mov qword [rel itv], 0
    mov qword [rel itv+8], 50000000   ; 0.05s = 5 ticks
    mov qword [rel itv+16], 0
    mov qword [rel itv+24], 0
    mov rax, SYS_TIMERFD_SETTIME
    mov rdi, r14
    xor rsi, rsi
    lea rdx, [rel itv]
    xor r10, r10
    syscall
    test rax, rax
    jl .fail
    mov rbx, 200000              ; boucle bornee (rcx clobbere par syscall)
.tloop:
    mov rax, SYS_READ
    mov rdi, r14
    lea rsi, [rel tbuf]
    mov rdx, 8
    syscall
    cmp rax, 8
    je .tok
    dec rbx
    jnz .tloop
    jmp .fail
.tok:
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m2]
    mov rdx, m2_len
    syscall

    ; --- signalfd : mask SIGUSR1(10), kill, read ---
    mov rax, SYS_SIGNALFD
    mov rdi, -1
    lea rsi, [rel mask]
    mov rdx, 8
    syscall
    test rax, rax
    jl .fail
    mov rbx, rax
    mov rax, SYS_GETPID
    syscall
    mov rdi, rax
    mov rsi, 10                  ; SIGUSR1
    mov rax, SYS_KILL
    syscall
    mov rax, SYS_READ
    mov rdi, rbx
    lea rsi, [rel sig]
    mov rdx, 128
    syscall
    cmp rax, 128
    jne .fail
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m3]
    mov rdx, m3_len
    syscall

    ; --- clone (= fork sans THREAD) : enfant ecrit, parent wait4 ---
    mov rax, SYS_CLONE
    xor rdi, rdi
    xor rsi, rsi
    xor rdx, rdx
    xor r10, r10
    xor r8, r8
    syscall
    cmp rax, 0
    jl .fail
    je .child
    mov rax, SYS_WAIT4
    mov rdi, -1
    xor rsi, rsi
    xor rdx, rdx
    syscall
    cmp rax, 0
    jl .fail
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m4]
    mov rdx, m4_len
    syscall
    jmp .after_clone
.child:
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m4c]
    mov rdx, m4c_len
    syscall
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall
.after_clone:

    ; --- /proc/self/stat : openat + read, 1er octet = chiffre ---
    mov rax, SYS_OPENAT
    mov rdi, AT_FDCWD
    lea rsi, [rel pstat]
    xor rdx, rdx
    xor r10, r10
    syscall
    test rax, rax
    jl .fail
    mov rbp, rax
    mov rax, SYS_READ
    mov rdi, rbp
    lea rsi, [rel buf]
    mov rdx, 256
    syscall
    test rax, rax
    jle .fail
    mov al, [rel buf]
    cmp al, '0'
    jl .fail
    cmp al, '9'
    jg .fail
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m5]
    mov rdx, m5_len
    syscall

    ; --- /dev/urandom : openat + read 4 ---
    mov rax, SYS_OPENAT
    mov rdi, AT_FDCWD
    lea rsi, [rel prand]
    xor rdx, rdx
    xor r10, r10
    syscall
    test rax, rax
    jl .fail
    mov rbp, rax
    mov rax, SYS_READ
    mov rdi, rbp
    lea rsi, [rel buf]
    mov rdx, 4
    syscall
    cmp rax, 4
    jne .fail
    mov rax, SYS_WRITE
    mov rdi, 1
    lea rsi, [rel m6]
    mov rdx, m6_len
    syscall

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
m1: db "D1: eventfd+epoll OK", 10
m1_len equ $ - m1
m2: db "D2: timerfd OK", 10
m2_len equ $ - m2
m3: db "D3: signalfd OK", 10
m3_len equ $ - m3
m4: db "D4: clone OK", 10
m4_len equ $ - m4
m4c: db "D4c: enfant clone", 10
m4c_len equ $ - m4c
m5: db "D5: proc stat OK", 10
m5_len equ $ - m5
m6: db "D6: urandom OK", 10
m6_len equ $ - m6
mok: db "probe_j4d: OK", 10
mok_len equ $ - mok
mf: db "probe_j4d: ECHEC", 10
mf_len equ $ - mf
pstat: db "/proc/self/stat", 0
prand: db "/dev/urandom", 0
mask: dq (1 << 10)
one: dq 0
section .bss
itv: resq 4
tbuf: resq 1
sig: resb 128
buf: resb 256
ev: resb 12
evout: resb 12
