/* ==========================================================================
   XILI — maquette du bureau
   Gestionnaire de fenêtres, épingles, terminal, fichiers, lisière.
   Décisions : docs/DESIGN.md §10 (barre haute, contrôles bas-droite,
   bureau épinglable, lisière libre sur la fenêtre active).
   ========================================================================== */

"use strict";

const $ = (sel, racine = document) => racine.querySelector(sel);

const bureau = $("#bureau");
const zoneEpingles = $("#epingles");
const modeleFenetre = $("#modele-fenetre");

function creerElement(html) {
  return document.createRange().createContextualFragment(html).firstElementChild;
}

function echapper(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/* --- Lisière (§10.5 : couleur libre, une seule fenêtre lumineuse) ------------ */

const entreeCouleur = $("#couleur-lisiere");

/* Tant que l'utilisateur n'a pas choisi, la lisière suit le thème
   (--lisiere = --primary, cf. style.css). Le choix manuel fige la couleur. */
let lisierePersonnalisee = false;

function appliquerLisiere(couleur) {
  document.documentElement.style.setProperty("--lisiere", couleur);
  lisierePersonnalisee = true;
}

function synchroniserLisiere() {
  // À l'ouverture du menu, on montre la valeur réellement en vigueur.
  const effective = getComputedStyle(document.documentElement)
    .getPropertyValue("--lisiere").trim();
  if (/^#[0-9a-fA-F]{6}$/.test(effective)) entreeCouleur.value = effective;
}

entreeCouleur.addEventListener("input", () => appliquerLisiere(entreeCouleur.value));

document.querySelectorAll(".menu-pastille").forEach((p) => {
  p.addEventListener("click", () => appliquerLisiere(p.dataset.lisiere));
});

/* --- Gestionnaire de fenêtres ------------------------------------------------- */

const fenetres = new Map();
let compteurId = 0;
let idActif = null;
let zSuivant = 100;

const TAILLE_DEFAUT = { w: 560, h: 380 };

function ouvrirFenetre(titre, contenu, opts = {}) {
  if (!opts.multi) {
    for (const [id, f] of fenetres) {
      if (f.titre === titre) { focusFenetre(id); return id; }
    }
  }

  const id = ++compteurId;
  const el = modeleFenetre.content.firstElementChild.cloneNode(true);
  const corps = $(".fenetre-corps", el);
  $(".fenetre-nom", el).textContent = titre;
  corps.appendChild(contenu);

  const w = Math.min(opts.w || TAILLE_DEFAUT.w, bureau.clientWidth - 16);
  const h = Math.min(opts.h || TAILLE_DEFAUT.h, bureau.clientHeight - 16);
  const decalage = (id % 5) * 24;
  const x = Math.max(8, Math.min(
    Math.round((bureau.clientWidth - w) / 2) + decalage - 60,
    bureau.clientWidth - w - 8));
  const y = Math.max(8, Math.min(
    Math.round((bureau.clientHeight - h) / 2) + decalage - 40,
    bureau.clientHeight - h - 8));

  Object.assign(el.style, {
    width: w + "px",
    height: h + "px",
    left: x + "px",
    top: y + "px",
    zIndex: ++zSuivant,
  });

  el.classList.add("ouverte");
  el.addEventListener("animationend", () => el.classList.remove("ouverte"), { once: true });

  fenetres.set(id, { el, corps, titre, maximisee: false, minimisee: false,
                     flottante: false, geo: null });
  bureau.appendChild(el);
  installerFenetre(id);
  focusFenetre(id);
  if (mosaiqueActivee) { mosaiqueAjouter(id); appliquerMosaique(); }
  majBarreTaches();
  return id;
}

function focusFenetre(id) {
  const f = fenetres.get(id);
  if (!f) return;
  if (f.minimisee) restaurerFenetre(id);

  idActif = id;
  f.el.style.zIndex = ++zSuivant;
  for (const [autre, g] of fenetres) g.el.classList.toggle("active", autre === id);
  majBarreTaches();

  const focusable = f.el.querySelector("input, button, [tabindex='0']");
  (focusable || f.el).focus({ preventScroll: true });
}

function fermerFenetre(id) {
  const f = fenetres.get(id);
  if (!f) return;
  fenetres.delete(id);
  f.el.remove();
  if (mosaiqueActivee) { mosaiqueRetirer(id); appliquerMosaique(); }
  if (idActif === id) {
    idActif = null;
    const restantes = [...fenetres.keys()];
    if (restantes.length) focusFenetre(restantes[restantes.length - 1]);
  }
  majBarreTaches();
}

function reduireFenetre(id) {
  const f = fenetres.get(id);
  if (!f || f.minimisee) return;
  f.minimisee = true;
  f.el.style.opacity = "0";
  f.el.style.pointerEvents = "none";
  const cacher = () => { if (f.minimisee) f.el.style.visibility = "hidden"; };
  f.el.addEventListener("transitionend", cacher, { once: true });
  setTimeout(cacher, 250);
  if (idActif === id) idActif = null;
  majBarreTaches();
}

function restaurerFenetre(id) {
  const f = fenetres.get(id);
  if (!f || !f.minimisee) return;
  f.minimisee = false;
  f.el.style.visibility = "";
  // Un frame plus tard pour que le navigateur revoie le passage à visible.
  requestAnimationFrame(() => { f.el.style.opacity = "1"; f.el.style.pointerEvents = ""; });
  focusFenetre(id);
}

function basculerAgrandir(id) {
  const f = fenetres.get(id);
  if (!f) return;
  const el = f.el;

  if (f.maximisee && f.geo) {
    Object.assign(el.style, f.geo);
    f.maximisee = false;
  } else {
    f.geo = { left: el.style.left, top: el.style.top, width: el.style.width, height: el.style.height };
    Object.assign(el.style, {
      left: "4px", top: "4px",
      width: (bureau.clientWidth - 8) + "px",
      height: (bureau.clientHeight - 8) + "px",
    });
    f.maximisee = true;
  }
  focusFenetre(id);
}

function installerFenetre(id) {
  const f = fenetres.get(id);
  const el = f.el;

  el.addEventListener("pointerdown", () => focusFenetre(id));

  $('[data-bouton="fermer"]', el).addEventListener("click", () => fermerFenetre(id));
  $('[data-bouton="minimiser"]', el).addEventListener("click", () => reduireFenetre(id));
  $('[data-bouton="agrandir"]', el).addEventListener("click", () => basculerAgrandir(id));
  $(".fenetre-titre", el).addEventListener("dblclick", () => basculerAgrandir(id));

  // Déplacement par la barre de titre.
  const titre = $(".fenetre-titre", el);
  titre.addEventListener("pointerdown", (e) => {
    if (f.maximisee) return;
    e.preventDefault();

    // Une fenêtre tuilée qu'on tire sort de l'arbre (§10.6) : elle redevient
    // flottante à l'endroit où on la lâche. C'est le geste naturel pour
    // « celle-là, je veux la déplacer ailleurs ».
    if (el.classList.contains("en-mosaique")) {
      f.flottante = true;
      el.classList.remove("en-mosaique");
      mosaiqueRetirer(id);
      appliquerMosaique();
    }

    const startX = e.clientX, startY = e.clientY;
    const origL = el.offsetLeft, origT = el.offsetTop;
    titre.setPointerCapture(e.pointerId);
    el.classList.add("deplacee", "sans-transition");

    const bouger = (ev) => {
      el.style.left = Math.max(-el.offsetWidth + 64,
        Math.min(origL + ev.clientX - startX, bureau.clientWidth - 64)) + "px";
      el.style.top = Math.max(0,
        Math.min(origT + ev.clientY - startY, bureau.clientHeight - 24)) + "px";
    };
    const relacher = () => {
      titre.removeEventListener("pointermove", bouger);
      titre.removeEventListener("pointerup", relacher);
      el.classList.remove("deplacee", "sans-transition");
    };
    titre.addEventListener("pointermove", bouger);
    titre.addEventListener("pointerup", relacher);
  });

  // Redimensionnement par la poignée du coin bas-droit.
  const poignee = $(".fenetre-poignee", el);
  poignee.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    focusFenetre(id);

    // En mosaïque, la poignée agit sur le ratio du partage parent :
    // on redimensionne le partage, pas la fenêtre (§10.6).
    const trouv = mosaiqueActivee ? trouverFeuille(racineMosaique, id) : null;
    const partage = trouv && trouv.parent ? trouv.parent : null;

    if (partage) {
      const r = calculerRects(racineMosaique, rectRacineMosaique(), new Map()).get(id);
      const horizontal = partage.or === "h";
      const dimension = horizontal ? r.w : r.h;
      const origRatio = partage.ratio;
      const startX = e.clientX, startY = e.clientY;
      const delta = () => horizontal
        ? (e.clientX - startX) / dimension
        : (e.clientY - startY) / dimension;
      void delta; // (début de geste : aucun mouvement encore)

      poignee.setPointerCapture(e.pointerId);
      el.classList.add("sans-transition");   // sinon la transition suit le pointeur

      const bouger = (ev) => {
        const d = horizontal ? (ev.clientX - startX) / dimension
                             : (ev.clientY - startY) / dimension;
        partage.ratio = Math.max(0.15, Math.min(0.85, origRatio + d));
        appliquerMosaique();
      };
      const relacher = () => {
        poignee.removeEventListener("pointermove", bouger);
        poignee.removeEventListener("pointerup", relacher);
        el.classList.remove("sans-transition");
      };
      poignee.addEventListener("pointermove", bouger);
      poignee.addEventListener("pointerup", relacher);
      return;
    }

    // Mode flottant : largeur / hauteur libres (comportement historique).
    const startX = e.clientX, startY = e.clientY;
    const origW = el.offsetWidth, origH = el.offsetHeight;
    poignee.setPointerCapture(e.pointerId);

    const bouger = (ev) => {
      el.style.width = Math.max(320, origW + ev.clientX - startX) + "px";
      el.style.height = Math.max(200, origH + ev.clientY - startY) + "px";
    };
    const relacher = () => {
      poignee.removeEventListener("pointermove", bouger);
      poignee.removeEventListener("pointerup", relacher);
    };
    poignee.addEventListener("pointermove", bouger);
    poignee.addEventListener("pointerup", relacher);
  });
}

