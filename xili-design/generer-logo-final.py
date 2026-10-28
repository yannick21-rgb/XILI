#!/usr/bin/env python3
"""Logo final XILI — forme (carré légèrement arrondi + X) + nom.

Forme choisie : un carré aux angles légèrement arrondis contenant un X,
posé à gauche du mot XILI (police Lyl Bah Demo, logotype uniquement).

Règle de licence (Lyl Bah Demo, « Free For Personal Use ») :
  - utilisable UNIQUEMENT pour le logotype (image), jamais dans
    l'interface ni pour du texte courant ;
  - jamais de chiffres ni de ponctuation (glyphe « 0 » filigrané).
"""

import os
from PIL import Image, ImageDraw, ImageFont

# ---------------------------------------------------------------- chemins
SORTIE = "/home/jhpy/XILI/assets/logo"
os.makedirs(SORTIE, exist_ok=True)
FONT = "/home/jhpy/xili-design/Lyl-Bah-Demo.ttf"

# ---------------------------------------------------------------- couleurs
SOMBRE_FOND = (22, 24, 29)        # --bg
SOMBRE_TEXTE = (230, 231, 234)    # --text
SOMBRE_FORME = (127, 166, 201)    # --primary
SOMBRE_ACCENT = (201, 166, 127)   # --accent

CLAIR_FOND = (246, 246, 244)
CLAIR_TEXTE = (27, 29, 34)
CLAIR_FORME = (72, 118, 158)
CLAIR_ACCENT = (176, 133, 84)

# ------------------------------------------------------- dessin de la forme


def dessiner_forme(d, x, y, taille, couleur, couleur_x, epaisseur=8):
    """Carré légèrement arrondi avec un X à l'intérieur.

    (x, y) = coin haut-gauche, taille = côté du carré.
    """
    rayon = taille // 8  # arrondi léger : 1/8 du côté
    d.rounded_rectangle(
        [x, y, x + taille, y + taille],
        radius=rayon,
        outline=couleur,
        width=epaisseur,
    )

    # X à l'intérieur, avec une marge pour qu'il "respire"
    marge = taille * 0.26
    a = (x + marge, y + marge)
    b = (x + taille - marge, y + taille - marge)
    c = (x + taille - marge, y + marge)
    e = (x + marge, y + taille - marge)
    epaisseur_x = max(6, taille // 12)
    d.line([a, b], fill=couleur_x, width=epaisseur_x)
    d.line([c, e], fill=couleur_x, width=epaisseur_x)


def dessiner_logo(couleur_forme, couleur_x, fond, texte, taille=140,
                  nom_police=FONT, marge=60, taille_forme=None):
    """Rendu complet : forme + mot XILI. Retourne une image RGBA."""
    if taille_forme is None:
        taille_forme = int(taille * 1.05)

    police = ImageFont.truetype(nom_police, taille)
    boite = police.getbbox("XILI")
    largeur_texte = boite[2] - boite[0]
    hauteur_texte = boite[3] - boite[1]

    ecart = int(taille_forme * 0.38)
    largeur = marge + taille_forme + ecart + largeur_texte + marge
    hauteur = max(taille_forme, hauteur_texte) + marge * 2

    mode = "RGB" if fond is not None else "RGBA"
    img = Image.new(mode, (largeur, hauteur),
                    fond if fond is not None else (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # forme, centrée verticalement
    fy = (hauteur - taille_forme) // 2
    dessiner_forme(d, marge, fy, taille_forme, couleur_forme, couleur_x)

    # mot, centré verticalement et aligné à gauche après la forme
    tx = marge + taille_forme + ecart
    ty = (hauteur - hauteur_texte) // 2 - boite[1]
    d.text((tx, ty), "XILI", font=police, fill=texte)

    return img


def dessiner_embleme(couleur_forme, couleur_x, fond, taille=256, epaisseur=None):
    """Emblème seul (favicon / icône) : carré arrondi + X, plein cadre."""
    if epaisseur is None:
        epaisseur = max(6, taille // 32)
    mode = "RGB" if fond is not None else "RGBA"
    img = Image.new(mode, (taille, taille),
                    fond if fond is not None else (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    dessiner_forme(d, 0, 0, taille, couleur_forme, couleur_x, epaisseur)
    return img


# ---------------------------------------------------------------- génération
print("Génération du logo final XILI (carré arrondi + X + nom)")
print("=" * 56)

entrees = [
    # nom, image
    ("logo-xili-sombre.png",
     dessiner_logo(SOMBRE_FORME, SOMBRE_ACCENT, SOMBRE_FOND, SOMBRE_TEXTE)),
    ("logo-xili-clair.png",
     dessiner_logo(CLAIR_FORME, CLAIR_ACCENT, CLAIR_FOND, CLAIR_TEXTE)),
    ("logo-xili-transparent.png",
     dessiner_logo(SOMBRE_FORME, SOMBRE_ACCENT, None, SOMBRE_TEXTE)),
    ("logo-xili-transparent-sombre-sur-clair.png",
     dessiner_logo(CLAIR_FORME, CLAIR_ACCENT, None, CLAIR_TEXTE)),
    # emblèmes seuls
    ("embleme-256-sombre.png",
     dessiner_embleme(SOMBRE_FORME, SOMBRE_ACCENT, SOMBRE_FOND)),
    ("embleme-256-transparent.png",
     dessiner_embleme(SOMBRE_FORME, SOMBRE_ACCENT, None)),
    ("embleme-128.png",
     dessiner_embleme(SOMBRE_FORME, SOMBRE_ACCENT, SOMBRE_FOND, 128)),
    ("embleme-64.png",
     dessiner_embleme(SOMBRE_FORME, SOMBRE_ACCENT, SOMBRE_FOND, 64)),
    ("favicon-32.png",
     dessiner_embleme(SOMBRE_FORME, SOMBRE_ACCENT, SOMBRE_FOND, 32,
                      epaisseur=3)),
]

for nom, img in entrees:
    img.save(os.path.join(SORTIE, nom))
    print(f"✓ {nom:44} {img.width}x{img.height}")

print("=" * 56)
print(f"✓ {len(entrees)} fichiers écrits dans {SORTIE}")
