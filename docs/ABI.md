# Contrat ABI — XILI

Le noyau expose l'**ABI Linux x86-64**. Ce document est la référence
interne : tout ce qui n'y est pas explicitement autorisé est hors contrat.

> Source de vérité : les headers UAPI Linux (`include/uapi/asm-generic/`,
> `arch/x86/include/uapi/asm/unistd_64.h`) et les pages `man 2`.
> Ce document les résume ; en cas de doute, **le header gagne**.

---

## 1. Convention d'appel

| Élément | Valeur |
|---|---|
| Instruction | `syscall` (entrée), `sysret` (sortie) |
| Numéro | `rax` |
| Arguments | `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` |
| Retour | `rax` (valeur ou `-errno`) |
| Registres détruits | `rcx`, `r11` |
| MSRs | `LSTAR`, `STAR`, `SFMASK`, `FMASK` |
| 32 bits | `int 0x80`, args `ebx, ecx, edx, esi, edi, ebp` |

Règles :
- `rax` dans `[-4095, -1]` = erreur (`-EINVAL`, `-EFAULT`, …).
- Les structures sont **identiques à Linux** (mêmes tailles, mêmes
  alignements, mêmes champs). Aucune « amélioration ».
- Les pointeurs invalides renvoient `-EFAULT`, jamais un panic.

---

## 2. Structures critiques

| Structure | Piège |
|---|---|
| `struct stat` | taille et disposition exactes ; `statx` recommandé |
| `struct timespec` | 16 octets (sec 64, nsec 64) |
| `struct epoll_event` | 12 octets, **packed** |
| `struct sigaction` | `sa_restorer`, `sa_flags`, `sa_mask` |
| `struct rlimit` | 2 × `u64` |
| `struct utsname` | 6 × 65 octets |
| `struct iovec` | 16 octets |
| `struct msghdr` | 56 octets, `msg_control` pour `SCM_RIGHTS` |
| `struct sockaddr_un` | 110 octets |
| `struct clone_args` | `clone3`, 88 octets |

Toute structure doit être vérifiée par un test de taille (`size_of`)
et comparée à un programme Linux de référence.

---

## 3. Lots d'implémentation

### Lot A — busybox statique (J3)

Objectif : un shell, `ls`, `cat`, `echo`, `sh -c`.

```
read 0            write 1           close 3           lseek 8
mmap 9            mprotect 10       munmap 11         brk 12
ioctl 16          pread64 17        pwrite64 18       readv 19
writev 20         access 21         pipe 22           mremap 25
madvise 28        dup 32            dup2 33           nanosleep 35
getpid 39         clone 56          fork 57           vfork 58
execve 59         exit 60           wait4 61          kill 62
uname 63          fcntl 72          fsync 74          fdatasync 75
truncate 76       ftruncate 77      getcwd 79         chdir 80
fchdir 81         rename 82         mkdir 83          rmdir 84
creat 85          link 86           unlink 87         symlink 88
readlink 89       chmod 90          fchmod 91         chown 92
fchown 93         umask 95          gettimeofday 96   getrlimit 97
getrusage 98      sysinfo 99        getuid 102        getgid 104
geteuid 107       getegid 108       setpgid 109       getppid 110
getpgrp 111       setsid 112        getpgid 121       getsid 124
sigaltstack 131   statfs 137        fstatfs 138       arch_prctl 158
setrlimit 160     sync 162          mount 165         umount2 166
gettid 186        futex 202         sched_setaffinity 203
sched_getaffinity 204               getdents64 217    set_tid_address 218
clock_gettime 228 clock_getres 229  clock_nanosleep 230
exit_group 231    epoll_wait 232    epoll_ctl 233     tgkill 234
openat 257        mkdirat 258       newfstatat 262    unlinkat 263
renameat 264      readlinkat 267    utimensat 280     epoll_pwait 281
signalfd 282      timerfd_create 283 eventfd 284      accept4 288
signalfd4 289     eventfd2 290      epoll_create1 291 dup3 292
pipe2 293         prlimit64 302     getrandom 318     memfd_create 319
statx 332         rseq 334
```

Priorité : `read`, `write`, `openat`, `close`, `mmap`, `brk`, `exit`,
`execve`, `wait4`, `clone`, `newfstatat`, `getdents64`.

### Lot B — Alpine / musl (J4)

- `futex` complet : `WAIT`, `WAKE`, `REQUEUE`, `CMP_REQUEUE`, `WAKE_OP`,
  `LOCK_PI`, `UNLOCK_PI`, `TRYLOCK_PI`, `WAIT_BITSET`, `WAKE_BITSET`,
  `WAIT_REQUEUE_PI`, `CMP_REQUEUE_PI`, `ROBUST`.