/* --- Mosaïque automatique (§10.6) ------------------------------------------- */

let mosaiqueActivee = false;
let racineMosaique = null;

const MARGE_MOSAIQUE = 12;   // écart extérieur, par rapport au bord du bureau
const ECART_MOSAIQUE = 8;    // écart intérieur, entre deux fenêtres tuilées

/* Nœud : { type: "partage", or: "h"|"v", ratio, enfants: [a, b] }
          | { type: "feuille", id } */

function feuillesMosaique(n) {
  if (!n) return [];
  if (n.type === "feuille") return [n.id];
  return [...feuillesMosaique(n.enfants[0]), ...feuillesMosaique(n.enfants[1])];
}

function trouverFeuille(n, id, parent = null) {
  if (!n) return null;
  if (n.type === "feuille") return n.id === id ? { noeud: n, parent } : null;
  return trouverFeuille(n.enfants[0], id, n) || trouverFeuille(n.enfants[1], id, n);
}

/* Le nouvel onglet partage la feuille de la fenêtre active. L'orientation
   s'inverse à chaque niveau — on obtient une grille, pas une colonne. */
function mosaiqueAjouter(id) {
  const presente = feuillesMosaique(racineMosaique);
  if (presente.includes(id)) return;

  if (!racineMosaique) { racineMosaique = { type: "feuille", id }; return; }

  const cible = trouverFeuille(racineMosaique, idActif) ||
                trouverFeuille(racineMosaique, presente[presente.length - 1]);
  const or = cible && cible.parent ? (cible.parent.or === "h" ? "v" : "h") : "h";

  const partage = { type: "partage", or, ratio: 0.5,
                    enfants: [{ ...cible.noeud }, { type: "feuille", id }] };
  remplacerNoeud(racineMosaique, cible.noeud, partage, null);
}

