# Feuille de route — XILI

Chaque jalon a un **objectif**, des **livrables**, un **test d'acceptation**
et des **risques**. On ne passe au suivant qu'après le test d'acceptation.

Légende : ✅ fait · 🔄 en cours · ⬜ à faire

---

## Vue d'ensemble

| Jalon | Thème | Durée estimée (solo, plein temps) | État |
|---|---|---|---|
| J0 | Cadrage, outillage, CI | 2–3 semaines | ✅ |
| J1 | Boot + console | 1–2 mois | ✅ |
| J2 | Mémoire | 2–4 mois | 🔄 |
| J3 | Processus + shell | 3–6 mois | ⬜ |
| J4 | Alpine (userland complet) | 4–8 mois | ⬜ |
| J5 | Réseau | 2–4 mois | ⬜ |
| J6 | Stockage robuste | 2–4 mois | ⬜ |
| J7 | Graphique + compositeur | 6–12 mois | ⬜ |
| J8 | Wine / Windows | 3–6 mois | ⬜ |
| J9 | 32 bits / WoW64 | 2–4 mois | ⬜ |
| J10 | Matériel réel + daily driver | 12–24 mois | ⬜ |

> Les durées sont des ordres de grandeur pour une personne à plein temps.
> Elles ne s'additionnent pas linéairement : certaines phases se chevauchent.

---

## J0 — Cadrage et outillage ✅

**Objectif** : un dépôt qui compile, boote et teste en une commande.

**Livrables**
- Workspace Cargo, noyau `no_std`, zéro dépendance externe.
- `Makefile` : `build`, `iso`, `run`, `test`.
- CI locale : `make test` vérifie un marqueur de boot.
- Documentation de pilotage (`docs/`).

**Test d'acceptation**
```bash
make test   # → "TEST OK"
```

---

## J1 — Boot et console ✅

**Objectif** : passer de GRUB au mode long 64 bits et afficher du texte.

**Livrables**
- `boot.s` : multiboot2, tables de pages 1 Gio, PAE/LME/PG, GDT 64 bits, SSE.
- Console double : UART 16550 (COM1) + VGA texte 80×25.
- Gestion `panic` minimale.
- Affichage du magic multiboot2 et de l'adresse de la MBI.

**Test d'acceptation**
```
J1: noyau charge, mode long actif, console OK.
```

**Limites connues**
- Boot BIOS uniquement (GRUB), pas encore UEFI.
- Pas de memory map exploitée, pas d'interruptions.

---

## J2 — Mémoire 🔄

**Objectif** : gérer la RAM physique et virtuelle de façon fiable.

**Livrables**
1. ~~Chargeur **UEFI** (Limine ou maison) remplaçant GRUB ; récupération de
   la memory map et du framebuffer.~~ **Partiel** : GRUB UEFI boote (ISO + ESP)
   et le critère d'acceptation UEFI est atteint. La memory map est lue
   (voir livrable 2) ; le chargeur n'est pas encore Limine/maison.
2. ~~Parseur de memory map (UEFI ou E820).~~ **Fait** : lecture de la MBI
   Multiboot2, tags 1 et 6, affichage de la synthèse et du détail.
3. Paging noyau : 4 niveaux, 4 Kio + 2 Mio, higher-half.
4. Allocateur physique bitmap (puis buddy).
5. Allocateur noyau slab (`kmalloc`/`kfree`).
6. `mmap`/`munmap`/`mprotect` côté noyau (sans userland).
7. GDT/TSS par CPU, IDT, exceptions (`#PF`, `#GP`, `#DF`), panic handler propre.
8. PIC → APIC, timer PIT/HPET, clavier PS/2.

**Test d'acceptation**
- Le noyau affiche la carte mémoire et la quantité de RAM utilisable.
- Une allocation de 100 Mio réussit puis est libérée.
- Un accès volontaire à une adresse non mappée déclenche un `#PF` géré,
  pas un triple fault.
- Boot sur **QEMU UEFI** (`-bios OVMF` ou `-machine q35`).

**Risques**
- Complexité du higher-half et des tables de pages → commencer simple
  (identity map) puis migrer.
- OVMF absent de la machine → prévoir un fallback BIOS.

