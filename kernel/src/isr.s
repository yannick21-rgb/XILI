/* ============================================================
 * isr.s — stubs d'interruption (vecteurs 0..31)
 *
 * Chaque stub uniformise la pile à :
 *   [vector][error_code][rip][cs][rflags]
 *
 * Exceptions SANS code d'erreur : le stub pousse un 0 factice.
 * Exceptions AVEC code d'erreur : le CPU l'a déjà poussé.
 *
 * Le point commun `isr_common` passe rsp (→ cadre) dans rdi et
 * appelle `handle_exception` (Rust, extern "C", → !).
 * ============================================================ */

    .section .text
    .align 16

# --- Point commun ---
isr_common:
    mov %rsp, %rdi            # rdi = &ExceptionFrame
    call handle_exception

1:  hlt
    jmp 1b

# --- Exception sans code d'erreur : pousser un 0 factice ---
    .macro NOERR vec
    .global isr_stub_\vec
isr_stub_\vec:
    pushq $0
    pushq $\vec
    jmp isr_common
    .endm

# --- Exception avec code d'erreur (déjà poussé par le CPU) ---
    .macro ERR vec
    .global isr_stub_\vec
isr_stub_\vec:
    pushq $\vec
    jmp isr_common
    .endm

NOERR 0   /* #DE  Divide Error                */
NOERR 1   /* #DB  Debug                      */
NOERR 2   /* NMI                            */
NOERR 3   /* #BP  Breakpoint                  */
NOERR 4   /* #OF  Overflow                   */
NOERR 5   /* #BR  BOUND Range Exceeded        */
NOERR 6   /* #UD  Invalid Opcode             */
NOERR 7   /* #NM  Device Not Available        */
NOERR 8   /* #DF  Double Fault (IST1)        */
NOERR 9   /* FS Error                        */
ERR  10   /* #TS  Invalid TSS                 */
ERR  11   /* #NP  Segment Not Present         */
ERR  12   /* #SS  Stack-Segment Fault         */
ERR  13   /* #GP  General Protection Fault    */
ERR  14   /* #PF  Page Fault                  */
NOERR 16  /* #MF  x87 FPU                    */
ERR  17   /* #AC  Alignment Check            */
NOERR 18  /* #MC  Machine Check              */
NOERR 19  /* #XF  SIMD FP Exception           */
NOERR 20  /* #VE  Virtualization Exception   */
ERR  21   /* #VC  Virtual Callable           */
NOERR 22  /* #NPF  Nested Page Fault          */
NOERR 23  /* 23 (reserved)                    */
NOERR 24  /* 24 (reserved)                    */
NOERR 25  /* 25 (reserved)                    */
NOERR 26  /* 26 (reserved)                    */
NOERR 27  /* 27 (reserved)                    */
NOERR 28  /* #HP  Hypervisor Injection        */
ERR  29   /* #HV  Hypervisor                 */
ERR  30   /* #VM  VMX Preemption             */
NOERR 31  /* 31 (reserved)                    */

/* ============================================================
 * Stubs d'interruptions matérielles (vecteurs 32..47)
 *
 * Le CPU n'envoie PAS de code d'erreur pour les IRQ → on pousse
 * un 0 factice + le numéro du vecteur, exactement comme NOERR.
 *
 * Contrairement aux exceptions, les IRQ doivent *revenir* :
 * `irq_common` appelle `handle_irq`, puis `iretq` restaure
 * RIP/CS/RFLAGS. Le handler envoie un EOI au PIC avant de
 * retourner.
 * ============================================================ */

    .section .text
    .align 16

# --- Point commun IRQ : appelle handle_irq, puis iretq ---
# Entrée : rsp → [vector][error_code][rip][cs][rflags]
# handle_irq retourne dans rax le RSP de la prochaine tâche
# (switch contextuel) → mov %rax, %rsp, pop GPRs, iretq
#
# J2.8 : sauvegarde et restauration des 15 registres généraux.
# Sans cela, handle_timer et schedule() clobblent les registres
# de la tâche interrompue → contexte corrompu à la reprise.
irq_common:
    # --- Sauvegarde des GPRs (15 registres, 120 octets) ---
    # Ordre push : r15 (adresse la plus haute) → rax (la plus basse)
    # Le bloc [r15..rax] est en dessous de l'interrupt frame sur la pile.
    pushq %r15
    pushq %r14
    pushq %r13
    pushq %r12
    pushq %r11
    pushq %r10
    pushq %r9
    pushq %r8
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %rdx
    pushq %rcx
    pushq %rbx
    pushq %rax

    # rdi = &gpr_state (= RSP courant, pointe vers le bloc sauvé)
    mov %rsp, %rdi
    call handle_irq           # rax = next_rsp (switch ou non)
    mov %rax, %rsp            # switch de pile vers la prochaine tâche

    # --- Restauration des GPRs de la tâche suivante ---
    # Ordre pop : rax (adresse la plus basse) → r15 (la plus haute)
    # Revers du push : chaque pop dépile dans l'ordre inverse.
    popq %rax
    popq %rbx
    popq %rcx
    popq %rdx
    popq %rsi
    popq %rdi
    popq %rbp
    popq %r8
    popq %r9
    popq %r10
    popq %r11
    popq %r12
    popq %r13
    popq %r14
    popq %r15

    # rsp → [vector][error_code][rip][cs][rflags] de la tâche suivante
    add $16, %rsp             # retirer vector + error_code factice
    iretq

