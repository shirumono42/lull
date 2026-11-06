"""Generates Lull's app icons: assets/icon-1024.png, lull.icns (macOS), lull.ico (Windows).

macOS: Apple's icon grid -- 1024 canvas, 824 x 824 body (100 px margin) with
continuous ("squircle") corners and a soft drop shadow.
Windows: the same artwork filling the whole canvas, no shadow.

Run:  python assets/make_icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).parent
SS = 4  # supersampling for smooth edges

BG_TOP, BG_BOTTOM = (40, 45, 62), (17, 19, 26)
# Same colours as the panel: white, pink, brown noise and binaural blue.
BARS = [((226, 230, 240), 0.42), ((255, 121, 176), 0.70), ((214, 146, 92), 0.55), ((122, 162, 255), 0.34)]


def squircle_mask(size: int, radius: float = 185.4 / 824) -> Image.Image:
    """Rounded square with continuous corners, as in Apple's icon grid.

    `radius` is the nominal corner radius (Apple: 185.4 on an 824 body). A
    continuous corner starts bending about 1.528 x radius from the corner and
    follows a superellipse there, instead of a circular arc.
    """
    big = size * SS
    m = Image.new("L", (big, big), 0)
    px = m.load()
    e = radius * 1.528 * big  # extent of the curved part
    n = 3.6  # superellipse exponent that matches the continuous curve
    for y in range(big):
        fy = y + 0.5
        dy = max(e - fy, fy - (big - e), 0.0)  # distance into a corner band
        if dy == 0:
            for x in range(big):
                px[x, y] = 255
            continue
        t = (dy / e) ** n
        half = (1 - t) ** (1 / n) * e if t < 1 else 0.0
        x0, x1 = int(round(e - half)), int(round(big - e + half))
        for x in range(max(x0, 0), min(x1, big)):
            px[x, y] = 255
    return m.resize((size, size), Image.LANCZOS)


def artwork(size: int) -> Image.Image:
    """Square artwork (gradient + bars), not yet masked."""
    big = size * SS
    img = Image.new("RGBA", (big, big))
    d = ImageDraw.Draw(img)
    for y in range(big):  # vertical gradient
        t = y / (big - 1)
        c = tuple(int(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM))
        d.line([(0, y), (big, y)], fill=c + (255,))
    # Soft light from the top, like a glass sheen.
    sheen = Image.new("L", (big, big), 0)
    ImageDraw.Draw(sheen).ellipse([-big * 0.4, -big * 0.9, big * 1.4, big * 0.35], fill=26)
    sheen = sheen.filter(ImageFilter.GaussianBlur(big * 0.14))
    img = Image.composite(Image.new("RGBA", (big, big), (255, 255, 255, 255)), img, sheen)

    d = ImageDraw.Draw(img)
    bar_w, gap = big * 0.105, big * 0.065
    total = len(BARS) * bar_w + (len(BARS) - 1) * gap
    x = (big - total) / 2
    cy = big * 0.52
    for colour, h in BARS:
        hh = big * h
        glow = Image.new("RGBA", (big, big), (0, 0, 0, 0))
        ImageDraw.Draw(glow).rounded_rectangle(
            [x, cy - hh / 2, x + bar_w, cy + hh / 2], radius=bar_w / 2, fill=colour + (120,))
        img = Image.alpha_composite(img, glow.filter(ImageFilter.GaussianBlur(big * 0.025)))
        d = ImageDraw.Draw(img)
        d.rounded_rectangle([x, cy - hh / 2, x + bar_w, cy + hh / 2], radius=bar_w / 2, fill=colour + (255,))
        x += bar_w + gap
    return img.resize((size, size), Image.LANCZOS)


def masked(size: int) -> Image.Image:
    art = artwork(size)
    art.putalpha(squircle_mask(size))
    return art


def mac_icon() -> Image.Image:
    canvas, body = 1024, 824
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    off = (canvas - body) // 2
    shadow = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    sm = squircle_mask(body).point(lambda a: a * 0.45)
    shadow.paste((0, 0, 0, 255), (off, off + 12), sm)
    out = Image.alpha_composite(out, shadow.filter(ImageFilter.GaussianBlur(14)))
    icon = masked(body)
    out.alpha_composite(icon, (off, off))
    return out


def main() -> None:
    mac = mac_icon()
    mac.save(HERE / "icon-1024.png")
    mac.save(HERE / "lull.icns")
    win = masked(256)
    win.save(HERE / "lull.ico", sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("wrote icon-1024.png, lull.icns, lull.ico")


if __name__ == "__main__":
    main()
