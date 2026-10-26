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

## 2026-10-26 — J2.1 : boot UEFI sous OVMF

- Deux chemins UEFI prouvés, le noyau atteint J1 dans les deux : ISO hybride
  (`grub-mkrescue`) et ESP disque (`tools/mk-esp.sh`).
- `make test-uefi` boote les deux sous OVMF et exige le marqueur J1.
- Trois pièges documentés dans `tools/mk-esp.sh` : `mformat` écrase le MBR
  quand il est lancé sur le disque entier ; `grub-mkstandalone` a besoin de
  `--modules=part_msdos` pour parser la table de partitions ; la config GRUB
  doit être passée en chemin relatif pour être embarquée au bon endroit.
- Décision : GRUB reste le chargeur temporaire. Limine ou chargeur maison au
  prochain jalon — GRUB UEFI ne sert qu'à prouver le démarrage UEFI.
- Reste : memory map UEFI/E820, paging higher-half, IDT/exceptions.

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
