# XILI

Noyau x86-64 écrit en Rust, compatible ABI Linux, destiné à réutiliser
l'écosystème existant (musl, Alpine, Wayland, Mesa, Wine).

> Nom : **XILI**.

---

## État actuel

**Jalon J1 atteint** : le noyau boote, passe en mode long 64 bits,
initialise une console (série + VGA texte) et affiche les informations
multiboot2.

**J2.1 atteint** : le noyau boote aussi en **UEFI**, sous le firmware
OVMF, sans BIOS — par ISO hybride et par ESP disque. C'est le critère
d'acceptation UEFI du jalon J2 ; le chargeur reste GRUB.

```
XILI - noyau x86-64 (Rust)
--------------------------------------
magic multiboot2 : 0x36d76289
adresse mbi      : 0x001004e8
Boot multiboot2  : OK

J1: noyau charge, mode long actif, console OK.
```

---

## Prérequis

| Outil | Rôle |
|---|---|
| `rustc` + `cargo` (stable) | compilation du noyau |
| `qemu-system-x86_64` | exécution |
| `grub-mkrescue`, `xorriso`, `mtools` | fabrication de l'ISO |
| `make` | pilotage |

Aucune dépendance externe (crate) n'est nécessaire : le noyau est
entièrement `no_std` et n'utilise que `core`.

---

## Commandes

```bash
make build        # compile le noyau
make iso          # construit xili.iso (BIOS)
make iso-uefi     # construit xili-uefi.iso (UEFI, ISO hybride)
make esp          # construit xili-esp.img (UEFI, vraie ESP)
make run          # boote dans QEMU avec affichage + série
make run-headless # boote sans affichage (série dans le terminal)
make run-uefi     # boote l'ISO UEFI sous OVMF
make run-esp      # boote l'ESP sous OVMF
make test         # BIOS : vérifie que le noyau atteint J1
make test-uefi     # UEFI : les deux chemins sous OVMF
make clean
```

---

## Arborescence

```
os/
├── Cargo.toml              # workspace
├── Makefile                # build, ISO, QEMU, test
├── .cargo/config.toml      # cible de compilation
├── boot/grub.cfg           # configuration GRUB (BIOS)
├── boot/grub-uefi.cfg      # configuration GRUB (UEFI)
├── tools/mk-esp.sh         # construit une ESP UEFI bootable
├── kernel/
│   ├── Cargo.toml
│   ├── linker.ld           # layout mémoire du noyau
│   └── src/
│       ├── main.rs         # point d'entrée Rust
│       ├── boot.s          # multiboot2 + passage en mode long
│       ├── console.rs      # sortie double (série + VGA)
│       ├── serial.rs       # UART 16550 (COM1)
│       └── vga.rs          # texte 80x25 à 0xB8000
└── docs/
    ├── ARCHITECTURE.md     # décisions structurantes
    ├── ROADMAP.md          # jalons J0 → J10
    ├── ABI.md              # contrat d'appels système
    └── AI-WORKFLOW.md      # méthode de travail IA + humain
```

---

## Chaîne de boot actuelle

```
BIOS   → GRUB depuis le MBR (multiboot2) → boot.s (32 bits)
UEFI   → GRUB depuis l'ESP (multiboot2)   → boot.s (32 bits)
          → tables de pages 1 Gio identity-mappées
          → PAE + LME + PG → mode long 64 bits
          → SSE activé → kernel_main(magic, mbi)
```

GRUB est utilisé **temporairement**. La cible est un chargeur UEFI
(Limine ou chargeur maison) au jalon J2 — GRUB UEFI ne sert ici qu'à
prouver que le noyau démarre sous un firmware UEFI.

---

## Règles du projet

1. **Tout commit doit booter.** `make test` est la porte d'entrée.
2. **Pas de dépendance non maîtrisée** dans le noyau.
3. **Une seule machine cible** à la fois pour le matériel réel.
4. **L'ABI Linux est le contrat** : toute décision qui s'en écarte doit
   être écrite dans `docs/ARCHITECTURE.md`.
5. **Licence** : GPLv2 pour le noyau (à confirmer), MIT/LGPL pour les
   bibliothèques internes.

---

## Prochaine étape

**J2 — mémoire** : parseur de memory map (UEFI/E820), paging higher-half,
allocateur physique, IDT/exceptions. Voir `docs/ROADMAP.md`.
