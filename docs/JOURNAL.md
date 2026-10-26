# Journal de développement

Une entrée de 5 lignes par itération : ce qui marche, ce qui reste, les
décisions prises.

---

## 2026-10-26 — J0 : cadrage et outillage

- Workspace Cargo `no_std`, zéro dépendance externe.
- Makefile : `build`, `iso`, `run`, `run-headless`, `test`, `clean`.
- Cible : `x86_64-unknown-linux-gnu` + `RUSTFLAGS` freestanding
  (`-nostdlib`, `-static`, `-no-pie`, script de link dédié).
- Décision : GRUB/multiboot2 pour démarrer vite, UEFI en J2.
- Reste : CI distante (GitHub Actions) à ajouter.

## 2026-10-26 — J1 : boot et console

- `boot.s` : multiboot2 → mode long 64 bits, tables de pages 1 Gio
  identity-mappées (2 Mio), PAE + LME + PG, GDT 64 bits.
- Console double : UART 16550 (COM1) + VGA texte 80×25.
- `panic` minimal qui écrit sur les deux consoles.
- Bugs corrigés : CR3 non chargé (page fault réservé), SSE non activé
  (`#UD` sur `movaps`).
- Reste : memory map, IDT/exceptions, APIC/timer, UEFI.

---

## Modèle d'entrée

```
## AAAA-MM-JJ — JX : titre

- Ce qui a été fait.
- Ce qui a été vérifié (commande + résultat).
- Décisions prises.
- Ce qui reste.
- Risques / dette.
```
