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

## 2026-10-26 — J2.2 : carte mémoire lue depuis la MBI

- `kernel/src/mbi.rs` : structures Multiboot2 (en-tête, tags 1 et 6, entrées
  de carte) et itérateur de tags.
- `kernel/src/mm/mod.rs` : balayage de la carte, synthèse par type, détail
  région par région.
- Affichage au boot : synthèse (régions disponibles/réservées, RAM totale)
  puis le détail complet.
- Vérifié : `make test` (BIOS, 7 régions) et `make test-uefi` (ISO + ESP,
  21 régions sous UEFI — GRUB y expose ACPI NVS et reclaimable).
- Piège trouvé : **GRUB 2.12 émet un tag MEMORY_INFO (type 1) de 9 octets**
  au lieu des 16 de la spécification. Lire `mem_lower`/`mem_upper` à
  `off + 8`/`off + 12` renvoyait du remplissage (des centaines de Mio de
  « RAM sous 1 Mio »). `MemInfo::read` n'accepte plus que la taille exacte
  et signale l'absence plutôt que d'afficher des zéros trompeurs.
- Le tag 6 (carte mémoire) est la source réellement exploitée ; il est
  conforme et suffisant.
- Reste : paging higher-half, allocateur physique, IDT/exceptions.

## 2026-10-26 — J2.3 : tables de pages 4 niveaux, identity + higher-half

- `kernel/src/mm/paging.rs` : descripteurs, tables, construction des tables
  en Rust, résolution d'adresse, chargement de CR3, vérification.
- `kernel/src/runtime.rs` : `memset`/`memcpy`/`memmove`/`memcmp`, absents
  d'un binaire `no_std` mais exigés par rustc dès qu'un tableau est
  initialisé — le noyau compilait mais ne se liait pas.
- `kernel/src/boot.s` : EFER reçoit **NXE** (bit 11) en plus de LME.
- Tables couvertes : identity `0..1 Gio` et higher-half
  `0xFFFFFFFF80000000..+1 Gio`, en pages de 2 Mio ; plus une page de 4 Kio
  dans un slot de PML4 libre.
- Vérifié : `make test` et `make test-uefi` passent. L'activation est
  prouvée par écriture par l'adresse virtuelle de test et relecture par
  l'adresse physique — même valeur `0x123456789abcdef0` par les deux
  voies, ce qui prouve que le parcours 4 niveaux résout bien la page.
- Trois pièges trouvés, tous les trois invisibles dans le code correct :
  1. `pml4_index(0x80000000)` = **0** : la page de test écrasait
     l'identity map. Il faut une adresse dont l'index de PML4 est libre
     (ici `0x8000000000`, index 1).
  2. `pdpt_index(0xFFFFFFFF80000000)` = **511**, pas 0 : les bits 38..30
     du higher-half sont tous à 1. Écrire l'index 0 en dur laissait le
     higher-half dans le vide. Les indices se calculent depuis l'adresse,
     jamais en dur.
  3. **EFER.NXE sans lequel le bit 63 d'un descripteur est réservé** :
     poser le bit NX déclenchait une faute « reserved bit violation »
     (code d'erreur `0x0a` : absent, écriture, RSV) au premier accès.
- Le bit Global n'est posé que sur les pages mappées ; sur un descripteur
  de table, les bits 8 à 11 sont réservés.
- Reste : relier le noyau en higher-half (il tourne encore à 1 Mio),
  allocateur physique, IDT/exceptions.

## 2026-10-27 — J2.3b : noyau lié et exécuté en higher-half
- `boot.s` : saut en higher-half par `movabs` + `call *%rax` — un `call rel32`
  ne peut pas atteindre le higher-half (> 4 GiB). AT&T corrigé : `jmp *%rax`.
- `linker.ld` : `AT(. - HIGHER_HALF)` donne des LMA physiques basses (< 4 GiB)
  pour GRUB ; `KEEP()` empêche `--gc-sections` de pétrir le header multiboot2.
- `main.rs` : preuve d'exécution — LINK_GUARD a une adresse de lien >= HIGHER_HALF,
  lue volatil par l'adresse virtuelle (sinon #PF avant l'impression).
- Vérifié : `make test` (BIOS) et `make test-uefi` (ISO + ESP) — `OK (lien=…,
  phys=…)` avec phys = lien - HIGHER_HALF. Piège : `code-model=kernel` (pas
  `relocation-model=large`, qui n'existe pas) → R_X86_64_64 vs R_X86_64_32.

## 2026-10-27 — J2.4 : allocateur physique bitmap
- `kernel/src/mm/phys.rs` : bitmap 1 bit/page (4 Kio), plafond 4 GiB. `init`
  scanne la carte MBI, libère les régions AVAILABLE, réserve noyau + MBI.
- API : `alloc_page`/`free_page` sûres (unsafe contenu dans le module), recherche
  circulaire amortie via `NEXT_WORD` + `(!mot).trailing_zeros()`.
- Test : alloue 100 Mio (25600 pages) sur un réservoir de 256 entrées, libère,
  vérifie le compte libre revient exactement (ex. 130728 -> 130728 pages).
- Vérifié : `make test` + `make test-uefi` passent, gate J2.4 ajouté.
- Bug : `bits[w].trailing_zeros()` trouve le plus bas bit UTILISÉ — il faut
  inverser (`!bits[w]`) pour obtenir le plus bas bit LIBRE.

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
