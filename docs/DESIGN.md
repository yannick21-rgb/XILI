---
type: design
status: proposition
---

# Design — XILI

> **Statut : PROPOSITION.** Chaque valeur de ce document est une proposition
> de l'agent, pas une décision. Rien ici n'est gelé tant que Jean Yannick ne
> l'a pas validé. Les décisions de design sont les siennes (voir
> `AI-WORKFLOW.md` §10). Ce document se corrige, il ne se réécrit pas.

---

## 1. Raison d'être

XILI est un système **qu'on peut lire d'un coup d'œil**.

Le nom est celui retenu dans le dépôt, mais aucune signification n'est
écrite nulle part : c'est une lacune à combler. Trois candidats, à trancher :

| # | Signification proposée | Ce qu'elle implique |
|---|---|---|
| A | « xili » — variance de *sily* / *système* : un jeu sur « le système » | identité technique, discrète, presque abstraite |
| B | « X-ILI » — *x* comme l'inconnu, *ILI* comme racine de « ile » (île) : un territoire à soi | identité d'espace, de refuge, de calme |
| C | Aucune signification : un nom propre, comme Haiku ou Minix | liberté totale, mais aucune histoire à raconter |

**À trancher :** quel sens, et est-ce que le nom porte le design ou le design
porte le nom.

La phrase qui résume l'identité (proposition) :

> Un système calme, lisible, qui ne demande rien — et qui ne déçoit pas.

---

## 2. Principes

Quatre principes, énoncés comme des arbitrages pour rester utilisables :

1. **Lisible avant joli.** L'information porte le design, pas l'inverse.
2. **Matière honnête.** Le rendu est logiciel : on ne simule pas la profondeur,
   on l'évite.
3. **Calme par défaut.** Le système ne sollicite pas l'attention. Les
   notifications et les animations existent, elles ne s'imposent pas.
4. **Cohérent ou absent.** Mieux vaut une seule couleur et une seule typo que
   trois conventions qui se marchent dessus.

---

## 3. Budget de rendu

C'est la section qui compte le plus, et la seule qui soit une contrainte
**vérifiable**.

| Effet | llvmpipe (réel, J7) | lavapipe / GPU (cible, J7+) | Verdict |
|---|---|---|---|
| Remplissage uni, rectangles | oui, gratuit | oui | **de base** |
| Ligne fine (1 px), bordures nettes | oui, gratuit | oui | **de base** |
| Coins arrondis | oui, formes précalculées | oui | **de base** |
| Opacité (transform + alpha) | oui, tant que peu de couches | oui | **de base** |
| Animation de transform / opacité | oui, 60 fps si ≤ 2–3 couches | oui | **de base** |
| Ombre portée douce, grand rayon | **non** à 60 fps | partiel | **différé** |
| Flou gaussien / backdrop-filter | **non** | partiel | **différé** |
| Translucidité en couches multiples | **non** | partiel | **différé** |
| Matériaux, reflets, dégradés lourds | **non** | oui | **différé** |

**Conséquence écrite noir sur blanc :** la translucidité et le flou ne sont
**pas** le langage visuel de XILI. Ce sont des luxes de la cible GPU, pas des
fondations. Le langage tient en : aplats, filets, typographie, mouvement.

C'est aussi la réponse à la tension que `ARCHITECTURE.md` §11 laisse ouverte :
« sensation macOS » et « llvmpipe d'abord » sont contradictoires si la sensation
voulue est celle du verre dépoli. La sensation retenue ici est celle de
**l'outil bien rangé**, pas celle du verre.

---

## 4. Palette

Une palette de cinq tokens, un thème clair et un thème sombre. Le sombre est
le thème par défaut (un système calme se regarde plus souvent le soir).

| Token | Sombre (défaut) | Clair | Usage |
|---|---|---|---|
| `bg` | `#16181D` | `#F6F6F4` | fond de l'espace de travail |
| `surface` | `#1E2128` | `#FFFFFF` | panneaux, fenêtres, barres |
| `text` | `#E6E7EA` | `#1B1D22` | texte courant |
| `text-dim` | `#8B8F99` | `#6B6E76` | texte secondaire, libellés |
| `primary` | `#7FA6C9` | `#3E6E9E` | actions principales, focus |
| `accent` | `#C9A67F` | `#9E7038` | accent rare : sélection, marqueur |
| `rule` | `#2A2E36` | `#DEDEDA` | filets, séparateurs, bordures |

Règles d'usage :

- **Un seul accent à la fois** à l'écran. L'accent est une exception, pas une
  habitude.
- `rule` porte la séparation, jamais une ombre. Pas d'ombre portée dans le
  thème par défaut.