# --- Stub IRQ : même pattern que NOERR, mais → irq_common ---
    .macro IRQ vec
    .global irq_stub_\vec
irq_stub_\vec:
    pushq $0                  # code d'erreur factice
    pushq $\vec
    jmp irq_common
    .endm

IRQ 32   /* timer (IRQ 0)           */
IRQ 33   /* clavier (IRQ 1)         */
IRQ 34   /* (réservé / cascade)     */
IRQ 35   /* COM2 / IRQ 3            */
IRQ 36   /* COM3 / IRQ 4            */
IRQ 37   /* COM4 / IRQ 5            */
IRQ 38   /* floppy (IRQ 6)          */
IRQ 39   /* fin de session (IRQ 7)  */
IRQ 40   /* esclave (IRQ 8)         */
IRQ 41   /* IRQ 9                   */
IRQ 42   /* IRQ 10                  */
IRQ 43   /* IRQ 11                  */
IRQ 44   /* IRQ 12 (souris)         */
IRQ 45   /* IRQ 13                  */
IRQ 46   /* IRQ 14 (disque)         */
IRQ 47   /* IRQ 15 (disque)         */

/* ============================================================
 * syscall_entry — point d'entrée SYSCALL (J3.2)
 *
 * Le CPU entre ici en anneau 0, sur la pile de l'appelant (anneau 3).
 * RCX = RIP utilisateur, R11 = RFLAGS utilisateur (sauvés par syscall).
 * RAX = numéro de syscall, RDI/RSI/RDX/R10/R8/R9 = arguments.
 *
 * Le handler :
 *   1. Sauvegarde RSP utilisateur dans R15, switch pile noyau
 *   2. Pousse les 15 GPRs (même ordre que irq_common)
 *   3. Appelle handle_syscall(gpr_ptr) — lit/modifie le bloc
 *   4. Restaure les GPRs (RCX=RIP, R11=RFLAGS pour sysret)
 *   5. Restore user RSP
 *   6. `sysret` → retour en anneau 3
 * ============================================================ */

/* Pile noyau pour syscalls — 16 Kio dans la BSS. */
.section .bss
.align 16
.global syscall_stack
syscall_stack:
    .skip 16384
syscall_stack_top:

/* Drapeaux pour J3.2 — syscall_entry lit SYSCALL_TEST_DONE
 * pour décider entre sysret (anneau 3) et iretq (anneau 0). */
.align 8
.global SYSCALL_TEST_DONE
SYSCALL_TEST_DONE:
    .byte 0

.global EXIT_REQUESTED
EXIT_REQUESTED:
    .byte 0

/* Stub d'entrée. */
.section .text
.align 16
.extern handle_syscall

.global syscall_entry
syscall_entry:
    # Sauvegarder user RSP avant de changer de pile.
    movq %rsp, %r15                # r15 = user RSP (temporaire)
    leaq syscall_stack_top(%rip), %rsp  # switch vers pile noyau

    # Pousser les 15 GPRs — même ordre que irq_common (r15 → rax).
    # r15 contient user RSP (pour le restaurer avant sysret).
    pushq %r15                       # user RSP (offset 112)
    pushq %r14
    pushq %r13
    pushq %r12
    pushq %r11                       # user RFLAGS (sauvé par syscall)
    pushq %r10
    pushq %r9
    pushq %r8
    pushq %rbp
    pushq %rdi                       # arg1
    pushq %rsi                       # arg2
    pushq %rdx                       # arg3
    pushq %rcx                       # user RIP (sauvé par syscall)
    pushq %rbx
    pushq %rax                       # syscall number / return value

    # Alignement de la pile sur 16 octets avant l'appel.
    # 15 pushes = 120 octets = 8 (mod 16) ; un dummy supplementaire
    # donne 128 octets = 0 (mod 16). Apres `call`, RSP est 8 (mod 16)
    # a l'entree de handle_syscall ; son prologue (push rbp + push rbx
    # + sub 0x58 = 104 = 8 mod 16) ramene alors RSP a 0 (mod 16), ce
    # que les instructions SSE (movdqa) attendent.
    pushq $0
    movq %rsp, %rdi                  # rdi = gpr_ptr (arg de handle_syscall)
    addq $8, %rdi                    # sauter le dummy pour pointer sur rax
    call handle_syscall
    addq $8, %rsp                    # retirer le dummy

    # Restore GPRs dans l'ordre inverse (rax → r15).
    # rcx = user RIP, r11 = user RFLAGS — utilisés par sysret/iretq.
    popq %rax                        # return value
    popq %rbx
    popq %rcx                        # user RIP
    popq %rdx
    popq %rsi
    popq %rdi
    popq %rbp
    popq %r8
    popq %r9
    popq %r10
    popq %r11                        # user RFLAGS
    popq %r12
    popq %r13
    popq %r14
    popq %r15                        # user RSP

    # Vérifier si un retour en anneau 0 est demandé.
    # SYSCALL_TEST_DONE (J3.2 — test done) et EXIT_REQUESTED (J3.3 —
    # sys_exit ELF) sont fixés par handle_syscall. Dans les deux cas,
    # RCX pointe vers une fonction ring-0 et R11 contient les RFLAGS.
    cmpb $0, SYSCALL_TEST_DONE(%rip)
    jne ring0_return
    cmpb $0, EXIT_REQUESTED(%rip)
    je sysret_return

    # Fall-through : SYSCALL_TEST_DONE = 0 mais EXIT_REQUESTED != 0.
    # (J3.3 — sys_exit a été appelé par le binaire ELF.)
