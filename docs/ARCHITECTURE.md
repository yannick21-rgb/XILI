# Architecture — XILI

Document de référence des décisions structurantes. Toute décision qui
contredit ce document doit être discutée puis consignée ici.

---

## 1. Vision

Un OS **libre**, **robuste** et **fluide**, capable d'exécuter :

- les applications **Linux** nativement (ABI Linux) ;
- la majorité des applications **Windows** via **Wine/Proton** ;
- un environnement graphique **Wayland** avec un compositeur maîtrisé.

Le noyau est écrit de zéro. Tout le reste est réutilisé, adapté ou assemblé.

---

## 2. Objectifs et non-objectifs

### Objectifs

| # | Objectif | Critère de réussite |
|---|---|---|
| O1 | Noyau x86-64 + compatibilité 32 bits | exécute un binaire ELF 32 bits |
| O2 | ABI Linux suffisante | busybox, Alpine `apk`, bash, Python |
| O3 | Réseau | `apk update`, `ssh`, DNS |
| O4 | Graphique | compositeur Wayland, Mesa (llvmpipe puis GPU) |
| O5 | Compatibilité Windows | une application Win32/Win64 via Wine |
| O6 | Robustesse | panic isolé, FS transactionnel, mises à jour atomiques |
| O7 | Fluidité | 60 fps stables sur le compositeur, vsync, VRR |
| O8 | Licence libre | noyau GPLv2, pas de blob obligatoire |

### Non-objectifs (assumés)

- Compatibilité binaire avec les **pilotes noyau Windows** (`.sys`).
- Anti-cheat kernel-level (EAC, BattlEye, Vanguard).
- Support de « n'importe quel PC » : une machine cible à la fois.
- Docker/Flatpak avant J7.
- Micro-noyau pur.

---

## 3. Décisions structurantes