/* Remplace `ancien` par `nouveau` dans l'arbre ; si l'ancien était la
   racine, la racine elle-même change. */
function remplacerNoeud(n, ancien, nouveau, parent) {
  if (!n) return false;
  if (n === ancien) {
    if (parent) {
      const i = parent.enfants.indexOf(n);
      parent.enfants[i] = nouveau;
    } else {
      racineMosaique = nouveau;
    }
    return true;
  }
  if (n.type === "partage") {
    return remplacerNoeud(n.enfants[0], ancien, nouveau, n) ||
           remplacerNoeud(n.enfants[1], ancien, nouveau, n);
  }
  return false;
}

function mosaiqueRetirer(id) {
  const trouv = trouverFeuille(racineMosaique, id);
  if (!trouv) return;
  const { noeud, parent } = trouv;
  if (!parent) { racineMosaique = null; return; }

  const reste = parent.enfants[0] === noeud ? parent.enfants[1] : parent.enfants[0];
  // Le partage s'effondre sur sa sœur (§10.6).
  remplacerNoeud(racineMosaique, parent, reste, null);
}

function centreFenetre(f) {
  const r = f.el.getBoundingClientRect(), b = bureau.getBoundingClientRect();
  return { x: r.left - b.left + r.width / 2, y: r.top - b.top + r.height / 2 };
}

