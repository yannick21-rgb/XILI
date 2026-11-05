; probe_j4b.s — sonde J4-B : futex, ioctl TTY, signaux, getrandom, memfd.
;
; Enchaine les syscalls et ecrit un marqueur par etape.
; Un echec s'arrete (exit 1) : le dernier marqueur montre l'etape.

global _start

%define SYS_WRITE      1
%define SYS_MMAP       9
%define SYS_IOCTL      16
%define SYS_RT_SIGACTION 13
%define SYS_RT_SIGPROCMASK 14
%define SYS_KILL       62
%define SYS_FUTEX      202
%define SYS_GETRANDOM  318
%define SYS_MEMFD_CREATE 319
%define SYS_OPENAT     257
%define SYS_CLOSE      3
%define SYS_EXIT       60

%define PROT_READ      0x1
%define PROT_WRITE     0x2
%define MAP_PRIVATE    0x02
%define MAP_ANONYMOUS  0x20

%define TCGETS         0x5401
%define TIOCGWINSZ     0x5413

%define FUTEX_WAIT     0
%define FUTEX_WAKE     1

section .text
_start:
    ; --- etape 1 : mmap anonyme ---
    mov rax, SYS_MMAP
    xor rdi, rdi
    mov rsi, 0x1000
    mov rdx, PROT_READ | PROT_WRITE
    mov r10, MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9, r9
    syscall
    cmp rax, 0
    jl .fail
    mov r12, rax                 ; r12 = base mmap

    ; --- etape 2 : ioctl TCGETS sur fd=1 ---
    mov rax, SYS_IOCTL
    mov rdi, 1
    mov rsi, TCGETS
    mov rdx, r12
    syscall
    cmp rax, 0
    jne .fail

    ; --- etape 3 : ioctl TIOCGWINSZ ---
    mov rax, SYS_IOCTL
    mov rdi, 1
    mov rsi, TIOCGWINSZ
    mov rdx, r12
    syscall
    cmp rax, 0
    jne .fail

    ; --- etape 4 : futex WAIT sur une valeur deja differente ---
    ; *uaddr = 1, on attend 0 -> doit rendre 0 (pas d'attente)
    mov dword [r12], 1
    mov rax, SYS_FUTEX
    mov rdi, r12
    mov rsi, FUTEX_WAIT
    mov rdx, 0
    xor r10, r10
    xor r8, r8
    xor r9, r9
    syscall
    cmp rax, 0
    jne .fail

    ; --- etape 5 : futex WAKE (0 attendeur) ---
    mov rax, SYS_FUTEX
    mov rdi, r12
    mov rsi, FUTEX_WAKE
    mov rdx, 1
    xor r10, r10
    xor r8, r8
    xor r9, r9
    syscall
    cmp rax, 0
    jl .fail

    ; --- etape 6 : rt_sigaction ---
    mov rax, SYS_RT_SIGACTION
    mov rdi, 17                  ; SIGCHLD
    xor rsi, rsi                 ; act = NULL
    mov rdx, r12                 ; oldact
    syscall
    cmp rax, 0
    jne .fail

    ; --- etape 7 : rt_sigprocmask ---
    mov rax, SYS_RT_SIGPROCMASK
    xor rdi, rdi                 ; SIG_BLOCK
    xor rsi, rsi                 ; set = NULL
    mov rdx, r12                 ; oldset
    syscall
    cmp rax, 0
    jne .fail

    ; --- etape 8 : getrandom ---
    mov rax, SYS_GETRANDOM
    mov rdi, r12
    mov rsi, 16
    xor rdx, rdx
    syscall
    cmp rax, 16
    jne .fail

    ; --- etape 9 : memfd_create ---
    mov rax, SYS_MEMFD_CREATE
    lea rdi, [rel memfd_name]
    xor rsi, rsi
    syscall
    cmp rax, 0
    jl .fail

    ; --- etape 10 : kill(getpid(), 0) ---
    ; sig=0 = verification d'existence, ne tue pas
    mov rax, SYS_KILL
    mov rdi, 1                   ; pid 1 (init)
    xor rsi, rsi                 ; sig 0
    syscall
    ; -ESRCH est acceptable (pas de processus 1) ; -EINVAL non
    cmp rax, -3
    je .ok
    cmp rax, 0
    jne .fail

.ok:
    lea rsi, [rel ok_msg]
    mov rdx, ok_msg_len
    call write
    jmp .done

.fail:
    lea rsi, [rel fail_msg]
    mov rdx, fail_msg_len
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
ok_msg:     db "probe_j4b: OK", 10
ok_msg_len  equ $ - ok_msg
fail_msg:   db "probe_j4b: ECHEC", 10
fail_msg_len equ $ - fail_msg
memfd_name: db "probe", 0
