; probe_step.s — une seule etape, choisie par -DSTEP=n.
; 1=mmap seul, 2=uname seul (sans mmap), 3=mprotect, 4=munmap,
; 5=getdents64, 6=mmap+uname (comme stage 1)

global _start

%define SYS_WRITE      1
%define SYS_MMAP       9
%define SYS_MPROTECT   10
%define SYS_MUNMAP     11
%define SYS_UNAME      63
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
    %if STEP == 1
    ; mmap seul : afficher l'adresse retournee
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
    ; ecrire "M1: OK" puis la valeur hexa
    lea rsi, [rel m1]
    mov rdx, m1_len
    call write
    %elif STEP == 2
    ; uname dans une zone de pile (pas de mmap)
    sub rsp, 390
    mov rax, SYS_UNAME
    mov rdi, rsp
    syscall
    add rsp, 390
    cmp rax, 0
    jne .fail
    lea rsi, [rel m2]
    mov rdx, m2_len
    call write
    %elif STEP == 3
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
    %elif STEP == 4
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
    %elif STEP == 5
    ; openat /proc puis getdents64
    mov rax, SYS_OPENAT
    mov rdi, -100
    lea rsi, [rel proc_path]
    xor rdx, rdx
    syscall
    cmp rax, 0
    jl .fail
    mov r13, rax
    sub rsp, 0x1000
    mov rax, SYS_GETDENTS64
    mov rdi, r13
    mov rsi, rsp
    mov rdx, 0x1000
    syscall
    add rsp, 0x1000
    cmp rax, 0
    jl .fail
    lea rsi, [rel m5]
    mov rdx, m5_len
    call write
    %elif STEP == 6
    ; mmap puis uname (comme stage 1)
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
    mov rax, SYS_UNAME
    mov rdi, r12
    syscall
    cmp rax, 0
    jne .fail
    movzx eax, byte [r12]
    test al, al
    jz .fail
    lea rsi, [rel m6]
    mov rdx, m6_len
    call write
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
m1:     db "M1: OK", 10
m1_len  equ $ - m1
m2:     db "M2: OK", 10
m2_len  equ $ - m2
m3:     db "M3: OK", 10
m3_len  equ $ - m3
m4:     db "M4: OK", 10
m4_len  equ $ - m4
m5:     db "M5: OK", 10
m5_len  equ $ - m5
m6:     db "M6: OK", 10
m6_len  equ $ - m6
mf:     db "M: ECHEC", 10
mf_len  equ $ - mf
proc_path: db "/proc", 0