function voisineDirection(dir) {
  const courante = fenetres.get(idActif);
  if (!courante || courante.minimisee) return null;
  const c = centreFenetre(courante);
  let meilleur = null, meilleurScore = Infinity;

  for (const [id, f] of fenetres) {
    if (id === idActif || f.minimisee) continue;
    const o = centreFenetre(f);
    const dx = o.x - c.x, dy = o.y - c.y;
    let dansLaDirection = false, dist = 0;

    if (dir === "gauche" && dx < -1) { dansLaDirection = true; dist = Math.abs(dx) + Math.abs(dy) * 2; }
    if (dir === "droite" && dx > 1)  { dansLaDirection = true; dist = Math.abs(dx) + Math.abs(dy) * 2; }
    if (dir === "haut"   && dy < -1) { dansLaDirection = true; dist = Math.abs(dy) + Math.abs(dx) * 2; }
    if (dir === "bas"    && dy > 1)  { dansLaDirection = true; dist = Math.abs(dy) + Math.abs(dx) * 2; }

    if (dansLaDirection && dist < meilleurScore) { meilleurScore = dist; meilleur = id; }
  }
  return meilleur;
}

function focusDirection(dir) {
  const id = voisineDirection(dir);
  if (id !== null) focusFenetre(id);
}

function echangerDirection(dir) {
  if (!mosaiqueActivee) return;
  const cible = voisineDirection(dir);
  if (cible === null) return;
  const a = trouverFeuille(racineMosaique, idActif);
  const b = trouverFeuille(racineMosaique, cible);
  if (!a || !b) return;
  const tmp = a.noeud.id;
  a.noeud.id = b.noeud.id;
  b.noeud.id = tmp;
  appliquerMosaique();
  focusFenetre(cible);
}

function tournerPartage() {
  if (!mosaiqueActivee) return;
  const trouv = trouverFeuille(racineMosaique, idActif);
  if (!trouv) return;
  const partage = trouv.parent || racineMosaique;
  if (!partage || partage.type !== "partage") return;
  partage.or = partage.or === "h" ? "v" : "h";
  appliquerMosaique();
}

function rectRacineMosaique() {
  const m = MARGE_MOSAIQUE;
  return { x: m, y: m,
           w: bureau.clientWidth - 2 * m,
           h: bureau.clientHeight - 2 * m };
}

function calculerRects(n, r, out) {
  if (!n) return out;
  if (n.type === "feuille") { out.set(n.id, r); return out; }

  const [gauche, droite] = n.enfants;
  if (n.or === "h") {
    const coupe = Math.round(r.w * n.ratio);
    calculerRects(gauche, { x: r.x, y: r.y, w: coupe - ECART_MOSAIQUE / 2, h: r.h }, out);
    calculerRects(droite, { x: r.x + coupe + ECART_MOSAIQUE / 2, y: r.y,
                            w: r.w - coupe - ECART_MOSAIQUE / 2, h: r.h }, out);
  } else {
    const coupe = Math.round(r.h * n.ratio);
    calculerRects(gauche, { x: r.x, y: r.y, w: r.w, h: coupe - ECART_MOSAIQUE / 2 }, out);
    calculerRects(droite, { x: r.x, y: r.y + coupe + ECART_MOSAIQUE / 2,
                            w: r.w, h: r.h - coupe - ECART_MOSAIQUE / 2 }, out);
  }
  return out;
}

function appliquerMosaique() {
  if (!mosaiqueActivee) return;
  const rects = calculerRects(racineMosaique, rectRacineMosaique(), new Map());
  for (const [id, r] of rects) {
    const f = fenetres.get(id);
    if (!f) continue;
    Object.assign(f.el.style, {
      left: r.x + "px", top: r.y + "px",
      width: r.w + "px", height: r.h + "px",
    });
    f.el.classList.add("en-mosaique");
  }
}

/* Bascule globale : les fenêtres ouvertes entrent dans l'arbre, dans l'ordre
   de création. Éteindre les libère à l'endroit où elles sont. */
