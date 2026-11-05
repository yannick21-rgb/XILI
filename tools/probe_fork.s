; probe_fork.s — teste fork : le parent et l'enfant ecrivent tous les
; deux, puis le parent attend l'enfant (wait4).
;
; Marqueurs attendus :
;   F1: parent (pid N)
;   F2: enfant (pid 0)
;   F3: wait4 OK
;
; Si le noyau meurt, le dernier marqueur montre l'etape.

global _start

%define SYS_WRITE      1
%define SYS_FORK       57
%define SYS_WAIT4      61
%define SYS_GETPID     39
%define SYS_EXIT       60

section .text
_start:
    ; fork()
    mov rax, SYS_FORK
    syscall
    mov r12, rax                 ; r12 = retour fork
    cmp rax, 0
    jl .fail

    je .child                    ; enfant : rax == 0

    ; ---------- parent ----------
    ; ecrire "F1: parent pid=<n>"
    mov rax, SYS_GETPID
    syscall
    mov r13, rax                 ; r13 = pid parent

    lea rsi, [rel m1]
    mov rdx, m1_len
    call write

    ; attendre l'enfant
    mov rax, SYS_WAIT4
    mov rdi, -1                  ; n'importe quel enfant
    xor rsi, rsi                 ; wstatus = NULL
    xor rdx, rdx                 ; options = 0
    syscall
    cmp rax, 0
    jl .fail_wait

    lea rsi, [rel m3]
    mov rdx, m3_len
    call write
    jmp .done

.child:
    ; ---------- enfant ----------
    lea rsi, [rel m2]
    mov rdx, m2_len
    call write

    ; exit(0)
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall

.fail_wait:
    lea rsi, [rel mf3]
    mov rdx, mf3_len
    call write
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
m1:     db "F1: parent", 10
m1_len  equ $ - m1
m2:     db "F2: enfant", 10
m2_len  equ $ - m2
m3:     db "F3: wait4 OK", 10
m3_len  equ $ - m3
mf:     db "F: ECHEC fork", 10
mf_len  equ $ - mf
mf3:    db "F: ECHEC wait4", 10
mf3_len equ $ - mf3
