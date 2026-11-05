; desk.s — bureau utilisateur minimal (J4).
;
; Programme userland du bureau : ecrit l'ecran du bureau (marker),
; puis se termine. Le bureau complet (cadre fenetre actif, fond sombre,
; bord lumineux) viendra etendre ce squelette au moment ou le rendu
; graphique userland (framebuffer) entrera dans le lot ABI.

global _start
section .text
_start:
    mov rax, 1          ; SYS_write
    mov rdi, 1          ; fd 1
    mov rsi, msg
    mov rdx, msg_len
    syscall             ; "J4: bureau pret"

    mov rax, 60         ; SYS_exit
    mov rdi, 0          ; exit code 0
    syscall

section .rodata
msg: db 'J4: bureau pret', 10
msg_len equ $ - msg

section .bss
buf: resb 256
