# Méthode de travail — IA + humain

L'IA écrit le code. L'humain décide, exécute sur le matériel réel, et
tranche. Ce document définit la boucle de travail pour que « l'IA code
tout » ne devienne pas « l'IA code n'importe quoi ».

---

## 1. Répartition des rôles

| Tâche | IA | Humain |
|---|---|---|
| Écrire le code noyau | ✅ | — |
| Écrire les tests | ✅ | — |
| Lire les specs (SDM, UEFI, ACPI) | ✅ | — |
| Compiler, lancer QEMU | ✅ | — |
| Analyser les logs, `-d int`, GDB | ✅ | — |
| Décider de l'architecture | propose | ✅ tranche |
| Tester sur matériel réel | ❌ | ✅ |
| Fournir les logs matériels | ❌ | ✅ |
| Arbitrer les compromis | propose | ✅ |
| Valider un jalon | propose | ✅ |

**Règle d'or** : l'IA ne prétend jamais qu'un code fonctionne sur du
matériel réel. Seul QEMU est vérifiable par l'IA.

---

## 2. La boucle (une itération = une petite tâche)

```
1. SPÉCIFIER   → objectif en 3 lignes + test d'acceptation
2. PLANIFIER   → 1 à 5 fichiers touchés, pas plus
3. CODER       → implémentation + tests
4. COMPILER    → cargo build, zéro warning
5. TESTER      → make test + test spécifique
6. INSPECTER   → logs QEMU, GDB si crash
7. COMMITTER   → un commit atomique, message clair
8. JOURNALISER → 5 lignes dans docs/JOURNAL.md
```

Une itération ne doit **jamais** casser `make test`. Si c'est le cas,
on revient en arrière avant de continuer.

---

## 3. Règles de code

1. **Pas de dépendance externe** dans le noyau sans justification écrite.
2. **`unsafe` isolé** : un module par abstraction, commentaire `// SAFETY:`.
3. **Pas de `panic!`** dans un chemin critique ; `Result` + `errno`.
4. **Toute structure ABI** a un test de taille.
5. **Tout syscall** a un test de cas nominal + un cas d'erreur.
6. **Pas de code mort** : ce qui n'est pas utilisé est supprimé.
7. **Commentaires en français**, identifiants en anglais.

---

## 4. Règles anti-hallucination

Les erreurs classiques d'une IA sur un noyau :

| Erreur | Contre-mesure |
|---|---|
| Inventer un registre, un MSR, un bit | citer la section du SDM dans le commit |
| Confondre 32 et 64 bits | compiler et tester, jamais « ça devrait marcher » |
| Oublier un `volatile`/une barrière | relire les accès MMIO, tester en QEMU |
| Copier du code Linux (GPL) | écrire depuis les specs, jamais depuis le source |
| Affirmer qu'un test passe | **coller la sortie réelle** du test |
| Ignorer un warning | `#![deny(warnings)]` à terme |

Règle : **aucune affirmation de succès sans sortie de commande**.

---

## 5. Format d'une demande (côté humain)

```
Objectif : ...
Contrainte : ...
Test d'acceptation : ...
Fichiers pressentis : ...
```

Exemple :

```
Objectif : allocateur physique bitmap
Contrainte : pas d'allocation dans les IRQ
Test : allouer 100 Mio, libérer, vérifier le compte de pages
Fichiers : kernel/src/mm/phys.rs, kernel/src/mm/mod.rs
```

---

## 6. Format d'une réponse (côté IA)

```
CE QUI A ÉTÉ FAIT
- ...

CE QUI A ÉTÉ VÉRIFIÉ
$ make test
TEST OK — ...

CE QUI RESTE / RISQUES
- ...

PROCHAINE ÉTAPE
- ...
```

Pas de promesse, pas de « devrait fonctionner ». Des faits et des logs.

---

## 7. Definition of Done

Une tâche est terminée quand :

1. `cargo build --release` passe **sans warning**.
2. `make test` passe.
3. Le test spécifique de la tâche passe.
4. Aucune régression sur les jalons précédents.
5. `docs/JOURNAL.md` est mis à jour.
6. Le commit est atomique et son message décrit le **pourquoi**.

---

## 8. Gestion du matériel réel

L'IA ne peut pas tester sur un PC physique. Protocole :

1. L'humain boote l'ISO sur la machine cible.
2. Il photographie l'écran ou branche un câble série.
3. Il colle :
   - le modèle exact de la machine ;
   - la sortie série (ou la photo) ;
   - ce qui devait se passer.
4. L'IA analyse et propose un correctif.
5. L'humain re-teste.

**Sans log, pas de diagnostic.** « Ça ne marche pas » n'est pas exploitable.

---

## 9. Gestion de la dette et des risques

- Un `TODO` doit référencer un jalon (`TODO(J4): ...`).
- Un contournement doit être documenté dans `ARCHITECTURE.md`.
- Un test désactivé doit avoir une raison et une date.
- Tous les 5 jalons : revue d'architecture (le document est-il toujours vrai ?).

---

## 10. Ce que l'IA ne fera pas

- Prétendre avoir testé sur du matériel réel.
- Copier du code sous GPL incompatible.
- Modifier `docs/ARCHITECTURE.md` sans le signaler.
- Supprimer un test pour faire passer la CI.
- Ajouter une dépendance sans justification.
- Prendre une décision d'architecture sans validation humaine.

---

## 11. Prochaine itération

**J2.2 — Memory map**

```
Objectif : lire et afficher la carte mémoire (UEFI ou E820)
Contrainte : ne pas allouer avant que l'allocateur physique existe
Test : le noyau affiche la RAM totale et utilisable au boot
Fichiers : kernel/src/mm/, kernel/src/main.rs
```

J2.1 (boot UEFI) est terminé : ISO hybride + ESP disque, `make test-uefi`.

Puis J2.3 (paging higher-half), J2.4 (allocateurs), J2.5 (IDT/exceptions),
J2.6 (APIC/timer).
