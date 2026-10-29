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
| Lisière de la fenêtre active : **bande 2 px seule** (spread, sans flou) | oui | oui | **de base** |
| Lisière : bande + **halo flou** de rayon court, **une seule fenêtre** | **non** | oui | **exception mesurée — §10.5** |
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

## 4. Palette — « Encre et soleil »

Identité visuelle **Encre et soleil**, validée par Jean Yannick le 2026-10-28
(`xili-design/Palette.pdf`). Elle remplace la palette précédente : deux
températures opposées, l'encre froide qui porte l'espace et le soleil chaud
qui porte l'attention. Le sombre est le thème par défaut (un système calme se
regarde plus souvent le soir).

### 4.1 Couleurs de marque (les quatre du document)

| Nom | Valeur | Usage déclaré |
|---|---|---|
| Encre | `#12151F` | fond |
| Ivoire | `#F6F1E7` | texte, logo |
| Orange brûlé | `#FF6B2C` | accent principal |
| Safran | `#FFC53D` | accent secondaire |

### 4.2 Jetons d'interface

Les quatre couleurs de marque ne suffisent pas à habiller une interface : il
faut une `surface` pour les panneaux, un `rule` pour les filets et un
`text-dim` pour le secondaire. Ces trois-là sont **dérivés par interpolation
entre Encre et Ivoire** — ils n'inventent aucune teinte, ils prolongent le
duo. `primary` et `accent` reçoivent les rôles suivants (validés) :
**l'orange porte le focus et les actions, le safran est l'accent rare.**

| Token | Sombre (défaut) | Clair | Usage |
|---|---|---|---|
| `bg` | `#12151F` | `#F6F1E7` | fond de l'espace de travail |
| `surface` | `#20222B` | `#FBF9F5` | panneaux, fenêtres, barres |
| `text` | `#F6F1E7` | `#12151F` | texte courant |
| `text-dim` | `#8F8E8D` | `#69696B` | texte secondaire, libellés |
| `primary` | `#FF6B2C` | `#BF3900` | focus, actions, liens, lisière |
| `accent` | `#FFC53D` | `#8E6400` | accent rare : sélection, marqueur |
| `rule` | `#2B2D35` | `#DBD7CF` | filets, séparateurs, bordures |

Dérivation : `surface` = Encre→Ivoire à 6 %, `rule` = 11 %, `text-dim` = 55 %
(sombre) / Ivoire→Encre à 62 % (clair).

### 4.3 Assombrissement des accents dans le clair

**Contrainte vérifiée, pas une préférence.** L'orange et le safran sont des
couleurs de lumière : sur l'encre ils éclairent, sur l'ivoire ils
s'éteignent. Mesures sur `#F6F1E7` :

| Couleur | Sur Encre | Sur Ivoire | Verdict |
|---|---|---|---|
| Orange `#FF6B2C` | 6.41:1 | **2.52:1** | refusé en clair : sous le plancher de 3:1 |
| Safran `#FFC53D` | 11.54:1 | **1.40:1** | refusé en clair : invisible |

Le thème clair assombrit donc ses deux accents, en conservant la teinte
(assombrissement HLS, saturation intacte) : orange → `#BF3900` (4.91:1),
safran → `#8E6400` (4.70:1). C'est la même mécanique que la palette
précédente, qui passait `primary` de `#7FA6C9` à `#3E6E9E`.

**Conséquence :** le soleil éclate sur le sombre et se discipline sur le
clair. Ce n'est pas un compromis, c'est la lecture du duo.

### 4.4 Règles d'usage

- **Un seul accent à la fois** à l'écran. L'accent est une exception, pas une
  habitude.
- `rule` porte la séparation, jamais une ombre. Pas d'ombre portée dans le
  thème par défaut.
- Le contraste `text` / `bg` vise WCAG AA (≥ 4.5:1) ; c'est un plancher, pas
  un objectif de style. Vérifié pour tous les jetons ci-dessus.
- **Exception à la palette** (validée 2026-10-28) : la lisière lumineuse de la
  fenêtre active (§10.5) est **libre** — l'utilisateur peut y mettre la couleur
  qu'il veut. C'est sa fenêtre, sa couleur. La palette reste verrouillée pour
  tout le reste du système.

### 4.5 La marque

Source : `xili-design/XILI-logo.svg` fourni par Jean Yannick (2026-10-28),
recadrée, coloriée et mesurée dans `assets/logo/xili-marque-couleur.svg`.
Forme : **deux disques** (rayon
100) aux centres (197.6, 286.3) et (397.6, 486.3), plus un **ruban en S** dont
deux arcs coïncident avec la circonférence de ces disques. Les décalages entre
les trois formes sont produits par le `stroke`, pas par des trous dans les
tracés — la marque est donc un assemblage plein, jamais un contour creusé.