- Le contraste `text` / `bg` vise WCAG AA (≥ 4.5:1) ; c'est un plancher, pas un
  objectif de style.

---

## 5. Typographie

Deux faces, pas trois. La typographie est le principal vecteur d'identité
(disposition, interlignage, graisse).

| Rôle | Face | Détail |
|---|---|---|
| Interface | sans-serif à œil bas, large x-height | style Inter / IBM Plex Sans |
| Mono | monospace à bonne distinction `0/O`, `1/l/I` | style JetBrains Mono / IBM Plex Mono |

| Jeton | Valeur | Usage |
|---|---|---|
| `text-xs` | 12 px | libellés, horodatages, états |
| `text-sm` | 14 px | texte d'interface courant |
| `text-md` | 16 px | texte de lecture |
| `text-lg` | 20 px | titres de panneau |
| `text-xl` | 28 px | titres d'écran, rare |

- Hauteur de ligne : 1.45 pour l'interface, 1.6 pour la lecture.
- Graisse : 400 pour le courant, 500 pour l'emphase, **jamais 700** pour le
  corps de texte. Le gras est réservé aux titres.
- Le mono sert à tout ce qui s'aligne : chiffres, chemins, états, journaux.
  Le terminal est un citoyen de première classe, pas un mode dégradé.

---

## 6. Géométrie

| Jeton | Valeur | Note |
|---|---|---|
| `radius-sm` | 4 px | contrôles, puces |
| `radius-md` | 8 px | fenêtres, panneaux |
| `radius-lg` | 12 px | modales, écrans |
| espacement | échelle 4 pt (4, 8, 12, 16, 24, 32, 48) | une seule échelle |
| `hit` | 32 px minimum | cibles tactiles et souris |
| élévation | 3 niveaux, portés par `rule` + léger décalage, **pas** par ombre | |

Les coins arrondis sont le seul ornement structurel du système. Ils sont
cohérents parce qu'ils sont peu nombreux.

---

## 7. Mouvement

Le mouvement existe pour expliquer une transition, jamais pour orner.

| Jeton | Valeur |
|---|---|
| `dur-fast` | 120 ms |
| `dur-base` | 200 ms |
| `dur-slow` | 320 ms |
| easing | cubic-bezier(0.2, 0.0, 0.0, 1) |

- **Seules** la position, la taille et l'opacité animent. Jamais la couleur,
  jamais la forme.
- Une animation ne dépasse **jamais** `dur-slow` : au-delà, c'est un signe que
  l'information devrait être réorganisée, pas animée.
- Un changement d'état sans déplacement n'anime pas.
- Le mouvement se désactive globalement (préférence « réduire les animations »)
  et doit se désactiver proprement.

---

## 8. Voix

Court, neutre, à l'impératif pour les actions. Pas de familiarité, pas de
sollicitation.

- Fait : « Enregistrer », « Redémarrer », « 3 fichiers déplacés ».
- Évite : « Hé ! », « Super, c'est fait ! », « Voulez-vous vraiment… ? » pour
  une action réversible.
- Une erreur dit ce qui s'est passé et ce qu'on peut faire ensuite. Jamais de
  code brut sans explication.

---

## 9. Décisions ouvertes

Ce sont les points qui bloquent, et ce qui les lèverait.

| # | Décision | Ce qui la lève |
|---|---|---|
| 1 | Signification du nom XILI | le choix A/B/C de la section 1 |
| 2 | Thème par défaut (sombre ou clair) | un usage réel ; provisoirement sombre |
| 3 | Résolution et densité de référence | le choix de la machine cible (J10) |
| 4 | Faces typographiques exactes | une maquette qui les montre côte à côte |
| 5 | Statut de « sensation macOS » dans `ARCHITECTURE.md` §11 | la section 3 : le reformuler ou l'abandonner explicitement |
| 6 | Couleur d'accent définitive | une maquette ; la proposition ci-dessus est un point de départ |

---

## 10. Comment ce document est utilisé

1. **Figer les jetons** (sections 4 à 7). C'est le seul contenu qui ne bouge
   plus après validation.
2. **Prototyper le compositeur hors noyau** — HTML/CSS/Canvas, dès maintenant,
   en parallèle de J2 → J6. C'est là que les jetons prennent vie, que le budget
   de rendu se vérifie, et que les espacements s'ajustent en secondes.
3. **En J7, porter le prototype** dans le compositeur maison. Le design existe
   déjà et est validé ; on porte, on ne redessine pas.

Règle de maintenance : toute nouvelle valeur visuelle entre dans ce document
avant d'entrer dans le code, et une valeur non documentée n'existe pas.
