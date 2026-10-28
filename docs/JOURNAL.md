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
|- Bug : `bits[w].trailing_zeros()` trouve le plus bas bit UTILISÉ — il faut
|  inverser (`!bits[w]`) pour obtenir le plus bas bit LIBRE.

## 2026-10-28 — J2.5 : IDT + gestion des exceptions (#DF, #GP, #PF)
|- `kernel/src/tss.rs` : TSS 64 bits (104 octets) avec IST1 (8 KiB BSS,
|  aligné 16). `init(stack_top)` construit une GDT complète (null/code/data/TSS),
|  encode le descripteur TSS (base = &TSS, type = 0x9, P=1) et charge le TR
|  via `ltr 0x18` sur entrée 3 de la GDT.
|- `kernel/src/idt.rs` : IDT 256 entrées, `#[repr(C, align(8))]`. 32 stubs
|  ASM (`isr.s`) poussent [vector][error_code] puis appellent
|  `handle_exception`. Handlers Rust pour #DF (IST1), #GP, #PF. CR2 lu via
|  `mov ..., cr2`. Gate : trap gate pour #DF, interrupt gate sinon.
|- `kernel/src/isr.s` : stubs ASM pour les 32 vecteurs (NOERR/ERR macros).
|  `isr_common` pousse rsp → rdi, appelle `handle_exception` (→ !).
|- `kernel/src/main.rs` : `mod tss; mod idt;`, `tss::init()` + `idt::init()`
|  après le paging, test #PF volontaire (`0xDEAD0000`).
|- `Makefile` : gate J2.5 + J2.5b ajouté (vérifie marqueurs IDT, #PF, CR2).
|- Bugs rencontrés :
|  - `nomem` sur `asm!` lgdt/ltr → LLVM réordonne les écritures GDT après
|    l'instruction → TSS descriptor à 0 → #GP → triple fault. Supprimé `nomem`.
|  - `ltr {0}` avec `in(reg) u16` → LLVM génère `ltr rax` → rejeté par
|    l'assembleur. Corrigé avec `{0:x}` (format 16 bits → `ltr cx`).
|  - `extern "C" { fn stub() } as usize` en `const` → erreur const-eval.
|    Solution : résolu à l'exécution dans `init()`.
|- Vérifié : `make test` passe — BIOS (J1 + J2.3 + J2.3b + J2.4 + J2.5 + J2.5b).
|  Le #PF sur 0xDEAD0000 est intercepté, CR2 = 0xdead0000, code 0x2
|  (écriture page non-présente, mode noyau), blocage via hlt.

## 2026-10-28 — Maquette du bureau (HTML/CSS/JS, hors noyau)

- `prototype/` : maquette du compositeur cible (J7), bâtie sur les jetons de
  `DESIGN.md` §4–§7. Aucune ombre, aucun flou, mouvement limité à position /
  taille / opacité, ≤ 320 ms.
- `docs/DESIGN.md` mis à jour **avant** le code (règle §10) :
  - §9 : décisions 2 (thème sombre par défaut), 3 (1280×800), 6 (accent)
    tranchées par Jean Yannick.
  - §10 nouveau : composition du bureau — barre haute unique (menu gauche,
    tâches centre, état droite), contrôles de fenêtre **en bas à droite**
    (Fermer · Réduire · Agrandir), bureau vide épinglable, démo Terminal +
    Fichiers côte à côte.
  - §10.5 nouveau : **lisière lumineuse** de la fenêtre active — 2 px, halo
    ≤ 4 px, statique, couleur **libre** (exception §4 assumée). Coût noté au
    §3 : tenable en llvmpipe sur une seule couche ; repli = filet coloré sans
    halo.
- Contenu : gestionnaire de fenêtres (déplacement, redimensionnement,
  agrandir, réduire, focus), barre des tâches, menu XILI, terminal (aide, ls,
  date, lisiere, animations, sombre/clair, epingler, historique ↑↓),
  gestionnaire de fichiers (navigation, drag vers le bureau → épingle),
  épingles déplaçables et sélectionnables (accent), thèmes sombre/clair,
  animations désactivables, raccourcis Ctrl+Alt+T/F/L.
- Vérifié : servi en local, testé en navigateur — ouverture des deux apps,
  menu, bascule de thème, lisière personnalisée, aucune erreur console.
- Décisions prises : contrôles bas-droite (choix personnel, contre macOS et
  Windows), lisière couleur libre, bureau épinglable plutôt qu'icônes fixes.
- Reste : faces typographiques exactes (décision 4 ouverte), signification
  du nom (décision 1), validation en conditions réelles (QEMU).

## 2026-10-28 — Identité « Encre et soleil » + marque

- Sources : `xili-design/Palette.pdf` et `xili-design/Logo en couleur.pdf`,
  fournis par Jean Yannick. **La palette remplace la précédente** au §4 —
  décision explicite, l'ancienne était pourtant gelée.
- Nouvelles couleurs de marque : Encre `#12151F`, Ivoire `#F6F1E7`, Orange
  brûlé `#FF6B2C`, Safran `#FFC53D`.
- Jetons manquants **dérivés par interpolation** (Encre↔Ivoire), pas inventés :
  `surface` 6 %, `rule` 11 %, `text-dim` 55 % (sombre) / 62 % (clair).
- Contrainte **mesurée**, écrite au §4.3 : l'orange et le safran sont des
  couleurs de lumière. Sur Ivoire ils tombent à 2.52:1 et 1.40:1 — sous le
  plancher. Le clair assombrit donc ses accents (HLS, teinte préservée) :
  orange → `#BF3900` (4.91:1), safran → `#8E6400` (4.70:1). Même mécanique
  que l'ancienne palette, qui passait `primary` de `#7FA6C9` à `#3E6E9E`.
- Rôles validés : **l'orange porte le focus et les actions, le safran est
  l'accent rare.**
- Marque : `assets/logo/xili-marque.svg` (recadrée sur 460×460) + variante
  favicon à couleurs figées. **Monochrome** — §4.1 assigne Ivoire à
  « texte, logo » ; la planche colorée du PDF est une présentation de marque,
  pas un usage d'interface. Suit `currentColor` via `--marque`.
  *(Décision **inversée** le jour même — voir l'entrée suivante. Je l'avais
  prise sur une lecture de §4.1, sans regarder la planche : la marque est en
  couleurs.)*
- **Taille minimale 24 px** : vérifié au rendu, à 18 px la marque se réduit à
  quatre points. La barre la porte donc à 24 px.
- Bug corrigé : un commentaire XML ne peut pas contenir `--` — mon premier
  SVG avait `--marque` dans le commentaire, invalide.
- Lisière : défaut=`primary` du thème (c'est une bordure, 3:1 minimum),
  personnalisable ensuite. Le choix manuel fige la couleur, le changement de
  thème la remet sur le primaire tant qu'elle n'est pas personnalisée.
- Vérifié : les deux thèmes en navigateur, lisière suivant le thème, marque
  en ivoire sur encre et inversement, aucune erreur console.
- Reste : la marque sous 24 px (forme réduite à définir), génération des PNG
  d'assets depuis le SVG si le noyau en a besoin avant J7.

## 2026-10-28 — Marque en couleurs + halo 6 px

- Demande de Jean Yannick : « met de la couleur dans le logo ». La décision
  monochrome de l'entrée précédente est **inversée** — elle reposait sur une
  lecture de §4.1 (« Ivoire : texte, logo »), sans vérifier la planche.
- **Affectation mesurée** sur `xili-design/Logo en couleur.pdf` (échantillon
  de pixels, pas à l'œil) : ruban en S → Orange brûlé `#FF6B2C`, les deux
  disques → Safran `#FFC53D`, les vides → Encre `#12151F`. Teintes **fixes**
  sur les trois fonds de la planche ; seul le ruban passe en ivoire sur fond
  orange (variante inutilisée ici).
- **La plaque** : la planche pose la marque sur un carré encre arrondi dès
  que le fond n'est pas de l'encre. On l'inclut **toujours** — sur sombre
  elle se confond, sur clair elle porte la marque. C'est elle qui tient le
  contraste, jamais les teintes chaudes seules (2,52:1 sur ivoire, §4.3).
- **Deux erreurs de ma part, trouvées par mesure :**
  1. Première version : plaque de 500 unités, soit 20 de marge — jugé « à
     peu près ». Puis 540 (40 de marge). Mesuré sur la planche à taille
     native : la boîte chaude doit occuper **74,8 %** de la plaque ; mes
     versions donnaient 62,3 %. La plaque vaut **exactement l'emprise du
     trait, 460 unités, aucune marge**. Règle retenue : la plaque n'est pas
     un cadre autour du logo, c'est son gabarit.
  2. Je partais des 400 unités du tracé pour la boîte *chaude*. Le `stroke`
     de 60 en recouvre 30 en dedans : le visible fait **340 unités**, et
     340/460 = 73,9 % — pile la valeur de la planche.
- **Rayon : corrigé de 85 à 98 unités (18,5 % → 21,3 % du côté).** Mon
  premier calcul par simple profil de coin donnait 18,5 % pour la planche —
  **faux**, il s'arrêtait au premier pixel nul, avalé par l'antialiasing. Ce
  qui a trahi l'erreur, c'est la comparaison agrandie à 4× : à l'échelle 1:1
  les deux coins semblaient identiques. Mesure reprise par **ajustement
  circulaire** (`inset(k) = r − √(r² − (r−k)²)`) : planche = 21,3 %. La
  méthode se valide elle-même — appliquée à notre propre rendu elle rend
  18,5 %, qui est la valeur connue, donc elle ne ment pas. Après correction,
  les deux mesurent 21,3 %.
- Contrôle des marges final, natif : théorique 340/460 = **73,9 %**, notre
  rendu 73,1 %, planche 75,0 % — la valeur vraie tombe **entre** les deux,
  l'écart tient au rasterizer et pas à la géométrie.
- Fichiers : `assets/logo/xili-marque-couleur.svg` (référence) et
  `xili-favicon.svg` (idem, teintes figées). La marque est **inlinée** dans
  `prototype/index.html` — un `<img>` ne reçoit pas les variables de la page.
- **Nettoyage** : `--marque` / `--marque-fond` et la règle `.marque-forme
  path` sont du code mort depuis l'inlining ; supprimés de `style.css`.
  `xili-marque.svg` (une seule encre) est conservé mais **plus référencé par
  aucune interface** — réservé à l'impression monochrome et au gravé.
- **Même bug XML, repris une deuxième fois** : une ligne de séparation
  `------` dans le commentaire du SVG est une série de tirets, donc du `--`
  interdit en commentaire XML — le fichier était invalide, ImageMagick le
  refusait. Corrigé en `====`. Depuis, **les trois SVG sont validés par un
  parseur XML** (et la validation a elle-même été contrôlée : le parseur
  rejette bien un `--` de commentaire, sinon elle ne vaudrait rien).
- **Halo de la lisière : 4 px → 6 px**, demande explicite. §10.5 mis à jour
  **avant** le CSS (règle du dépôt).
- **DESIGN.md §4.5 réécrit** : la marque est en couleurs, la planche PDF est
  un usage et non plus une simple présentation, et les proportions de la
  plaque sont documentées avec leurs mesures.
- Vérifié : les deux thèmes en navigateur (plaque encre tenue sur l'ivoire),
  halo à 6 px, lisière suivant le thème, zéro erreur console.
- Reste : la marque sous 24 px (forme réduite à définir), variante ruban
  ivoire sur fond orange si un jour un fond orange apparaît.

## 2026-10-28 — La lisière devient une bande (le flou disparaît)

- Demande de Jean Yannick : « je veux que cela soit une légère bande » pour
  le halo de la lisière. **Le halo flou est supprimé au profit d'une bande
  nette.**
- Implémentation : `box-shadow` en **spread pur** — `0 0 0 6px`, rayon de
  flou **0**. Vérifié en navigateur : `rgb(255,107,44) 0px 0px 0px 6px`.
  Variable renommée `--lisiere-halo` → `--lisiere-bande`, parce qu'elle ne
  désigne plus un halo.
- **Le doc le disait déjà** : §10.5 s'ouvre sur « Chaque fenêtre est
  entourée d'une bande fine et lumineuse ». C'est l'implémentation qui
  dérivait — j'avais traduit « bande » par « glow ». Le code est réaligné sur
  le document.
- **Gain de budget (§3)** : un spread sans flou est un aplat, pas une
  convolution — la lisière passe de « accepté, une seule à l'écran » à
  **« de base »** dans le tableau du §3, et le repli prévu (lisière sans
  halo) n'a plus lieu d'être. On retrouve le langage assumé du §3 :
  « aplats, filets, typographie, mouvement ».
- Filet + bande sont de la même couleur et contigus : **1 px + 6 px = 7 px**
  de lecture autour de la fenêtre active.
- **Écart doc/code trouvé au passage** : §10.5 annonçait « Épaisseur 2 px »
  alors que le filet fait 1 px. Cause : la variable `--lisiere-epaisseur`
  était **déclarée mais jamais utilisée** — du code mort, supprimée. Doc
  corrigé au 1 px réel. On garde 1 px uniforme entre fenêtres actives et
  inactives : passer à 2 px au focus décalerait le contenu de 1 px à chaque
  changement de focus.
- Note ouverte : une bande de 6 px **pleine** paraît plus épaisse qu'un flou
  de 6 px, qui s'essore. 6 px est conservé (nombre demandé explicitement) ;
  si le rendu paraît trop lourd, c'est `--lisiere-bande`, un seul chiffre.
- Non vérifié cette fois : **capture d'écran impossible** — le bureau de
  session n'était pas visible. La vérification repose sur le style calculé
  (`box-shadow` à 0 de flou) et l'absence d'erreur console.

---

## 2026-10-28 — J2.6 : PIC remappé, timer 100 Hz, clavier PS/2

- `kernel/src/io.rs` : primitives partagées `outb`/`inb`/`outw`/`inw` avec
  `asm!` inline. `serial.rs` refactorisé pour réutiliser ce module.
- `kernel/src/pic.rs` : remappage du PIC 8259 (master 0x20/0x21, slave 0xA0/0xA1)
  vers les vecteurs 32..47 (offset maître 0x20, esclave 0x28). ICW1-4 + OCW1
  (masque IRQ0+IRQ1 actifs, le reste masqué). `eoi(irq)` envoie le signal de
  fin d'interruption au bon PIC.
- `kernel/src/irq.rs` : programmation du PIT à 100 Hz (divisor = 1193180/100 = 1193,
  mode 2 rate generator). `handle_irq` (extern "C") dispatch sur le vecteur :
  32 → timer, 33 → clavier. Compteur `TICK_COUNT` incrémenté via `read_volatile`/
  `write_volatile`. `LAST_SCANCODE` et `KEY_COUNT` pour le clavier.
- `kernel/src/isr.s` : 16 stubs IRQ (irq_stub_32..47). Même pattern NOERR
  (push 0 factice + vecteur) → `irq_common` → `handle_irq` → `iretq`.
- `kernel/src/idt.rs` : entrées IDT[32..47] pointant sur `irq_stub_32..47`,
  interrupt gates (0x8E), IST=0 (pile courante, IF=0 pendant le handler).
- `kernel/src/main.rs` : `mod io; mod pic; mod irq;`, `pic::init()` + `irq::init()`
  après l'IDT, `sti`, test J2.6b (boucle `hlt` attendant 3 ticks du timer).
- `Makefile` : gates J2.6 ("PIC remape") et J2.6b ("timer IRQ active") ajoutés.
- Bug identifié (non résolu) : `TICK_COUNT` vaut 18446744071563173920
  (0xFFFFFFFF8010xxxx) — corruption du `static mut`. `handle_irq` est bien
  appelé (trace "IRQ 0"), le timer tick (trace "B" = tick #1), mais la lecture
  depuis `kernel_main` est corrompue. Cause probable : le `nomem` sur `outb`
  permet au compilateur de réordonner les écritures PIC/PIT par rapport à `sti`
  et aux `read_volatile`/`write_volatile`. Diagnostic en cours avec GDB.
- Reste : corriger la corruption du compteur de ticks (sus : `nomem` sur `outb`),
  valider `make test` (gate J2.6b échoue).

---

```
## AAAA-MM-JJ — JX : titre

- Ce qui a été fait.
- Ce qui a été vérifié (commande + résultat).
- Décisions prises.
- Ce qui reste.
- Risques / dette.
```
