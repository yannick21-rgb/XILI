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
# Après handle_irq : add $16, %rsp (retirer vector+error_code), iretq
irq_common:
    mov %rsp, %rdi            # rdi = &frame
    call handle_irq
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
