; probe_j4a.s — sonde J4-A : lseek / getdents64 / mprotect / munmap / uname
;
; Enchaine les syscalls et ecrit un marqueur par etape sur fd 1.
; Un echec s'arrete la (exit code 1) : le log serie montre le dernier
; marqueur atteint.

global _start

%define SYS_WRITE      1
%define SYS_LSEEK      8
%define SYS_MMAP       9
%define SYS_MPROTECT   10
%define SYS_MUNMAP     11
%define SYS_UNAME      63
%define SYS_GETDENTS64 217
%define SYS_EXIT       60

%define PROT_READ      0x1
%define PROT_WRITE     0x2
%define PROT_EXEC      0x4

%define MAP_PRIVATE    0x02
%define MAP_ANONYMOUS  0x20

section .text
_start:
    ; ---------------------------------------------------------------
    ; 1. uname : ecrire une structure utsname (6 x 65 = 390 octets)
    ;    dans une mmap anonyme, puis lire le champ sysname.
    ; ---------------------------------------------------------------
    mov rax, SYS_MMAP
    xor rdi, rdi                 ; addr = 0 (le noyau choisit)
    mov rsi, 0x1000              ; length = 1 page
    mov rdx, PROT_READ | PROT_WRITE
    mov r10, MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1                   ; fd = -1
    xor r9, r9                   ; offset = 0
    syscall
    cmp rax, 0
    jl .fail
    mov r12, rax                 ; r12 = base mmap

    ; uname(buf)
    mov rax, SYS_UNAME
    mov rdi, r12
    syscall
    cmp rax, 0
    jne .fail

    ; sysname[0] doit etre un caractere imprimable (pas 0)
    movzx eax, byte [r12]
    test al, al
    jz .fail
    cmp al, 0x40
    jl .fail

    ; ---------------------------------------------------------------
    ; 2. lseek : ouvrir le flux via un mmap n'a pas de fd. On teste
    ;    lseek sur fd=1 (stdout) : ESPIPE attendu (-29) car ce n'est
    ;    pas un fichier seekable. Le syscall doit repondre, pas -ENOSYS.
    ; ---------------------------------------------------------------
    mov rax, SYS_LSEEK
    mov rdi, 1
    xor rsi, rsi
    xor rdx, rdx
    syscall
    cmp rax, -38                 ; -ENOSYS = pas implemente
    je .fail
    ; toute autre reponse (y compris -ESPIPE) prouve le chemin

    ; ---------------------------------------------------------------
    ; 3. mprotect : la page mmappee doit passer de RW a R seulement.
    ; ---------------------------------------------------------------
    mov rax, SYS_MPROTECT
    mov rdi, r12
    mov rsi, 0x1000
    mov rdx, PROT_READ
    syscall
    cmp rax, 0
    jne .fail

    ; ---------------------------------------------------------------
    ; 4. munmap : liberer la page.
    ; ---------------------------------------------------------------
    mov rax, SYS_MUNMAP
    mov rdi, r12
    mov rsi, 0x1000
    syscall
    cmp rax, 0
    jne .fail

    ; ---------------------------------------------------------------
    ; 5. getdents64 : ouvrir /proc puis lire le repertoire. Le noyau
    ;    doit renvoyer au moins une entree (ou 0 si vide).
    ; ---------------------------------------------------------------
    ; openat(AT_FDCWD, "/proc", O_RDONLY)
    mov rax, 257
    mov rdi, -100
    lea rsi, [rel proc_path]
    xor rdx, rdx
    syscall
    cmp rax, 0
    jl .fail
    mov r13, rax                 ; r13 = fd /proc

    ; mmap un buffer pour getdents64
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
    mov r14, rax                 ; r14 = buffer

    mov rax, SYS_GETDENTS64
    mov rdi, r13
    mov rsi, r14
    mov rdx, 0x1000
    syscall
    cmp rax, 0
    jl .fail
    mov r15, rax                 ; r15 = octets lus (>= 0)

    ; close(fd)
    mov rax, 3
    mov rdi, r13
    syscall

    ; ---------------------------------------------------------------
    ; Marqueur final : les 5 etapes ont passe.
    ; ---------------------------------------------------------------
    lea rsi, [rel ok_msg]
    mov rdx, ok_msg_len
    call write

    ; exit(0)
    mov rax, SYS_EXIT
    xor rdi, rdi
    syscall

.fail:
    lea rsi, [rel fail_msg]
    mov rdx, fail_msg_len
    call write
    mov rax, SYS_EXIT
    mov rdi, 1
    syscall

; write(fd=1, buf=rsi, len=rdx)
write:
    mov rax, SYS_WRITE
    mov rdi, 1
    syscall
    ret

section .rodata
ok_msg:     db "probe_j4a: OK", 10
ok_msg_len  equ $ - ok_msg
fail_msg:   db "probe_j4a: ECHEC", 10
fail_msg_len equ $ - fail_msg
proc_path:  db "/proc", 0