ring0_return:
    # Consommer les drapeaux de sortie (J3.2 / J3.3) avant de sauter :
    # sans reset, le prochain syscall serait re-intercepte en R0.
    movb $0, SYSCALL_TEST_DONE(%rip)
    movb $0, EXIT_REQUESTED(%rip)
    # Retour en anneau 0 — jmp direct vers la fonction ring-0.
    # iretq non utilisable : QEMU TCG 8.2 helper_syscall charge CS depuis
    # STAR[47:32] (KERNEL_DS=0x10) au lieu de STAR[63:48] (KERNEL_CS=0x38),
    # puis iretq #GP car le CS courant (0x10, segment données) n'est pas
    # un segment code. Contournement : jmp direct vers la fonction ring-0.
    jmp *%rcx

sysret_return:
    # Retour normal en anneau 3 via sysret (avec REX.W pour 64-bit).
    movq %r15, %rsp                  # restore user stack
    sysretq

/* ============================================================
 * entry_ring3 — entre en anneau 3 depuis l'anneau 0 (J3.2 test)
 *
 * Construit un cadre iret pour passer de l'anneau 0 à l'anneau 3 :
 *   RDI = ring-3 entry point (user RIP)
 *   RSI = ring-3 stack top
 *
 * Stack layout (iret pops from low to high address) :
 *   [rsp]    = rip   (user entry)
 *   [rsp+8]  = cs    (USER_CS | RPL=3 = 0x2B)
 *   [rsp+16] = rflags (IF=0 — pas d'IRQ pendant le test)
 *   [rsp+24] = rsp   (user stack top)
 *   [rsp+32] = ss    (USER_DS | RPL=3 = 0x33)
 * ============================================================ */

.global entry_ring3
entry_ring3:
    # Préparer la transition R0→R3 : CS/SS sont poussés pour iretq,
    # mais les segments data (DS/ES/FS/GS) doivent aussi être valides
    # en R3, sinon le CPU peut lever un #GP (GDT/LDT, index 0).
    movq $0x30, %rax           # USER_DS (DPL=3)
    movq %rax, %ds
    movq %rax, %es
    movq %rax, %fs
    movq %rax, %gs

    pushq $0x33                      # User SS (0x30 | RPL=3)
    pushq %rsi                        # User RSP (stack top)
    pushq $0x0                        # User RFLAGS (IF=0)
    pushq $0x2B                       # User CS (0x28 | RPL=3)
    pushq %rdi                        # User RIP (entry point)
    iretq

/* ============================================================
 * ring3_test_func — code utilisateur de test (J3.2)
 *
 * S'exécute en anneau 3. Appelle `syscall` pour écrire 'S'
 * sur le port serie, puis appelle le syscall 0x1FF (test done).
 *
 * Le message "S" est stocké dans .rodata, accessible depuis
 * l'anneau 3 via l'identity map (0..1 Gio) ou le higher-half.
 * ============================================================ */

.section .rodata
.align 1
ring3_test_msg:
    .byte 'S'

.section .text
.global ring3_test_func
ring3_test_func:
    # sys_write(1, &msg, 1) — ecrire 'S' sur le serie
    movq $1, %rax                     # sys_write
    movq $1, %rdi                     # fd = 1 (stdout/serial)
    leaq ring3_test_msg(%rip), %rsi   # buf = "S"
    movq $1, %rdx                     # count = 1
    syscall

    # sys_getpid() -- test de valeur de retour en anneau 3
    movq $39, %rax                    # sys_getpid
    syscall

    # syscall 0x1FF -- test done: le handler ecrit le marqueur
    # J3.2, modifie rcx (-> ring0_after_test) + r11 (-> RFLAGS),
    # puis syscall_entry utilise iretq au lieu de sysret
    # pour revenir en anneau 0.
    movq $0x1FF, %rax
    syscall

    # Ne devrait pas arriver -- le handler retourne en anneau 0
1:
    hlt
    jmp 1b