function basculerMosaique(force) {
  const suivant = typeof force === "boolean" ? force : !mosaiqueActivee;
  if (suivant === mosaiqueActivee) return;
  mosaiqueActivee = suivant;

  if (mosaiqueActivee) {
    racineMosaique = null;
    for (const [id, f] of fenetres) {
      if (f.flottante) continue;
      mosaiqueAjouter(id);
    }
    appliquerMosaique();
  } else {
    for (const [, f] of fenetres) f.el.classList.remove("en-mosaique");
    racineMosaique = null;
  }
  majIndicateurMosaique();
}

/* Flottant / mosaïque, fenêtre par fenêtre (§10.6) */
function basculerFlottante(id) {
  const f = fenetres.get(id);
  if (!f) return;
  f.flottante = !f.flottante;

  if (f.flottante) {
    f.el.classList.remove("en-mosaique");
    mosaiqueRetirer(id);
    appliquerMosaique();
  } else if (mosaiqueActivee) {
    mosaiqueAjouter(id);
    appliquerMosaique();
  }
  focusFenetre(id);
}

/* --- Barre des tâches (centre de la barre haute) -------------------------------- */

const barreTaches = $("#barre-taches");

function majBarreTaches() {
  barreTaches.textContent = "";
  for (const [id, f] of fenetres) {
    const b = document.createElement("button");
    b.className = "barre-tache";
    b.textContent = f.titre;
    if (id === idActif) b.classList.add("active");
    if (f.minimisee) b.classList.add("minimisee");
    b.addEventListener("click", () => {
      if (id === idActif && !f.minimisee) reduireFenetre(id);
      else focusFenetre(id);
    });
    barreTaches.appendChild(b);
  }
}

/* --- Horloge --------------------------------------------------------------------- */

const horloge = $("#horloge");