| # | Décision | Choix | Raison | Alternative rejetée |
|---|---|---|---|---|
| D1 | Langage noyau | **Rust** (`no_std`) | sûreté mémoire, robustesse | C (plus de bugs mémoire), C++ (complexité) |
| D2 | Architecture | **x86-64 + IA32** | ABI Linux, Wine/WoW64, matériel courant | ARM64 (moins d'apps), RISC-V (pas de Wine mature) |
| D3 | ABI | **Linux x86-64** | réutiliser musl/Alpine/Wine sans portage | ABI original (portage massif) |
| D4 | Structure noyau | **Monolithique modulaire** | simplicité, performance, un seul développeur | micro-noyau (IPC partout, 2× le travail) |
| D5 | Boot | **GRUB/multiboot2** → **UEFI** | démarrer vite, migrer ensuite | bootloader maison (semaines perdues) |
| D6 | Userland | **musl + Alpine** | léger, statique possible, `apk` | glibc/Debian (plus lourd), from scratch |
| D7 | Graphique | **Wayland + compositeur maison** | contrôle de la fluidité | X11 (legacy), serveur tiers (moins de contrôle) |
| D8 | Windows | **Wine/Proton** | seule voie réaliste | réimplémentation NT (ReactOS, 30 ans) |
| D9 | FS | **ext2/FAT32 d'abord, COW ensuite** | simplicité puis fiabilité | ZFS/Btrfs immédiat (trop tôt) |
| D10 | Licence | **GPLv2** noyau, MIT/LGPL bibliothèques | liberté, contributions en retour | propriétaire, BSD (moins de réciprocité) |

---

## 4. Vue en couches

```
┌──────────────────────────────────────────────────────────────┐
│ Applications Windows (PE)   │  Applications Linux (ELF)      │
│ Wine / Proton / DXVK        │  natif                         │
├──────────────────────────────────────────────────────────────┤
│ Wayland + compositeur + Mesa + toolkit                       │
├──────────────────────────────────────────────────────────────┤
│ Interface d'appels système (ABI Linux x86-64)                │
├──────────────────────────────────────────────────────────────┤
│ VFS │ Sched │ VM │ Net │ IPC │ Sécurité │ Time                │
├──────────────────────────────────────────────────────────────┤
│ Drivers : PCI, AHCI/NVMe, virtio, e1000, fb, HDA, evdev      │
├──────────────────────────────────────────────────────────────┤
│ HAL : paging, IRQ, APIC, timers, SMP, ACPI                   │
├──────────────────────────────────────────────────────────────┤
│ Boot : GRUB/multiboot2 (aujourd'hui) → UEFI (J2)             │
└──────────────────────────────────────────────────────────────┘
```

Règle interne : chaque couche expose une **interface stable** en appels
de fonction. Un driver ne connaît jamais les détails du scheduler.

---

## 5. Chaîne de boot

### Aujourd'hui (J1)

1. GRUB charge l'ELF à 1 Mio et saute à `_start` en mode protégé 32 bits.
2. `boot.s` construit PML4 → PDPT → PD (512 pages de 2 Mio = 1 Gio).
3. Active PAE, LME, PG, charge la GDT 64 bits, saute en mode long.
4. Active SSE (`CR0.EM=0`, `CR4.OSFXSR|OSXMMEXCPT`), initialise la pile.
5. Appelle `kernel_main(magic, mbi)`.

### Cible (J2)

- Chargeur **UEFI** (Limine ou maison) : pas de BIOS, Secure Boot plus tard.
- Récupération de la **memory map** UEFI et du framebuffer.
- Module `boot` séparé de l'architecture.

---

## 6. Mémoire

| Élément | Décision |
|---|---|
| Modèle | pagination 4 niveaux, 4 Kio et 2 Mio |
| Noyau | mappé dans tous les espaces (higher-half à terme) |
| Utilisateur | espace séparé, NX, W^X |
| Allocateur physique | bitmap puis buddy |
| Allocateur noyau | slab (`kmalloc`) |
| Utilisateur | `mmap`, `brk`, `mremap`, COW |
| Swap | plus tard (J6) |

Contraintes :
- pas d'allocation dans les chemins IRQ ;
- `alloc` du noyau derrière un verrou par CPU ;
- KASAN/KASLR activés dès que possible.

---

## 7. Processus et threads

- `Task` = PCB + espace d'adressage + table de fichiers + handles.
- Threads noyau et utilisateur séparés ; `clone` gère les deux.
- Ordonnanceur : round-robin à priorité → MLFQ → CFS-like.
- Préemption par timer (APIC), pas coopérative.
- `fork` = COW ; `execve` = remplacement d'image ; `exit` = reparentage.
- TLS via `arch_prctl(ARCH_SET_FS)` et MSR `FS_BASE`.
- 32 bits : `int 0x80`, `IA32` ELF, `vsyscall` émulation (J9).

---

## 8. ABI et appels système

- Numéros et structures **Linux x86-64** (`include/uapi/`).
- Convention : `rax` = numéro, args `rdi, rsi, rdx, r10, r8, r9`, retour `rax`.
- `errno` = valeur négative dans `rax` (`-EINVAL`), comme Linux.
- `syscall`/`sysret` avec `MSR_LSTAR`, `MSR_STAR`, `MSR_SFMASK`.
- Détail complet : `docs/ABI.md`.

---

## 9. VFS

```
VFS (inode, dentry, file, superblock)
├── ext2 / ext4 (lecture puis écriture)
├── FAT32 (échange, EFI)
├── procfs (/proc)
├── sysfs (/sys, minimal)
├── devtmpfs (/dev)
├── devpts (/dev/pts)
└── tmpfs/ramfs
```

- Cache de blocs + cache de pages.
- Journalisation puis COW (J6) pour la robustesse.
- Montage, `pivot_root`, `chroot`.

---

## 10. Drivers

Ordre d'implémentation :

1. **virtio** (bloc, réseau, console) — tout marche en QEMU.
2. PCI/PCIe (énumération, BAR, MSI-X).
3. AHCI (SATA), NVMe.
4. e1000 / virtio-net.
5. Framebuffer simple, puis DRM/KMS.
6. evdev (clavier/souris), HDA (audio).
7. USB (le plus tard possible).
8. Wi-Fi (le plus dur, vendor-specific).

Règle : **un driver risqué peut tourner en espace utilisateur** (modèle
micro-noyau partiel) sans changer l'architecture globale.

---

## 11. Graphique et fluidité

Objectif : la sensation macOS, pas seulement l'affichage.

| Mécanisme | Implémentation |
|---|---|
| Modesetting | DRM/KMS |
| Rendu | compositeur serveur, animations côté serveur |
| Synchronisation | vsync, triple buffering, explicit sync |
| VRR | Adaptive-Sync quand disponible |
| Zéro-copie | `dma-buf`, `memfd`, textures partagées |
| Entrée | `evdev` → compositeur, latence minimale |
| Accélération | Mesa (llvmpipe d'abord, GPU ensuite) |

Le compositeur est **dans le dépôt**, pas un projet externe : c'est lui
qui porte l'identité et la fluidité de l'OS.

---

## 12. Compatibilité Windows

Chaîne : `PE → Wine (ntdll/kernel32/user32) → syscalls Linux → noyau`.

Prérequis noyau :

- `mmap` complet (`MAP_FIXED`, `PROT_EXEC`, `MAP_NORESERVE`) ;
- `futex`, `eventfd`, `epoll`, `signalfd`, `timerfd` ;
- sockets `AF_UNIX` + `SCM_RIGHTS` ;
- `/proc/self/{maps,exe,fd}` ;
- `memfd_create`, mémoire partagée ;
- `ioctl` DRM/evdev ;
- **IA32** pour WoW64.

Limites assumées : pas de `.sys`, pas d'anti-cheat kernel, GPU requis
pour les jeux.

---

## 13. Sécurité et robustesse

| Mécanisme | Jalon |
|---|---|
| ASLR/KASLR, NX, W^X | J3 |
| SMEP/SMAP, CET | J4 |
| Capabilities, `seccomp`-like | J5 |
| FS COW + checksums | J6 |
| Mises à jour A/B atomiques | J8 |
| Signature des modules noyau | J8 |
| Watchdog + panic isolé | J4 |
| Tests LTP + syzkaller | J4+ |

---

## 14. Licence

- **Noyau** : GPLv2 (copyleft, comme Linux).
- **Bibliothèques internes** : MIT ou LGPL-2.1.
- **Compositeur / outils** : MIT.
- **Wine** : LGPL (réutilisé tel quel).
- **musl** : MIT.
- **Firmware** : acceptés s'ils sont redistribuables (comme Linux).

---

## 15. Conventions de code

- Rust 2021, `no_std`, pas de `std`.
- `unsafe` encapsulé dans des modules dédiés, avec commentaire `// SAFETY:`.
- Pas de `panic!` dans les chemins critiques ; `Result` partout.
- Nommage : `snake_case` fonctions, `CamelCase` types, modules courts.
- Chaque module a un test unitaire quand c'est testable en QEMU.
- Commits atomiques, message au format `domaine: action`.
  Exemple : `mm: ajouter l'allocateur physique bitmap`.

---

## 16. Références techniques

- Intel SDM (vol. 3) — paging, interruptions, MSRs.
- UEFI Specification — boot, memory map.
- ACPI Specification — énergie, tables.
- Linux UAPI headers — contrat ABI.
- `musl` — côté user de l'ABI.
- Wine `dlls/ntdll/unix/` — syscalls réellement utilisés.
- OSTEP — concepts VM/processus/FS.
- xv6 — référence de lisibilité.
- Asterinas / Kerla / Managarm — noyaux Linux-ABI modernes.
