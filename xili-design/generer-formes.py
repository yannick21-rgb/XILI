#!/usr/bin/env python3
"""Génère des propositions de logos XILI avec une forme + nom."""
import os
from PIL import Image, ImageDraw, ImageFont

SORTIE = "/home/jhpy/xili-design/logos-propositions"
os.makedirs(SORTIE, exist_ok=True)

# Thème sombre (couleurs du projet XILI)
FOND = (22, 24, 29)
TEXTE = (230, 231, 234)
ACCENT = (127, 166, 201)
GRIS = (139, 143, 153)
CLAIR_FOND = (246, 246, 244)
CLAIR_TEXTE = (27, 29, 34)
CLAIR_ACCENT = (90, 130, 170)

# Police (on utilise une police système standard car Lyl Bah Demo a des limitations)
try:
    POLICE = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
except:
    POLICE = "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"

def créer_fond(hauteur, fond):
    """Crée une image vierge."""
    largeur = 600
    img = Image.new("RGB", (largeur, hauteur), fond)
    return img, ImageDraw.Draw(img)

def dessiner_forme_1_cercle(d, cx, cy, r, couleur, epaisseur=4):
    """Cercle : le noyau."""
    d.ellipse([cx-r, cy-r, cx+r, cy+r], outline=couleur, width=epaisseur)

def dessiner_forme_2_carré(d, x, y, taille, couleur, epaisseur=4):
    """Carré : structure."""
    d.rectangle([x, y, x+taille, y+taille], outline=couleur, width=epaisseur)

def dessiner_forme_3_hexagone(d, cx, cy, r, couleur, epaisseur=4):
    """Hexagone : modularité."""
    import math
    points = []
    for i in range(6):
        angle = math.pi / 3 * i
        x = cx + r * math.cos(angle)
        y = cy + r * math.sin(angle)
        points.append((x, y))
    points.append(points[0])  # fermer
    d.line(points, fill=couleur, width=epaisseur, joint="curve")

def dessiner_forme_4_x_stylisé(d, cx, cy, taille, couleur, epaisseur=6):
    """X : signature."""
    moitié = taille // 2
    d.line([cx-moitié, cy-moitié, cx+moitié, cy+moitié], fill=couleur, width=epaisseur)
    d.line([cx+moitié, cy-moitié, cx-moitié, cy+moitié], fill=couleur, width=epaisseur)

def dessiner_forme_5_cercle_point(d, cx, cy, r, couleur, epaisseur=4):
    """Cercle + point central : système vivant."""
    d.ellipse([cx-r, cy-r, cx+r, cy+r], outline=couleur, width=epaisseur)
    d.ellipse([cx-8, cy-8, cx+8, cy+8], fill=couleur)

def dessiner_forme_6_fenêtre(d, x, y, l, h, couleur, epaisseur=4):
    """Fenêtre arrondie : interface."""
    d.rounded_rectangle([x, y, x+l, y+h], radius=12, outline=couleur, width=epaisseur)
    # barre supérieure
    d.rounded_rectangle([x, y, x+l, y+24], radius=[12, 12, 0, 0], fill=couleur)

def dessiner_nom(d, x, y, taille, couleur):
    """Dessine 'XILI'."""
    f = ImageFont.truetype(POLICE, taille)
    d.text((x, y), "XILI", font=f, fill=couleur)

def créer_logo(fond, texte, accent, nom, dessiner_forme):
    """Génère un logo avec une forme et le nom."""
    img, d = créer_fond(200, fond)

    # Dessiner la forme à gauche
    dessiner_forme(d, 80, 100, accent)

    # Dessiner le nom à droite
    dessiner_nom(d, 180, 60, 80, texte)

    chemin = os.path.join(SORTIE, nom)
    img.save(chemin)
    print(f"✓ {nom:40} {img.width}x{img.height}")
    return chemin

print("Génération des propositions de logos XILI...")
print("=" * 50)

# Définitions des formes (chacune reçoit d, cx, cy, couleur)
formes = [
    ("1-cercle", lambda d, cx, cy, c: dessiner_forme_1_cercle(d, cx, cy, 40, c)),
    ("2-carré", lambda d, cx, cy, c: dessiner_forme_2_carré(d, cx-40, cy-40, 80, c)),
    ("3-hexagone", lambda d, cx, cy, c: dessiner_forme_3_hexagone(d, cx, cy, 45, c)),
    ("4-x-stylisé", lambda d, cx, cy, c: dessiner_forme_4_x_stylisé(d, cx, cy, 70, c)),
    ("5-cercle-point", lambda d, cx, cy, c: dessiner_forme_5_cercle_point(d, cx, cy, 40, c)),
    ("6-fenêtre", lambda d, cx, cy, c: dessiner_forme_6_fenêtre(d, cx-40, cy-30, 80, 60, c)),
]

# Générer chaque forme en sombre et clair
for nom_forme, dessiner in formes:
    # Thème sombre
    créer_logo(FOND, TEXTE, ACCENT, f"logo-{nom_forme}-sombre.png", dessiner)
    # Thème clair
    créer_logo(CLAIR_FOND, CLAIR_TEXTE, CLAIR_ACCENT, f"logo-{nom_forme}-clair.png", dessiner)

print("\n" + "=" * 50)
print(f"✓ {len(formes)*2} logos générés dans : {SORTIE}")
print("Ouvrez le dossier pour choisir votre préférence.")
