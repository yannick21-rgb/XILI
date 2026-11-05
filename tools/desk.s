; desk.s — bureau userland J4 (lot 3 : le bureau lance un userland).
;
; Le bureau userland ecrit son cadre, puis se substitue (execve) au
; programme d'etat courant : ici "hello.elf". La nouvelle tâche
; (même PID) reçoit le contexte du programme lancé.

global _start
section .text
_start:
    mov rax, 1          ; SYS_write
    mov rdi, 1          ; fd 1
    mov rsi, msg1
    mov rdx, msg1_len
    syscall             ; "J4: bureau pret"

    mov rax, 1          ; SYS_write
    mov rdi, 1          ; fd 1
    mov rsi, frame
    mov rdx, frame_len
    syscall             ; cadre de la fenetre

    mov rax, 59         ; SYS_execve
    mov rdi, hello_name ; NUL-terminated : "hello.elf"
    mov rsi, 0          ; argv/envp (ignore par le noyau)
    mov rdx, 0
    syscall             ; execve hello.elf -> remplace ce processus

    mov rax, 60         ; SYS_exit
    mov rdi, 1          ; exit(1) si execve echoue
    syscall

section .rodata
msg1: db 'J4: bureau pret', 10
msg1_len equ $ - msg1
frame: db '+------------------------+', 10, '| Bureau                 |', 10, '+------------------------+', 10
frame_len equ $ - frame
hello_name: db 'hello.elf', 0

section .bss
buf: resb 256
