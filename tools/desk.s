; desk.s — bureau userland J4 (bureau avance).
;
; Dessine le cadre de la fenetre active (fond sombre + bordure lumineuse)
; par une succession de SYS_write, puis se termine.
; (Le rendu graphique plein — VESA framebuffer — arrive au lot ABI ; ce
; squelette userland exerce le pipeline multi-write + cadre.)

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
    syscall             ; cadre de la fenetre active

    mov rax, 60         ; SYS_exit
    mov rdi, 0          ; exit code 0
    syscall

section .rodata
msg1: db 'J4: bureau pret', 10
msg1_len equ $ - msg1
frame: db '+------------------------+', 10, '| Bureau                 |', 10, '+------------------------+', 10
frame_len equ $ - frame

section .bss
buf: resb 256
