"""Draws the Windows installer's pictures from the Ropeway mark.

    python scripts/make-installer-art.py

Writes src-tauri/installer/sidebar.bmp (164x314, welcome and finish pages)
and header.bmp (150x57, the other pages). Drawn at 4x and scaled down.
"""

import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "src-tauri", "installer")
NAVY = (11, 26, 46)
NAVY_2 = (18, 52, 74)
TEAL = (108, 200, 181)
LIGHT = (241, 245, 249)
S = 4


def font(name: str, size: int):
    for path in (os.path.join(os.environ.get("WINDIR", "C:\\Windows"), "Fonts", name), name):
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            continue
    return ImageFont.load_default()


def mark(draw: ImageDraw.ImageDraw, cx: float, cy: float, size: float, line=LIGHT, top=TEAL, width_k=0.075):
    """Three stations joined by a cable, as in the app icon."""
    r = size * 0.095
    a = (cx, cy - size * 0.30)
    b = (cx - size * 0.39, cy + size * 0.22)
    c = (cx + size * 0.39, cy + size * 0.22)
    w = int(size * width_k)
    draw.line([b, a, c, b], fill=line, width=w, joint="curve")
    for p, colour in ((b, line), (c, line), (a, top)):
        draw.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=colour)


def sidebar():
    w, h = 164 * S, 314 * S
    img = Image.new("RGB", (w, h), NAVY)
    # A soft diagonal light, like the app's glass themes.
    glow = Image.new("L", (w, h), 0)
    gd = ImageDraw.Draw(glow)
    gd.ellipse([-w * 0.6, -h * 0.25, w * 1.1, h * 0.55], fill=150)
    glow = glow.filter(ImageFilter.GaussianBlur(w * 0.25))
    img = Image.composite(Image.new("RGB", (w, h), NAVY_2), img, glow)
    d = ImageDraw.Draw(img)
    # The cable of a ropeway crossing the picture, with two cabins.
    d.line([(-10, h * 0.80), (w + 10, h * 0.66)], fill=(46, 84, 110), width=2 * S)
    for x in (0.28, 0.72):
        px, py = w * x, h * 0.80 + (h * 0.66 - h * 0.80) * x
        d.line([(px, py), (px, py + 14 * S)], fill=(46, 84, 110), width=2 * S)
        d.rounded_rectangle([px - 9 * S, py + 14 * S, px + 9 * S, py + 28 * S], radius=3 * S, fill=(30, 64, 90))
    mark(d, w / 2, h * 0.33, w * 0.62)
    title = font("seguisb.ttf", 22 * S)
    tw = d.textlength("Ropeway", font=title)
    d.text(((w - tw) / 2, h * 0.50), "Ropeway", font=title, fill=LIGHT)
    d.rounded_rectangle([w / 2 - 14 * S, h * 0.50 + 34 * S, w / 2 + 14 * S, h * 0.50 + 36 * S], radius=S, fill=TEAL)
    return img.resize((164, 314), Image.LANCZOS)


def header():
    w, h = 150 * S, 57 * S
    img = Image.new("RGB", (w, h), (255, 255, 255))
    d = ImageDraw.Draw(img)
    size = 42 * S
    x0, y0 = w - size - 8 * S, (h - size) / 2
    d.rounded_rectangle([x0, y0, x0 + size, y0 + size], radius=size * 0.24, fill=NAVY)
    mark(d, x0 + size / 2, y0 + size / 2 + size * 0.03, size * 0.82)
    return img.resize((150, 57), Image.LANCZOS)


def main():
    os.makedirs(OUT, exist_ok=True)
    sidebar().save(os.path.join(OUT, "sidebar.bmp"))
    header().save(os.path.join(OUT, "header.bmp"))
    print("ok")


if __name__ == "__main__":
    main()