function majHorloge() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  horloge.textContent = `${p(d.getDate())}/${p(d.getMonth() + 1)} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
majHorloge();
setInterval(majHorloge, 10_000);

/* --- Thème et animations ----------------------------------------------------------- */

const boutonTheme = $("#bascule-theme");
const racineDoc = document.documentElement;

function majTheme() {
  const sombre = racineDoc.dataset.theme === "sombre";
  boutonTheme.textContent = sombre ? "clair" : "sombre";
  boutonTheme.title = `Thème ${sombre ? "clair" : "sombre"} (Ctrl+Alt+L)`;
}

function basculerTheme() {
  racineDoc.dataset.theme = racineDoc.dataset.theme === "sombre" ? "clair" : "sombre";
  majTheme();
  // La lisière suit le thème tant qu'elle n'est pas personnalisée : la valeur
  // affichée dans le menu doit refléter le primary nouveau.
  if (!lisierePersonnalisee) synchroniserLisiere();
}
boutonTheme.addEventListener("click", basculerTheme);
majTheme();

let animationsActives = !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const etatAnimations = $("#etat-animations");

function majAnimations() {
  document.body.classList.toggle("sans-animations", !animationsActives);
  etatAnimations.textContent = animationsActives ? "actives" : "désactivées";
}
majAnimations();

/* --- Menu XILI ---------------------------------------------------------------------- */

const menu = $("#menu");
const boutonMenu = $("#bouton-menu");

function ouvrirMenu(ouvert) {
  menu.hidden = !ouvert;
  boutonMenu.setAttribute("aria-expanded", String(ouvert));
  if (ouvert) synchroniserLisiere();
}

boutonMenu.addEventListener("click", (e) => {
  e.stopPropagation();
  ouvrirMenu(menu.hidden);
});

document.addEventListener("pointerdown", (e) => {
  if (!menu.hidden && !menu.contains(e.target) && e.target !== boutonMenu) ouvrirMenu(false);
});

menu.addEventListener("click", (e) => {
  const item = e.target.closest(".menu-item");
  if (!item) return;
  ouvrirMenu(false);
  switch (item.dataset.action) {
    case "terminal": ouvrirTerminal(); break;
    case "fichiers": ouvrirFichiers(); break;
    case "animations":
      animationsActives = !animationsActives;
      majAnimations();
      break;
    case "restart": reinitialiserBureau(); break;
    case "propos": ouvrirPropos(); break;
  }
});

/* --- Épingles du bureau (§10.3 : le bureau est vide et épinglable) ------------------ */

const GLYPHES = { dossier: "▸", texte: "≡", image: "◫", app: "›_" };

function ajouterEpingle(ep) {
  const el = creerElement(`
    <div class="epingle" tabindex="0" role="button">
      <div class="epingle-glyphe">${ep.glyphe || GLYPHES[ep.type] || "·"}</div>
      <span class="epingle-nom">${echapper(ep.nom)}</span>
    </div>`);
  el.style.left = ep.x + "px";
  el.style.top = ep.y + "px";
  el._donnee = ep;

  el.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll(".epingle.selectionnee").forEach((p) => p.classList.remove("selectionnee"));
    el.classList.add("selectionnee");
    el.focus();
  });

  el.addEventListener("dblclick", () => ouvrirEpingle(ep));

  el.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const startX = e.clientX, startY = e.clientY;
    const origL = el.offsetLeft, origT = el.offsetTop;
    let bouge = false;
    el.setPointerCapture(e.pointerId);

    const bouger = (ev) => {
      const dx = ev.clientX - startX, dy = ev.clientY - startY;
      if (!bouge && Math.hypot(dx, dy) < 4) return;
      bouge = true;
      el.style.left = Math.max(0, Math.min(origL + dx, zoneEpingles.clientWidth - 88)) + "px";
      el.style.top = Math.max(0, Math.min(origT + dy, zoneEpingles.clientHeight - 72)) + "px";
    };
    const relacher = () => {
      el.removeEventListener("pointermove", bouger);
      el.removeEventListener("pointerup", relacher);
    };
    el.addEventListener("pointermove", bouger);
    el.addEventListener("pointerup", relacher);
  });

  zoneEpingles.appendChild(el);
}

function ouvrirEpingle(ep) {
  if (ep.ouvrir) ep.ouvrir();
  else if (ep.type === "dossier") ouvrirFichiers(ep.chemin || "/");
  else ouvrirFichiers("/");
}

// Le clic vide du bureau désélectionne.
bureau.addEventListener("pointerdown", (e) => {
  if (e.target === bureau || e.target === zoneEpingles) {
    document.querySelectorAll(".epingle.selectionnee").forEach((p) => p.classList.remove("selectionnee"));
    if (idActif) { idActif = null; for (const g of fenetres.values()) g.el.classList.remove("active"); }
    majBarreTaches();
  }
});

/* --- Application : Terminal ----------------------------------------------------------- */

const COMMANDES = {
  aide: {
    aide: "aide — liste des commandes",
    handler() {
      return Object.keys(COMMANDES).sort((a, b) => a.localeCompare(b, "fr"))
        .map((c) => `  ${c.padEnd(12)} ${COMMANDES[c].aide}`).join("\n");
    },
  },
  ls: {
    aide: "ls — lister le répertoire courant",
    handler: () => '<span class="primaire">documents/</span>  <span class="primaire">images/</span>  <span class="primaire">systeme/</span>  notes.txt  journal.log',
  },
  date: { aide: "date — date et heure", handler: () => new Date().toLocaleString("fr-FR") },
  echo: { aide: "echo [texte] — répéter", handler: (args) => echapper(args.join(" ")) },
  effacer: { aide: "effacer — vider l'écran", handler: () => null },
  lisiere: {
    aide: "lisiere [couleur] — couleur de la lisière (ex. #FFC53D)",
    handler(args) {
      if (!args[0]) return `lisière courante : ${getComputedStyle(document.documentElement).getPropertyValue("--lisiere").trim()}`;
      if (!/^#[0-9a-fA-F]{6}$/.test(args[0])) return "format attendu : #RRGGBB";
      appliquerLisiere(args[0]);
      return `lisière : ${args[0]}`;
    },
  },
  animations: {
    aide: "animations [on|off] — mouvement du système",
    handler(args) {
      if (args[0] === "on") animationsActives = true;
      else if (args[0] === "off") animationsActives = false;
      else return "usage : animations on | off";
      majAnimations();
      return `animations ${animationsActives ? "actives" : "désactivées"}`;
    },
  },
  sombre: { aide: "sombre — thème sombre", handler: () => { racineDoc.dataset.theme = "sombre"; majTheme(); return "thème sombre appliqué"; } },
  clair: { aide: "clair — thème clair", handler: () => { racineDoc.dataset.theme = "clair"; majTheme(); return "thème clair appliqué"; } },
  epingler: {
    aide: "epingler [nom] — épingler un raccourci sur le bureau",
    handler(args) {
      const nom = args.join(" ") || "raccourci";
      ajouterEpingle({ nom, type: "texte", x: 24, y: 24 + 88 * document.querySelectorAll(".epingle").length % 400 });
      return `épinglé : ${nom}`;
    },
  },
  propos: {
    aide: "propos — à propos de XILI",
    handler: () => 'XILI — <span class="accent">un système calme, lisible, qui ne demande rien</span>\n<span class="dim">maquette HTML/CSS/JS · docs/DESIGN.md · cible compositeur J7</span>',
  },
  fermer: {
    aide: "fermer — fermer ce terminal",
    handler() { setTimeout(() => fermerFenetre(idTerminal), 0); return "fermeture…"; },
  },
};

let idTerminal = null;

function ouvrirTerminal() {
  const el = creerElement(`
    <div class="terminal">
      <div class="terminal-sortie"><span class="dim">XILI — tapez « aide » pour les commandes.</span></div>
      <div class="terminal-saisie-ligne">
        <span class="terminal-prompt">xili&nbsp;›</span>
        <input class="terminal-saisie" autocomplete="off" spellcheck="false" aria-label="Saisie du terminal">
      </div>
    </div>`);

  const sortie = $(".terminal-sortie", el);
  const saisie = $(".terminal-saisie", el);
  const historique = [];
  let indice = 0;

  const ecrire = (ligne, classe = "") => {
    const div = document.createElement("div");
    if (classe) div.className = classe;
    div.innerHTML = ligne || "&nbsp;";
    sortie.appendChild(div);
    sortie.scrollTop = sortie.scrollHeight;
  };

  saisie.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      const brut = saisie.value.trim();
      ecrire(`<span class="primaire">xili ›</span> ${echapper(brut)}`, "dim");
      saisie.value = "";
      if (brut) { historique.push(brut); indice = historique.length; }
      const [nom, ...args] = brut.split(/\s+/);
      const cmd = nom && COMMANDES[nom.toLowerCase()];
      if (nom && !cmd) ecrire(`commande inconnue : ${echapper(nom)} — « aide » pour la liste`, "dim");
      else if (cmd) { const r = cmd.handler(args); if (r !== null) ecrire(r); }
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (indice > 0) { indice--; saisie.value = historique[indice]; }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (indice < historique.length - 1) { indice++; saisie.value = historique[indice]; }
      else { indice = historique.length; saisie.value = ""; }
    }
  });

  el.addEventListener("click", () => saisie.focus());
  idTerminal = ouvrirFenetre("Terminal", el, { w: 620, h: 400 });
}

/* --- Application : Fichiers ------------------------------------------------------------ */

const ARBRE = {
  "/": [
    { nom: "documents", type: "dossier", taille: "—" },
    { nom: "images", type: "dossier", taille: "—" },
    { nom: "systeme", type: "dossier", taille: "—" },
  ],
  "/documents": [
    { nom: "design.md", type: "texte", taille: "8 Ko" },
    { nom: "roadmap.md", type: "texte", taille: "6 Ko" },
    { nom: "journal.md", type: "texte", taille: "12 Ko" },
  ],
  "/images": [
    { nom: "embleme-256.png", type: "image", taille: "2 Ko" },
    { nom: "logo-xili-sombre.png", type: "image", taille: "7 Ko" },
  ],
  "/systeme": [
    { nom: "noyau", type: "dossier", taille: "—" },
    { nom: "compositeur", type: "dossier", taille: "—" },
    { nom: "xili.conf", type: "texte", taille: "1 Ko" },
  ],
  "/systeme/noyau": [],
  "/systeme/compositeur": [],
};

const LIEUX = [["/", "Accueil"], ["/documents", "Documents"], ["/images", "Images"], ["/systeme", "Système"]];

function ouvrirFichiers(depart) {
  const el = creerElement(`
    <div class="fichiers">
      <nav class="fichiers-lieux" aria-label="Lieux"></nav>
      <div class="fichiers-liste"></div>
    </div>`);

  const lieux = $(".fichiers-lieux", el);
  const liste = $(".fichiers-liste", el);
  let courant = depart && ARBRE[depart] ? depart : "/documents";

  function dessiner() {
    lieux.textContent = "";
    for (const [chemin, nom] of LIEUX) {
      const b = document.createElement("button");
      b.className = "fichiers-lieu" + (chemin === courant ? " actif" : "");
      b.textContent = nom;
      b.addEventListener("click", () => { courant = chemin; dessiner(); });
      lieux.appendChild(b);
    }

    liste.textContent = "";
    liste.appendChild(creerElement(`<div class="fichiers-chemin">${echapper(courant)}</div>`));
    liste.appendChild(creerElement(`<div class="fichiers-entete"><span>Nom</span><span style="text-align:right">Taille</span><span style="text-align:right">Type</span></div>`));

    const contenu = ARBRE[courant] || [];
    if (!contenu.length) {
      liste.appendChild(creerElement(`<div class="fichiers-aide">Dossier vide.<br>Glissez un fichier vers le bureau pour l'épingler.</div>`));
    }

    for (const e of contenu) {
      const estDossier = e.type === "dossier";
      const rangee = creerElement(`
        <div class="fichiers-rangee ${estDossier ? "dossier" : "fichier"}" tabindex="0" draggable="true">
          <span class="col-nom">${echapper(e.nom)}</span>
          <span class="col-taille">${e.taille}</span>
          <span class="col-type">${e.type}</span>
        </div>`);

      if (estDossier) {
        rangee.addEventListener("dblclick", () => {
          const sous = (courant === "/" ? "" : courant) + "/" + e.nom;
          ARBRE[sous] = ARBRE[sous] || [];
          courant = sous;
          dessiner();
        });
      }

      // Glisser une ligne vers le bureau → épingle.
      rangee.addEventListener("dragstart", (ev) => {
        ev.dataTransfer.setData("text/plain", JSON.stringify({ nom: e.nom, type: e.type }));
      });

      liste.appendChild(rangee);
    }
  }

  dessiner();
  ouvrirFenetre("Fichiers", el, { w: 620, h: 400 });
}