**Couleur : en couleurs.** (Décision Jean Yannick, 2026-10-28 — « met de la
couleur dans le logo ».) L'affectation est **mesurée sur la planche**, pas
choisie à l'œil :

| Élément | Teinte |
|---|---|
| le ruban en S | Orange brûlé `#FF6B2C` |
| les deux disques | Safran `#FFC53D` |
| les vides entre les formes | Encre `#12151F` (le `stroke`) |

Les teintes sont **fixes** : la planche emploie le même orange et le même
safran sur ses trois fonds. Seul le ruban passe en ivoire sur fond orange —
variante que l'interface n'utilise pas.

### 4.5.1 La plaque

La planche pose la marque sur un **carré encre arrondi** dès que le fond
n'est pas déjà de l'encre. On l'inclut donc **toujours** : sur fond sombre
elle se confond, sur fond clair elle porte la marque. Un seul fichier, tous
les fonds.

C'est la plaque qui tient le contraste, jamais les teintes chaudes seules —
sur Ivoire l'orange tombe à 2,52:1 (§4.3). La marque n'est jamais posée nue
sur un fond clair.

**La plaque vaut exactement l'emprise de la marque, strokes compris — aucune
marge.** Mesuré à taille native sur la tuile ivoire de la planche :

| | plaque | boîte chaude | ratio |
|---|---|---|---|
| planche | 107×107 px | 80×80 px | **74,8 %** |
| notre SVG | 107×107 px | 78×78 px | **72,9 %** |

L'écart de 2 px est de l'antialiasing entre deux rasteriseurs. Une première
version portait la plaque à 540 unités (marge de 40) : la boîte chaude y
tombait à **62,3 %**, deux fois trop serré. La règle retenue — **la plaque
n'est pas un cadre autour du logo, c'est son gabarit** (460 unités).

**Rayon : 98 unités, soit 21,3 % du côté.** Mesuré par **ajustement circulaire**
sur le profil du coin de la tuile ivoire (`inset(k) = r − √(r² − (r−k)²)`) :
la courbe s'arrête à k=9 sur un côté de 108. La méthode est validée — en
l'appliquant à notre propre rendu elle rend 18,5 %, qui est exactement la
valeur connue (85/460), donc elle ne ment pas. Un premier calcul par simple
profil donnait 18,5 % pour la planche : **faux**, il s'arrêtait au premier
pixel nul, faussé par l'antialiasing. C'est la comparaison agrandie à 4× qui
a trahi l'erreur, pas la mesure native — à l'échelle 1:1 les deux coins
semblaient identiques.

| Fichier | Rôle |
|---|---|
| `assets/logo/xili-marque-couleur.svg` | **marque de référence** — plaque + teintes figées |
| `assets/logo/xili-favicon.svg` | idem — un favicon n'hérite de rien |
| `prototype/index.html` | la même, inlinée dans la barre haute (un `<img>` ne reçoit pas les variables) |
| `assets/logo/xili-marque.svg` | **variante une seule encre**, par variables CSS — non utilisée par l'interface aujourd'hui ; réservée à l'impression monochrome et au gravé |

**Taille minimale : 24 px.** Vérifié au rendu : à 16 px la marque se réduit à
quatre points et perd sa lecture. En dessous de 24 px, utiliser une forme
réduite ou le mot seul. La barre haute la porte donc à 24 px.

**Licence typographique :** la police **Lyl Bah Demo** est « Free For Personal
Use », **logotype uniquement** — jamais dans l'interface, jamais pour du texte
courant, jamais de chiffres ni de ponctuation (glyphe « 0 » filigrané). Le mot
XILI n'apparaît donc que dans la barre et les documents de marque.

---

## 5. Typographie

Deux faces, pas trois. La typographie est le principal vecteur d'identité
(disposition, interlignage, graisse).

### 5.1 Les deux faces — décision 2026-10-29

| Rôle | Face retenue | Mesure |
|---|---|---|
| Interface | **Inter** | x-height **55 %**, accents français complets |
| Mono | **JetBrains Mono** | x-height **55 %**, zéro pointé, `l` à patte, `I` à empattements |

Binôme retenu par Jean Yannick le 2026-10-29, devant trois autres
(`xili-design/specimen-typo.html`) : Inter + Plex Mono, Plex Sans + Plex Mono,
Plex Sans + JetBrains Mono. Toutes étaient sous **SIL OFL** et couvraient les
accents ; le critère décisif a été le **rendu réel en 12 px**, la taille de la
barre haute et la plus utilisée du bureau.

