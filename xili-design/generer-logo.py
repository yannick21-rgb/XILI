#!/usr/bin/env python3
"""Génère les déclinaisons du logo XILI.

Règle de licence : Lyl Bah Demo est une police « Free For Personal Use ».
Elle ne sert qu'au LOGOTYPE (un fichier image), jamais à l'interface ni à un
texte courant, et jamais pour un texte contenant des chiffres ou de la
ponctuation — la version demo porte un filigrane dans le glyphe « 0 » et les
signes de ponctuation manquants s'affichent en bloc plein.
"""

import os
from PIL import Image, ImageDraw, ImageFont

FONT = "Lyl-Bah-Demo.ttf"
SORTIE = "/home/jhpy/XILI/assets/logo"
os.makedirs(SORTIE, exist_ok=True)

SOMBRE = (22, 24, 29)      # --bg
TEXT = (230, 231, 234)     # --text
ACCENT = (201, 166, 127)   # --accent
CLAIR_FOND = (246, 246, 244)
CLAIR_TEXTE = (27, 29, 34)


def mesurer(texte, taille):
    f = ImageFont.truetype(FONT, taille)
    boite = f.getbbox(texte)
    return f, boite[2] - boite[0], boite[3] - boite[1]


def rendre(texte, taille, couleur, fond, marge, nom, accent_x=False):
    """Dessine le mot et enregistre un PNG sur fond uni."""
    f, largeur, hauteur = mesurer(texte, taille)
    largeur_img = largeur + marge * 2
    hauteur_img = hauteur + marge * 2
    img = Image.new("RGB", (largeur_img, hauteur_img), fond)
    d = ImageDraw.Draw(img)
    if accent_x and len(texte) > 1:
        # la première lettre en accent, le reste en texte
        d.text((marge, marge), texte[0], font=f, fill=ACCENT)
        f, l, h = mesurer(texte[0], taille)
        d.text((marge + l, marge), texte[1:], font=f, fill=couleur)
    else:
        d.text((marge, marge), texte, font=f, fill=couleur)
    chemin = os.path.join(SORTIE, nom)
    img.save(chemin)
    print(f"{nom:28} {largeur_img}x{hauteur_img}  {os.path.getsize(chemin)} o")
    return chemin


def rendre_transparent(texte, taille, couleur, marge, nom):
    """Même logo, mais sans fond : utile pour superposer sur un fond inconnu."""
    f, largeur, hauteur = mesurer(texte, taille)
    img = Image.new("RGBA", (largeur + marge * 2, hauteur + marge * 2), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.text((marge, marge), texte, font=f, fill=couleur + (255,))
    chemin = os.path.join(SORTIE, nom)
    img.save(chemin)
    print(f"{nom:28} {img.width}x{img.height}  {os.path.getsize(chemin)} o")
    return chemin


print("== logo principal : thème sombre ==")
rendre("XILI", 200, TEXT, SOMBRE, 40, "logo-sombre.png")
rendre("XILI", 200, TEXT, SOMBRE, 40, "logo-sombre-accent.png", accent_x=True)

print("== logo principal : thème clair ==")
rendre("XILI", 200, CLAIR_TEXTE, CLAIR_FOND, 40, "logo-clair.png")
rendre("XILI", 200, CLAIR_TEXTE, CLAIR_FOND, 40, "logo-clair-accent.png", accent_x=True)

print("== déclinaisons ==")
rendre("XILI", 96, TEXT, SOMBRE, 24, "logo-96.png")
rendre("XILI", 96, TEXT, SOMBRE, 24, "logo-96-accent.png", accent_x=True)

print("== favicon (carré, fond rempli) ==")
rendre("XILI", 96, TEXT, SOMBRE, 0, "favicon-64.png")
rendre("XILI", 96, TEXT, SOMBRE, 0, "favicon-64-accent.png", accent_x=True)

print("== sans fond ==")
rendre_transparent("XILI", 200, TEXT, 40, "logo-transparent.png")

print("\nDossier :", SORTIE)
for f in sorted(os.listdir(SORTIE)):
    print("  ", f)