// Déposer sur le bureau → épingle (§10.3 : tout se pose sur le bureau).
bureau.addEventListener("dragover", (e) => e.preventDefault());
bureau.addEventListener("drop", (e) => {
  e.preventDefault();
  try {
    const d = JSON.parse(e.dataTransfer.getData("text/plain"));
    if (!d.nom) return;
    const rect = zoneEpingles.getBoundingClientRect();
    ajouterEpingle({
      nom: d.nom,
      type: d.type || "texte",
      x: Math.max(0, e.clientX - rect.left - 44),
      y: Math.max(0, e.clientY - rect.top - 36),
    });
  } catch { /* hors maquette : on ignore */ }
});

/* --- Application : À propos -------------------------------------------------------------- */

function ouvrirPropos() {
  const el = creerElement(`
    <div class="propos">
      <h1>XILI</h1>
      <p class="citation">Un système calme, lisible, qui ne demande rien — et qui ne déçoit pas.</p>
      <p>Maquette du compositeur, bâtie hors noyau pour faire vivre les jetons de
      <code>docs/DESIGN.md</code>. Elle sera portée telle quelle dans le compositeur
      en J7 : le design est validé, on porte, on ne redessine pas.</p>
      <p class="dim">Le bureau est vide : glissez-y des fichiers depuis Fichiers,
      double-cliquez pour ouvrir. La lisière de la fenêtre active se règle dans
      le menu XILI — c'est la seule couleur libre du système.</p>
    </div>`);
  ouvrirFenetre("À propos", el, { w: 460, h: 340 });
}