- **Pourquoi Inter plutôt que Plex Sans :** x-height 55 % contre 52 %. Mesuré
  au rendu en 12 px, Inter reste ouvert là où Plex Sans se resserre. Le
  principe 1 (« lisible avant joli ») tranche en faveur de la hauteur d'x.
  Contre : Inter est la plus générique des candidates — c'est le prix assumé
  d'une typo qui ne se remarque pas.
- **Pourquoi JetBrains Mono plutôt que Plex Mono :** même x-height (55 % vs
  52 %), 85 Ko de moins, et les glyphes critiques sont dans la source.
- **Distinction `0/O`, `1/l/I`** : vérifiée au rendu pour les deux monos. Les
  deux passaient ; c'est l'x-height qui a départagé.

**Note de lisibilité :** « Œil bas » et « grande hauteur d'x » sont deux
indications opposées. La phrase d'origine du §5 (« sans-serif à œil bas,
large x-height ») était ambiguë — elle a été corrigée ici. La valeur retenue
est **x-height haute**, et c'est elle qui a décidé du choix.

### 5.2 Distribution

| Fichier | Rôle | Poids |
|---|---|---|
| `assets/fonts/inter-latin.woff2` | interface, livré | 117 Ko |
| `assets/fonts/jetbrainsmono-latin.woff2` | mono, livré | 46 Ko |
| `assets/fonts/OFL-*.txt` | licences, livrées avec | — |
| `xili-design/fonts-evaluation/*.ttf` | sources complètes, travail | 1,6 Mo |

Sous-ensembles générés par `pyftsubset` : latin, latin-1 supplémentaire
(accents français), latin étendu A (Œ œ), ponctuation générale, opérateurs
mathématiques (`≡`), alphanumériques divers (`▸`, `◫`). Axes variables
conservés (`opsz`, `wght` pour Inter ; `wght` pour JetBrains Mono).

Licence : **SIL OFL 1.1** — redistribuable et embarquable. C'est ce qui a
écarté **Lyl Bah Demo** (« Free For Personal Use », glyphe `0` filigrané) du
reste du système : elle reste réservée au logotype.

**Règle des glyphes :** une police n'a pas tous les signes. `✕` (U+2715) et
`⤢` (U+2922) sont absents d'Inter, `≡` (U+2261) l'était de mon premier
sous-ensemble. Toute **icône d'interface se dessine en SVG**, jamais en glyphe
de police : c'est indépendant de la fonte, net à toute taille, et c'est
exactement ce que le compositeur devra faire en J7. Le texte courant, lui, ne
doit employer que des caractères présents dans les deux sous-ensembles.

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
| `radius-md` | **12 px** | fenêtres, panneaux |
| `radius-lg` | **20 px** | modales, écrans |
| espacement | échelle 4 pt (4, 8, 12, 16, 24, 32, 48) | une seule échelle |
| `hit` | 32 px minimum | cibles tactiles et souris |
| élévation | 3 niveaux, portés par `rule` + léger décalage, **pas** par ombre | |

Les trois rayons restent des multiples de 4, dans la même échelle que
l'espacement. Ils ont été **ouverts le 2026-10-29** (8 / 12 → 12 / 20) sur le
modèle de l'esthétique COSMIC/Pop!_OS : des fenêtres plus arrondies lisent
comme des objets posés, pas comme des cadres. C'est un ajustement de
caractère, pas un changement de système — le nombre de jetons n'a pas bougé,
et les coins restent le seul ornement structurel.

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

| # | Décision | Statut |
|---|---|---|
| 1 | Signification du nom XILI | **ouverte** — le choix A/B/C de la section 1 |
| 2 | Thème par défaut (sombre ou clair) | **tranchée** (Jean Yannick, 2026-10-28) : sombre |
| 3 | Résolution et densité de référence | **tranchée** (2026-10-28) : 1280×800 — le petit écran comme contrainte |
| 4 | Faces typographiques exactes | **ouverte** — une maquette qui les montre côte à côte |
| 5 | Statut de « sensation macOS » dans `ARCHITECTURE.md` §11 | **ouverte** — la section 3 : le reformuler ou l'abandonner explicitement |
| 6 | Couleur d'accent définitive | **tranchée** (2026-10-28) : palette « Encre et soleil » (§4) — orange brûlé `#FF6B2C` en principal, safran `#FFC53D` en secondaire ; assombrissements clairs au §4.3 |
| 7 | Rapport à l'esthétique Pop!_OS / COSMIC | **tranchée** (Jean Yannick, 2026-10-29) : on emprunte la **mosaïque automatique** (§10.6) et l'**esthétique** (coins arrondis ouverts, §6). **Palette conservée** — Encre et soleil ne bouge pas. Ni dock, ni panneau latéral, ni lanceur, ni contrôles en haut à droite |
| 8 | Halo lumineux de la lisière | **tranchée** (2026-10-29) : flou réel au-dessus de la bande 2 px. Exception §3 **provisoire** — le coût llvmpipe ne se mesurera qu'à J7 (§10.5) |

