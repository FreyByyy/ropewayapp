"""Draws the Ropeway icon (user_revisions/icons/roapway-*.svg) as PNGs.

Run from the project root:  python scripts/make-icons.py
Writes icon-source.png (the app icon, 1024 px) for `npx tauri icon`, and the
Android adaptive icon layers into src-tauri/gen/android.
"""

import os

from PIL import Image, ImageDraw

NAVY = (11, 26, 46, 255)
TEAL = (108, 200, 181, 255)
WHITE = (244, 247, 250, 255)

ROOT = os.path.join(os.path.dirname(__file__), "..")
RES = os.path.join(ROOT, "src-tauri", "gen", "android", "app", "src", "main", "res")
SS = 4  # supersampling


def mark(draw: ImageDraw.ImageDraw, size: float, ox: float, oy: float, scale: float, stroke: float, r: float, fg, top):
    """The mark in a 100x100 box: a triangle of cable and three stations."""
    p = lambda x, y: (ox + x * scale * size / 100, oy + y * scale * size / 100)
    pts = [p(50, 22), p(22, 74), p(78, 74)]
    w = stroke * scale * size / 100
    draw.line(pts + [pts[0], pts[1]], fill=fg, width=round(w), joint="curve")
    rr = r * scale * size / 100
    for (x, y), color in zip(pts, [top, fg, fg]):
        draw.ellipse([x - rr, y - rr, x + rr, y + rr], fill=color)


def app_icon(px: int, rounded: bool = True, circle: bool = False) -> Image.Image:
    s = px * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if circle:
        d.ellipse([0, 0, s - 1, s - 1], fill=NAVY)
    elif rounded:
        d.rounded_rectangle([0, 0, s - 1, s - 1], radius=round(s * 0.24), fill=NAVY)
    else:
        d.rectangle([0, 0, s, s], fill=NAVY)
    # favicon.svg: translate(15 15) scale(0.7), stroke 9, stations r 13.
    mark(d, s, s * 0.15, s * 0.15, 0.7, 9, 13, WHITE, TEAL)
    return img.resize((px, px), Image.LANCZOS)


def adaptive_foreground(px: int) -> Image.Image:
    """108dp layer; the mark stays inside the 66dp safe zone."""
    s = px * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    inner = 0.6
    off = s * (1 - inner) / 2
    mark(d, s, off, off, inner, 9, 13, WHITE, TEAL)
    return img.resize((px, px), Image.LANCZOS)


def main():
    app_icon(1024).save(os.path.join(ROOT, "icon-source.png"))
    densities = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
    for name, k in densities.items():
        folder = os.path.join(RES, f"mipmap-{name}")
        os.makedirs(folder, exist_ok=True)
        app_icon(round(48 * k)).save(os.path.join(folder, "ic_launcher.png"))
        app_icon(round(48 * k), circle=True).save(os.path.join(folder, "ic_launcher_round.png"))
        adaptive_foreground(round(108 * k)).save(os.path.join(folder, "ic_launcher_foreground.png"))
    print("ok")


if __name__ == "__main__":
    main()