/* --- Réinitialisation -------------------------------------------------------------------- */

function reinitialiserBureau() {
  for (const id of [...fenetres.keys()]) fermerFenetre(id);
  zoneEpingles.textContent = "";
  demarrer();
}

/* --- Raccourcis clavier -------------------------------------------------------------------- */

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !menu.hidden) { ouvrirMenu(false); return; }
  if (!e.ctrlKey || !e.altKey) return;
  const t = e.key.toLowerCase();
  if (t === "t") { e.preventDefault(); ouvrirTerminal(); }
  else if (t === "f") { e.preventDefault(); ouvrirFichiers(); }
  else if (t === "l") { e.preventDefault(); basculerTheme(); }
});

/* --- Démarrage ------------------------------------------------------------------------------ */

function demarrer() {
  ajouterEpingle({ nom: "Système", type: "dossier", x: 24, y: 24, chemin: "/systeme" });
  const t = ouvrirTerminal();
  const f = ouvrirFichiers();

  // Côte à côte quand la largeur le permet (cible 1280×800).
  const largeur = bureau.clientWidth;
  if (largeur >= 960) {
    const w = Math.floor((largeur - 48) / 2);
    const hauteur = bureau.clientHeight - 64;
    const placer = (id, x) => {
      const fen = fenetres.get(id);
      if (!fen) return;
      Object.assign(fen.el.style, { left: x + "px", top: "24px", width: w + "px", height: hauteur + "px" });
    };
    placer(t, 16);
    placer(f, 32 + w);
  }
}

demarrer();