---

## 10. Composition du bureau

Décisions arrêtées par Jean Yannick le 2026-10-28, avant toute ligne de code
de la maquette.

### 10.1 Structure de l'écran

**Une barre en haut, une seule.** Pas de dock, pas de panneau latéral.

| Zone | Contenu |
|---|---|
| Gauche | Menu XILI (démarreur, thème, animations, préférences) |
| Centre | Tâches : une entrée par fenêtre ouverte ; l'active est marquée au filet `primary` |
| Droite | Bascule de thème, horloge (mono, `text-xs`) |

Justification : maximise l'espace vertical, tient sur 1280×800, et cohérent
avec « l'outil bien rangé ». Le dock est refusé : c'est un deuxième système
de lancement qui marche sur le même principe que la barre des tâches.

### 10.2 Fenêtres

- **Barre de titre en haut** : nom de l'application à gauche (`text-xs`),
  déplacement par glisser. Pas de boutons ici.
- **Contrôles en bas à droite** : `Fermer · Réduire · Agrandir`, de gauche à
  droite, dans cet ordre, au-dessus de la poignée de redimensionnement.
  C'est le choix validé, contre la convention macOS et la convention
  Windows : la fenêtre garde un dessus calme, les contrôles sont où le
  regard arrive déjà (le coin bas-droit est celui du travail fini).
- **Élévation** : la fenêtre active porte un filet `primary` ; les autres un
  filet `rule`. Aucune ombre, jamais.
- Ouverture : déplacement vertical de 8 px + opacité, `dur-base`.

### 10.3 Le bureau

Le bureau est **vide par défaut et épinglable** : fichiers, dossiers,
applications — n'importe quoi se pose dessus.

