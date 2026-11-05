; probe_order.s — reproduit l'ordre exact de probe_j4a pour localiser
; l'etape qui fait mourir le noyau.

global _start

%define SYS_WRITE      1
%define SYS_MMAP       9
%define SYS_MPROTECT   10
%define SYS_MUNMAP     11
%define SYS_UNAME      63
%define SYS_LSEEK      8
%define SYS_OPENAT     257
%define SYS_GETDENTS64 217
%define SYS_CLOSE      3
%define SYS_EXIT       60

%define PROT_READ      0x1
%define PROT_WRITE     0x2
%define MAP_PRIVATE    0x02
%define MAP_ANONYMOUS  0x20

%ifndef STEP
%define STEP 1
%endif

section .text
_start:
    ; mmap
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
    mov r12, rax

    ; uname
    mov rax, SYS_UNAME
    mov rdi, r12
    syscall
    cmp rax, 0
    jne .fail

    ; lseek
    mov rax, SYS_LSEEK
    mov rdi, 1
    xor rsi, rsi
    xor rdx, rdx
    syscall
    cmp rax, -38
    je .fail

    ; mprotect
    mov rax, SYS_MPROTECT
    mov rdi, r12
    mov rsi, 0x1000
    mov rdx, PROT_READ
    syscall
    cmp rax, 0
    jne .fail

    ; munmap
    mov rax, SYS_MUNMAP
    mov rdi, r12
    mov rsi, 0x1000
    syscall
    cmp rax, 0
    jne .fail

    %if STEP >= 2
    ; openat /proc
    mov rax, SYS_OPENAT
    mov rdi, -100
    lea rsi, [rel proc_path]
    xor rdx, rdx
    syscall
    cmp rax, 0
    jl .fail
    mov r13, rax

    ; mmap buffer
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
    mov r14, rax

    ; getdents64
    mov rax, SYS_GETDENTS64
    mov rdi, r13
    mov rsi, r14
    mov rdx, 0x1000
    syscall
    cmp rax, 0
    jl .fail

    ; close
    mov rax, SYS_CLOSE
    mov rdi, r13
    syscall
    %endif

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
ok_msg:     db "O: OK", 10
ok_msg_len  equ $ - ok_msg
fail_msg:   db "O: ECHEC", 10
fail_msg_len equ $ - fail_msg
proc_path: db "/proc", 0
