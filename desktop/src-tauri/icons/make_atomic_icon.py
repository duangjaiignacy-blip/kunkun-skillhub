from __future__ import annotations

import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parent
ICONSET = ROOT / "kunkun.iconset"


def rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=255)
    return mask


def gradient_base(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pix = img.load()
    center = size / 2
    for y in range(size):
        for x in range(size):
            dx = (x - center) / center
            dy = (y - center) / center
            d = min(1.0, math.hypot(dx, dy))
            vignette = 1 - d
            r = int(10 + 18 * vignette + 3 * max(0, -dy))
            g = int(13 + 21 * vignette + 4 * max(0, -dy))
            b = int(15 + 24 * vignette + 5 * max(0, -dy))
            pix[x, y] = (r, g, b, 255)
    return img


def ellipse_points(cx: float, cy: float, rx: float, ry: float, angle: float, steps: int = 300):
    ca, sa = math.cos(angle), math.sin(angle)
    pts = []
    for i in range(steps + 1):
        t = i / steps * math.tau
        x = math.cos(t) * rx
        y = math.sin(t) * ry
        pts.append((cx + x * ca - y * sa, cy + x * sa + y * ca))
    return pts


def draw_orbit(layer: Image.Image, angle: float, width: int, alpha: int, offset: float = 0) -> None:
    size = layer.size[0]
    cx = cy = size / 2
    rx = size * (0.39 + offset)
    ry = size * 0.135
    draw = ImageDraw.Draw(layer)
    for spread in (-4, 0, 4):
        pts = ellipse_points(cx, cy + spread, rx, ry, angle, 360)
        draw.line(pts, fill=(235, 242, 244, max(20, alpha // 3)), width=max(1, width - 1), joint="curve")
    pts = ellipse_points(cx, cy, rx, ry, angle, 420)
    draw.line(pts, fill=(246, 249, 250, alpha), width=width, joint="curve")


def draw_glow_line(layer: Image.Image, angle: float, alpha: int) -> None:
    size = layer.size[0]
    cx = cy = size / 2
    pts = ellipse_points(cx, cy, size * 0.37, size * 0.118, angle, 260)
    glow = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.line(pts, fill=(255, 255, 255, alpha), width=max(2, size // 46), joint="curve")
    layer.alpha_composite(glow.filter(ImageFilter.GaussianBlur(size * 0.006)))
    ImageDraw.Draw(layer).line(pts, fill=(255, 255, 255, min(255, alpha + 35)), width=max(1, size // 94), joint="curve")


def draw_sphere(layer: Image.Image, x: float, y: float, r: float, alpha: int = 255) -> None:
    size = layer.size[0]
    box = (x - r, y - r, x + r, y + r)
    sphere = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    pix = sphere.load()
    for yy in range(max(0, int(y - r - 2)), min(size, int(y + r + 3))):
        for xx in range(max(0, int(x - r - 2)), min(size, int(x + r + 3))):
            dx = (xx - x) / r
            dy = (yy - y) / r
            d2 = dx * dx + dy * dy
            if d2 <= 1:
                shade = 1 - math.sqrt(d2)
                highlight = max(0, 1 - math.hypot(dx + 0.35, dy + 0.45))
                v = int(104 + 116 * shade + 75 * highlight)
                pix[xx, yy] = (v, v + 2, v + 4, alpha)
    shadow = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.ellipse(box, fill=(0, 0, 0, 110))
    layer.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(max(2, int(size * 0.012)))))
    layer.alpha_composite(sphere)
    ImageDraw.Draw(layer).ellipse(box, outline=(232, 236, 238, 110), width=max(1, int(size * 0.004)))


def create_icon(size: int = 1024) -> Image.Image:
    random.seed(37)
    base = gradient_base(size)
    mask = rounded_mask(size, int(size * 0.205))

    mist = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    md = ImageDraw.Draw(mist)
    for angle, alpha, width, offset in [
        (-0.75, 42, size // 95, 0.015),
        (0.36, 36, size // 105, 0.02),
        (1.14, 30, size // 112, 0.005),
    ]:
        draw_orbit(mist, angle, width, alpha, offset)
    base.alpha_composite(mist.filter(ImageFilter.GaussianBlur(size * 0.004)))

    orbit = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    for angle in (-0.68, 0.42, 1.18):
        draw_glow_line(orbit, angle, 152)
    draw_orbit(orbit, -0.08, max(1, size // 145), 88, 0.025)
    base.alpha_composite(orbit)

    particles = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pd = ImageDraw.Draw(particles)
    for _ in range(155):
        spread = random.gauss(0, size * 0.16)
        x = size / 2 + random.gauss(0, size * 0.16)
        y = size / 2 + spread
        if size * 0.14 < x < size * 0.86 and size * 0.10 < y < size * 0.90:
            r = random.choice([1, 1, 1, 2]) * size / 1024
            a = random.randint(42, 150)
            pd.ellipse((x - r, y - r, x + r, y + r), fill=(238, 244, 246, a))
    base.alpha_composite(particles)

    draw_sphere(base, size * 0.52, size * 0.57, size * 0.083, 255)
    draw_sphere(base, size * 0.58, size * 0.30, size * 0.040, 238)
    draw_sphere(base, size * 0.60, size * 0.78, size * 0.040, 224)

    gloss = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    gd = ImageDraw.Draw(gloss)
    gd.rounded_rectangle((size * 0.055, size * 0.045, size * 0.945, size * 0.955), radius=size * 0.18, outline=(255, 255, 255, 20), width=max(1, size // 95))
    gd.arc((size * 0.10, size * 0.07, size * 0.90, size * 0.56), 188, 352, fill=(255, 255, 255, 28), width=max(1, size // 120))
    base.alpha_composite(gloss)

    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.alpha_composite(base)
    out.putalpha(mask)
    return out


def save_iconset(master: Image.Image) -> None:
    ICONSET.mkdir(exist_ok=True)
    sizes = {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }
    for name, px in sizes.items():
        master.resize((px, px), Image.Resampling.LANCZOS).save(ICONSET / name)


def main() -> None:
    master = create_icon(1024)
    master.resize((512, 512), Image.Resampling.LANCZOS).save(ROOT / "icon.png")
    save_iconset(master)


if __name__ == "__main__":
    main()