- Sélection = filet `accent` (la seule apparition courante de l'accent).
- Épingles : icône + libellé `text-xs`, grille libre, glisser-déposer.
- Un épinglé s'ouvre avec l'application qui lui correspond ; le déplacement
  d'une épine se fait par glisser.

### 10.4 Démonstration au chargement

La maquette ouvre **Terminal + Fichiers**, côte à côte. Le terminal est un
citoyen de première classe (§5) et se prouve en le manipulant ; les fichiers
montrent la navigation et l'épinglage.

### 10.5 Lisière lumineuse (décision 2026-10-28)

Chaque fenêtre est **entourée d'une bande fine et lumineuse**, avec ces règles :

| Réglage | Valeur retenue |
|---|---|
| Portée | **fenêtre active uniquement** ; les autres gardent le filet `rule` |
| Couleur | **libre** — sélecteur de couleur dans le menu XILI (exception §4) |
| Épaisseur | filet **1 px** (uniforme avec les fenêtres inactives) + bande **2 px** = **3 px** de lecture. Le filet reste à 1 px au focus : passer à 2 px décalerait le contenu de 1 px à chaque changement de focus |
| **Bande** | **2 px nets, sans flou** — `box-shadow` en `spread` pur. C'est elle qui garde le bord lisible |
| **Halo** | **lumineux, flou réel** — `box-shadow` de rayon court (14 px) sous la bande. Décision Jean Yannick, 2026-10-29 : « le halo doit être lumineux ». Une seule fenêtre porte le halo : la fenêtre active |
| Mouvement | **statique** : la lisière apparaît/disparaît avec le focus, sans pulsation |

**Les deux coexistent** : la bande de 2 px donne l'arête nette, le halo donne
la lumière. La bande seule est lisible mais morte ; le halo seul est lumineux
mais flou au bord. Ensemble, l'arête reste nette et la lisière irradie.

Le filet de 1 px et la bande de 2 px sont de la **même couleur et contigus** :
ils fusionnent en une seule bande de 3 px autour de la fenêtre active, puis
le halo s'essore au-delà.

Justification : la lisière répond à la question « où est mon attention ? »
mieux qu'un simple filet, et une seule fenêtre lumineuse à la fois reste
« calme par défaut ». Le choix de la couleur est personnel : c'est la seule
pièce du système qui échappe à la palette, et c'est assumé.

**Coût (§3) — mesuré honnêtement.** La bande seule est un aplat, donc « de
base ». **Le halo flou est une convolution** : il a été retiré de la ligne
« de base » du tableau §3 et remis en **exception**, sur deux conditions qui
sont aussi ses limites :

1. **une seule fenêtre** à l'écran porte un halo — les fenêtres inactives ne
   portent que leur filet `rule`, jamais de halo ;
2. **un seul `box-shadow`**, pas une pile de couches.

Le repli si le budget casse en réel est écrit ici : retirer le halo, garder
la bande 2 px. La lisière reste lisible, elle perd juste la lumière.

> **Limite de la mesure.** La maquette tourne dans un navigateur hôte, qui
> accélère les `box-shadow` par le GPU. **Le coût réel en llvmpipe ne peut
> pas être mesuré depuis ici** — il se mesure à J7, sous QEMU. Tant que cette
> mesure n'existe pas, l'exception est **provisoire** et le repli (bande
> seule) est considéré comme le comportement sûr. C'est écrit plutôt que
> prétendu mesuré.

---

### 10.6 Mosaïque automatique (décision 2026-10-29)

Reprise de la **signature de Pop!_OS / COSMIC** : les fenêtres s'emboîtent
plutôt que de se chevaucher. C'est la seule pièce fonctionnelle empruntée — la
disposition de l'écran (panneau, dock, lanceur) et les couleurs restent celles
de XILI (§4, §10).

#### Modèle : arbre de partages binaires

Un nœud est soit un **partage** (orientation + ratio), soit une **feuille**
(id de fenêtre). La racine couvre le bureau entier, marges comprises.

| Opération | Effet |
|---|---|
| **Ouvrir** une fenêtre | on partage la feuille active en deux ; la nouvelle occupe la seconde moitié |
| **Fermer** une fenêtre | la feuille disparaît ; le partage qui la contenait s'effondre sur sa sœur |
| **Focus** directionnel | la fenêtre la plus proche dans cette direction (géométrique, centre à centre) |
| **Échanger** directionnel | les deux fenêtres échangent leur place dans l'arbre |
| **Tourner** | le partage courant passe de vertical à horizontal et inversement |
| **Flottant** | une fenêtre peut sortir de l'arbre et redevenir libre (glisser, redimensionner) |

Les ratios sont **égaux par défaut** (0.5) ; le redimensionnement au poignée
agit sur le ratio du partage parent, pas sur la taille absolue de la fenêtre.

#### Ce que la mosaïque ne change pas

- La **barre haute** reste unique, et les **contrôles de fenêtre** restent en
  bas à droite — ces décisions (§10) ne bougent pas.
- Le **glisser** et le **redimensionnement** restent disponibles : ce sont eux
  qui font sortir une fenêtre de l'arbre, en mode flottant.
- La **lisière** ne change pas : une seule fenêtre active reste lumineuse.

#### Raccourcis

| Raccourci | Action |
|---|---|
| `Ctrl+Alt+M` | basculer la mosaïque |
| `Ctrl+Alt+←→↑↓` | diriger le focus |
| `Ctrl+Alt+Maj+←→↑↓` | échanger la fenêtre |
| `Ctrl+Alt+R` | tourner le partage courant |
| `Ctrl+Alt+G` | basculer flottant / mosaïque pour la fenêtre active |

#### Pourquoi c'est là avant J7

Le compositeur cible est un poste de travail où l'on regarde plusieurs choses
à la fois. La mosaïque est la réponse structurelle à cette question, et la
maquette sert à **la valider avant d'en payer le coût en noyau** : un
gestionnaire de partages binaires est du calcul de géométrie pur, sans
dépendance matérielle — c'est précisément le genre de chose qu'on veut tester
en HTML avant de l'écrire en Rust.

---

## 11. Comment ce document est utilisé

1. **Figer les jetons** (sections 4 à 7). C'est le seul contenu qui ne bouge
   plus après validation.
2. **Prototyper le compositeur hors noyau** — HTML/CSS/Canvas, dès maintenant,
   en parallèle de J2 → J6. C'est là que les jetons prennent vie, que le budget
   de rendu se vérifie, et que les espacements s'ajustent en secondes.
3. **En J7, porter le prototype** dans le compositeur maison. Le design existe
   déjà et est validé ; on porte, on ne redessine pas.

Règle de maintenance : toute nouvelle valeur visuelle entre dans ce document
avant d'entrer dans le code, et une valeur non documentée n'existe pas.