---

## J3 — Processus, appels système, premier shell ⬜

**Objectif** : exécuter un binaire ELF et obtenir un shell.

**Livrables**
1. `Task`/`Thread`, context switch en assembleur, scheduler round-robin.
2. `syscall`/`sysret` : `MSR_LSTAR`, `MSR_STAR`, `MSR_SFMASK`.
3. Syscalls du sous-ensemble « busybox statique » (voir `ABI.md`, lot A).
4. Chargeur ELF64, pile initiale conforme Linux (`argv`, `envp`, `auxv`).
5. VFS minimal + `initrd` (cpio newc) + `devtmpfs` minimal.
6. `arch_prctl(ARCH_SET_FS)` + TLS.
7. `panic` isolé : un processus fautif meurt, le noyau survit.

**Test d'acceptation**
- `busybox sh` démarre, `ls`, `cat`, `echo` fonctionnent.
- `busybox sh -c 'exit 42'` renvoie 42.
- Un `segfault` utilisateur tue le processus uniquement.
- Deux processus tournent en parallèle (test `yes | head`).

**Risques**
- Layout de la pile initiale (cause n°1 d'échec de `ld.so`).
- `clone` et ses flags.
- `futex` absent à ce stade → pas de threads, c'est acceptable.

---

## J4 — Alpine, userland complet ⬜

**Objectif** : faire tourner un vrai userland Linux.

**Livrables**
1. `ld-musl-x86_64.so.1`, `PT_INTERP`, `AT_BASE`, `AT_PHDR`.
2. `futex` complet, `epoll`, `eventfd`, `signalfd`, `timerfd`.
3. `/proc` minimal (`self/maps`, `self/exe`, `self/fd`, `stat`, `meminfo`).
4. `/dev` (null, zero, random, tty), `devpts` (PTY).
5. `memfd_create`, mémoire partagée, `SCM_RIGHTS`.
6. `ioctl` TTY (`TCGETS`, `TCSETS`, `TIOCGWINSZ`).
7. Image Alpine montée en `initrd` ou sur disque.

**Test d'acceptation**
- `apk update` puis `apk add bash vim python3` réussissent (réseau en J5,
  donc d'abord depuis un cache local).
- `bash`, `vim`, `python3 -c 'print(1+1)'` fonctionnent.
- `ps`, `top` lisent `/proc`.

**Risques**
- `futex` : cœur de pthreads, très subtil (PI, ROBUST, BITSET).
- `ioctl` TTY : semaines de travail si mal abordé.

---

## J5 — Réseau ⬜

**Objectif** : connecter l'OS.

**Livrables**
1. virtio-net (QEMU), puis e1000.
2. Sockets BSD (`AF_INET`, `AF_UNIX`), `sendmsg`/`recvmsg`.
3. Pile TCP/IP : d'abord **lwIP** intégré, puis pile maison si nécessaire.
4. DNS (`/etc/resolv.conf`), DHCP.
5. `apk update` réel, `ssh`, `curl`.

**Test d'acceptation**
- `ping 1.1.1.1`, `curl https://example.com` (TLS via mbedTLS/OpenSSL).
- `apk update` depuis un dépôt distant.
- `ssh` vers une machine externe.

**Risques**
- Performance de la pile → commencer par lwIP.
- TLS et certificats → réutiliser, ne pas réécrire.

---

## J6 — Stockage robuste ⬜

**Objectif** : ne pas perdre de données.

**Livrables**
1. AHCI (SATA), NVMe.
2. ext2 lecture/écriture, puis ext4 (extents, journal).
3. Cache de pages, écriture différée, `fsync` correct.
4. FS copy-on-write + checksums (design maison ou inspiration Btrfs/ZFS).
5. Snapshots, `fsck` minimal.
6. Swap.

**Test d'acceptation**
- Installation sur disque, redémarrage, données intactes.
- Coupure brutale (`kill -9` de QEMU) → `fsck` répare, pas de corruption.
- `dd` + `sync` + reboot → checksums valides.

**Risques**
- La corruption de données est le pire scénario : tests intensifs.

---

## J7 — Graphique et compositeur ⬜

**Objectif** : un bureau fluide.

**Livrables**
1. DRM/KMS : modesetting, framebuffer, curseur matériel.
2. evdev (clavier, souris, touchpad).
3. Compositeur maison (animations côté serveur, vsync, triple buffering).
4. Wayland (protocole) + toolkit (GTK/Qt ou maison).
5. Mesa : `llvmpipe` d'abord, puis pilote GPU Intel/AMD.
6. `dma-buf`, `memfd`, zéro-copie.

**Test d'acceptation**
- Bureau avec fenêtres, redimensionnement, animations à 60 fps.
- `weston-info` / client Wayland de test fonctionne.
- Lecture vidéo sans déchirure.

**Risques**
- **Le pilote GPU est le chantier le plus long du projet.**
- Sans GPU : llvmpipe fonctionne mais lent.

---

## J8 — Compatibilité Windows (Wine) ⬜

**Objectif** : lancer une application Windows.

**Livrables**
1. `mmap` complet (`MAP_FIXED`, `PROT_EXEC`, `MAP_NORESERVE`).
2. `futex`, `epoll`, `signalfd`, `eventfd` durcis.
3. `/proc/self/maps` fidèle.
4. Wine compilé/porté, `wineboot` fonctionnel.
5. DXVK + VKD3D (Vulkan via Mesa).
6. `wineserver` (sockets Unix, mémoire partagée).

**Test d'acceptation**
- `notepad.exe` s'ouvre.
- Une application Win32 simple (7-Zip, PuTTY) fonctionne.
- Un jeu DirectX 9/11 tourne via DXVK (avec GPU).

**Risques**
- Wine est un intégrateur impitoyable : il révèle chaque bug d'ABI.
- Sans GPU, les jeux sont injouables.

---

## J9 — 32 bits / WoW64 ⬜

**Objectif** : exécuter les applications Windows 32 bits.

**Livrables**
1. Support IA32 : `int 0x80`, ELF32, GDT/TSS 32 bits, `vsyscall` émulé.
2. `IA32_EMULATION` : compatibilité des structures (`stat`, `timespec`).
3. Wine WoW64 (mode 32-sur-64 ou side-by-side).

**Test d'acceptation**
- Un binaire ELF 32 bits tourne.
- Une application Windows 32 bits tourne via Wine.

**Risques**
- Les structures 32 bits diffèrent : source classique de bugs silencieux.

---

## J10 — Matériel réel et daily driver ⬜

**Objectif** : utiliser l'OS tous les jours sur une machine.

**Livrables**
1. ACPI complet : veille, réveil, batterie, thermique.
2. Audio HDA, USB (clavier/souris/stockage).
3. Wi-Fi (puce choisie explicitement).
4. Installateur, partitionnement, bootloader UEFI signé.
5. Mises à jour A/B atomiques + rollback.
6. Gestionnaire de paquets, dépôt, signatures.
7. Sauvegardes, snapshots, restauration.

**Test d'acceptation**
- 30 jours d'usage principal sans revenir à un autre OS.
- Veille/réveil fiables, audio, Wi-Fi, mises à jour sans casse.

**Risques**
- Support matériel : puits sans fond. **Une machine cible, pas « tous les PC ».**

---

## Risques majeurs du projet

| Risque | Impact | Mitigation |
|---|---|---|
| Pilote GPU | Bloque J7 et J8 | Choisir une puce Intel/AMD bien documentée ; llvmpipe en attendant |
| `futex`/`clone` | Bloque threads et Wine | Implémenter tôt, tester avec LTP |
| Layout de pile initiale | `ld.so` ne démarre pas | Comparer avec `strace`/Linux, tests unitaires |
| Support matériel | Projet sans fin | Une machine, un chipset, un GPU |
| Motivation | Abandon | Jalons courts, `make test` toujours vert, devlog |
| Dérive de l'ABI | Portage massif | `ABI.md` comme contrat, LTP en CI |

---

## Définition de « terminé » pour un jalon

1. Le code compile sans warning.
2. `make test` passe.
3. Le test d'acceptation du jalon passe, documenté dans `docs/`.
4. Aucune régression sur les jalons précédents.
5. Une note de 5 lignes dans `docs/JOURNAL.md` (ce qui marche, ce qui reste).
