; shell.s — shell utilisateur minimal XILI (J3/J4).
;
; Prompt -> lecture (tty, polling) -> execve d'un programme du initrd.

global _start
section .text
_start:
    mov rax, 1          ; SYS_write
    mov rdi, 1          ; fd 1
    mov rsi, msg
    mov rdx, msg_len
    syscall             ; "J4: shell pret"

shell_loop:
    mov rax, 0          ; SYS_read
    mov rdi, 0          ; fd 0
    mov rsi, buf
    mov rdx, 256
    syscall             ; rax = nb octets lus
    mov r8, rax         ; r8 = len (preserve avant les syscalls suivants)
    mov rax, 1
    mov rdi, 1
    mov rsi, buf
    mov rdx, r8
    syscall             ; debug : renvoyer la ligne lue
    test r8, r8
    jz shell_loop       ; len == 0 -> polling
    dec r8              ; tronquer le \n final
    mov byte [buf + r8], 0
    test r8, r8
    jz shell_loop       ; commande vide -> polling
    mov rax, 59         ; SYS_execve
    mov rdi, buf
    mov rsi, 0
    mov rdx, 0
    syscall
    mov rax, 60         ; SYS_exit
    mov rdi, 1
    syscall

section .rodata
msg: db 'J4: shell pret', 10
msg_len equ $ - msg

section .bss
buf: resb 256
