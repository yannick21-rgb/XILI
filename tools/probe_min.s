; probe_min.s — sonde minimale : un seul syscall a la fois.
; Marqueurs : "P0: OK" (write), "P1: OK" (uname), "P2: OK" (lseek),
;             "P3: OK" (mprotect), "P4: OK" (munmap), "P5: OK" (getdents64)

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

; STAGE est fourni au compilateur (-DSTAGE=n). Valeur par defaut : 0.
%ifndef STAGE
%define STAGE 0
%endif

section .text
_start:
    %if STAGE == 0
    lea rsi, [rel m0]
    mov rdx, m0_len
    call write
    %elif STAGE == 1
    ; mmap anonyme
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
    movzx eax, byte [r12]
    test al, al
    jz .fail
    lea rsi, [rel m1]
    mov rdx, m1_len
    call write
    %elif STAGE == 2
    ; lseek sur stdout : doit repondre (pas -ENOSYS)
    mov rax, SYS_LSEEK
    mov rdi, 1
    xor rsi, rsi
    xor rdx, rdx
    syscall
    cmp rax, -38
    je .fail
    lea rsi, [rel m2]
    mov rdx, m2_len
    call write
    %elif STAGE == 3
    ; mmap puis mprotect
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
    mov rax, SYS_MPROTECT
    mov rdi, r12
    mov rsi, 0x1000
    mov rdx, PROT_READ
    syscall
    cmp rax, 0
    jne .fail
    lea rsi, [rel m3]
    mov rdx, m3_len
    call write
    %elif STAGE == 4
    ; mmap puis munmap
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
    mov rax, SYS_MUNMAP
    mov rdi, r12
    mov rsi, 0x1000
    syscall
    cmp rax, 0
    jne .fail
    lea rsi, [rel m4]
    mov rdx, m4_len
    call write
    %elif STAGE == 5
    ; openat /proc puis getdents64
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
    mov rax, SYS_GETDENTS64
    mov rdi, r13
    mov rsi, r14
    mov rdx, 0x1000
    syscall
    cmp rax, 0
    jl .fail
    ; afficher le nombre d'octets
    mov r15, rax
    lea rsi, [rel m5]
    mov rdx, m5_len
    call write
    ; close
    mov rax, SYS_CLOSE
    mov rdi, r13
    syscall
    %endif
    jmp .done

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
m0:     db "P0: OK", 10
m0_len  equ $ - m0
m1:     db "P1: OK", 10
m1_len  equ $ - m1
m2:     db "P2: OK", 10
m2_len  equ $ - m2
m3:     db "P3: OK", 10
m3_len  equ $ - m3
m4:     db "P4: OK", 10
m4_len  equ $ - m4
m5:     db "P5: OK", 10
m5_len  equ $ - m5
mf:     db "P: ECHEC", 10
mf_len  equ $ - mf
proc_path: db "/proc", 0
