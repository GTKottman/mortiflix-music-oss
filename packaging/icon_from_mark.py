"""Mortiflix Studio app icon from the Mortiflix 3D mark: the same outputs as packaging/icons.sh."""
import base64, io, os, sys
from PIL import Image, ImageDraw

ROOT = sys.argv[1]
SRC = sys.argv[2]
OUT = sys.argv[3]  # scratch dir for the .ico parts
DIR = os.path.join(ROOT, "assets/app-icon")
APP_ID = "com.mortiflix.studio"

src = Image.open(SRC).convert("RGBA")
# the mark sits a little below centre in the render: a square crop around it, mark about 72 % wide
cx, cy, side = 512, 505, 690
tile = src.crop((cx - side // 2, cy - side // 2, cx + side // 2, cy + side // 2)).resize((1024, 1024), Image.LANCZOS)

def rounded(img):
    n = img.size[0]
    mask = Image.new("L", (n * 4, n * 4), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, n * 4 - 1, n * 4 - 1), radius=round(112 / 512 * n * 4), fill=255)
    mask = mask.resize((n, n), Image.LANCZOS)
    out = img.copy()
    out.putalpha(mask)
    return out

full = rounded(tile)  # Windows, Linux, web: the full-bleed tile

def mac(n):
    # Apple's grid: the tile is 824 of 1024 px with a transparent margin
    body = round(n * 824 / 1024)
    canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    canvas.alpha_composite(full.resize((body, body), Image.LANCZOS), ((n - body) // 2, (n - body) // 2))
    return canvas

def size(n):
    return full.resize((n, n), Image.LANCZOS)

full.save(os.path.join(DIR, "mortiflix-studio-1024.png"), optimize=True)
mac(512).save(os.path.join(DIR, "mortiflix-studio-macos-512.png"), optimize=True)
for s in (16, 24, 32, 48, 64, 128, 256, 512):
    d = os.path.join(DIR, f"hicolor/{s}x{s}/apps")
    os.makedirs(d, exist_ok=True)
    size(s).save(os.path.join(d, f"{APP_ID}.png"), optimize=True)
size(128).save(os.path.join(ROOT, "apps/mortiflix-studio-web/web/favicon.png"), optimize=True)

# SVGs: the render embedded (the mark is a 3D render, not a vector drawing)
def svg(n):
    buf = io.BytesIO()
    size(n).save(buf, "PNG", optimize=True)
    b64 = base64.b64encode(buf.getvalue()).decode()
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">'
            f'<title>Mortiflix Studio</title><image width="512" height="512" href="data:image/png;base64,{b64}"/></svg>\n')
open(os.path.join(DIR, "mortiflix-studio.svg"), "w").write(svg(1024))
small = svg(256)
open(os.path.join(DIR, "mortiflix-studio-small.svg"), "w").write(small)
os.makedirs(os.path.join(DIR, "hicolor/scalable/apps"), exist_ok=True)
open(os.path.join(DIR, f"hicolor/scalable/apps/{APP_ID}.svg"), "w").write(small)

# .ico parts (the project's xtask packs them) and the macOS .icns
os.makedirs(OUT, exist_ok=True)
for s in (16, 20, 24, 32, 40, 48, 64, 128, 256):
    size(s).save(os.path.join(OUT, f"ico-{s}.png"))
mac(1024).save(os.path.join(DIR, "mortiflix-studio.icns"), sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)])
print("ok")