- `epoll_create1` / `epoll_ctl` / `epoll_wait` / `epoll_pwait2`.
- `eventfd2`, `signalfd4`, `timerfd_*`.
- `inotify_*`, `fanotify_*` (plus tard).
- `socket`, `bind`, `listen`, `accept4`, `connect`, `sendmsg`,
  `recvmsg`, `setsockopt`, `getsockopt`, `shutdown`, `socketpair`.
- `ptrace` (debug), `process_vm_readv/writev`.
- `prctl`, `capget`, `capset`, `sched_*`, `setpriority`.
- `fallocate`, `copy_file_range`, `sendfile`, `splice`, `tee`.
- `clone3`, `close_range`, `openat2`, `faccessat2`, `mseal`.

### Lot C — Wine (J8)

Wine utilise **directement** les syscalls Linux. Le lot A+B doit être
quasi complet, plus :

- `mmap` : `MAP_FIXED`, `MAP_FIXED_NOREPLACE`, `MAP_NORESERVE`,
  `PROT_EXEC`, `MAP_32BIT` (pour WoW64).
- `mprotect`, `mremap`, `madvise` (`MADV_DONTNEED`, `MADV_FREE`).
- `memfd_create`, `shmget`/`shmat`/`shmdt`/`shmctl` (ou équivalent).
- `AF_UNIX` + `SCM_RIGHTS` (passage de descripteurs).
- `/proc/self/maps`, `/proc/self/exe`, `/proc/self/fd`, `/proc/<pid>/`.
- `ioctl` : DRM, evdev, TTY.
- `uname` : renvoyer un nom crédible (Wine teste la version).
- `getrandom`, `sysinfo`, `sched_setaffinity`, `prctl`.
- **IA32** : `int 0x80`, ELF32, structures 32 bits (J9).

### Lot D — Graphique (J7)

- `ioctl` DRM : `DRM_IOCTL_MODE_*`, `GEM_*`, `SYNCOBJ_*`.
- `dma-buf` : `DMA_BUF_IOCTL_*`, `SYNC_FILE`.
- `evdev` : `EVIOCG*`, `EVIOCS*`.
- `memfd_create` + `mmap` partagé.

---

## 4. Les pièges (par ordre de gravité)

1. **Pile initiale** : `argc`, `argv[]`, `envp[]`, `auxv[]` aux bonnes
   adresses, `AT_PHDR`, `AT_ENTRY`, `AT_BASE`, `AT_RANDOM`, `AT_EXECFN`,
   `AT_SYSINFO_EHDR`. Une erreur = `ld.so` segfault avant `main`.
2. **TLS** : `arch_prctl(ARCH_SET_FS)` + `MSR_FS_BASE`. Sans TLS, pas
   d'`errno`, pas de threads.
3. **`futex`** : sémantique exacte, y compris les cas `EAGAIN`, `EINTR`,
   les timeouts et `FUTEX_CLOCK_REALTIME`.
4. **`clone`** : tous les flags (`CLONE_VM`, `CLONE_THREAD`, `CLONE_FS`,
   `CLONE_FILES`, `CLONE_SIGHAND`, `CLONE_SETTLS`, `CLONE_CHILD_CLEARTID`,
   `CLONE_PARENT_SETTID`, `CLONE_VFORK`).
5. **`errno`** : valeurs exactes. `strace` est l'oracle.
6. **`/proc`** : formats texte précis (`maps`, `stat`, `status`).
7. **`ioctl` TTY** : `TCGETS`/`TCSETS`/`TIOCGWINSZ` — sinon pas de shell
   interactif correct.
8. **vDSO** : optionnel, mais prévoir le fallback syscall pour
   `clock_gettime`.
9. **32 bits** : tailles de `long`, `stat`, `timespec`, `iovec` différentes.

---

## 5. Méthode de vérification

| Outil | Usage |
|---|---|
| `strace -f -e trace=all` sur Linux | liste exacte des syscalls d'un programme |
| **LTP** (Linux Test Project) | conformité syscall par syscall |
| `syzkaller` | fuzzing, recherche de crashes |
| `linux-api-tests` | tests d'API |
| Tests maison | tailles de structures, errno, cas limites |
| QEMU + GDB | débogage noyau |

Boucle recommandée :

1. Lancer le programme cible sous `strace` sur Linux.
2. Extraire les syscalls utilisés.
3. Vérifier qu'ils sont dans le lot courant.
4. Implémenter, tester, comparer les retours.
5. Ajouter un test de non-régression.

---

## 6. Références

- `man 2 syscall`, `man 2 <nom>` — sémantique.
- Linux `include/uapi/` — structures et constantes.
- `arch/x86/entry/syscalls/syscall_64.tbl` — numéros.
- musl `src/internal/` et `arch/x86_64/` — côté user.
- Wine `dlls/ntdll/unix/` — usage réel.
- LTP, syzkaller — validation.
