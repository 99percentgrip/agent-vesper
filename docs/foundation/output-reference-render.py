"""Rasterize actual TestBackend cells; evidence only, never production rendering.

Requires Pillow and Fedora Noto mono/emoji/symbol fonts. Run from repository root.
Input: /tmp/vesper-output-captures JSON from ui::output_upgrade_reference.
Output: three cropped transcript PNGs beside this script. Terminal font rendering
may differ; positions, text and RGB colors come from the actual frame buffer.
"""

import json
import re
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

CAPTURES = Path("/tmp/vesper-output-captures")
OUTPUT = Path(__file__).parent
FONTS = Path("/usr/share/fonts/google-noto")
NORMAL = ImageFont.truetype(str(FONTS / "NotoSansMono-Regular.ttf"), 16)
BOLD = ImageFont.truetype(str(FONTS / "NotoSansMono-Bold.ttf"), 16)
SYMBOL = ImageFont.truetype(str(FONTS / "NotoSansSymbols2-Regular.ttf"), 16)
EMOJI = ImageFont.truetype(
    "/usr/share/fonts/google-noto-emoji-fonts/NotoEmoji-Regular.ttf", 16
)
COLORS = {
    "Black": (0, 0, 0),
    "White": (240, 240, 240),
    "Green": (50, 190, 90),
    "Red": (240, 65, 70),
    "Cyan": (30, 190, 230),
    "Yellow": (235, 190, 60),
    "Blue": (60, 130, 235),
    "Magenta": (190, 95, 200),
    "Gray": (170, 170, 170),
    "DarkGray": (90, 90, 90),
}


def color(value, default):
    rgb = re.fullmatch(r"Rgb\((\d+), (\d+), (\d+)\)", value)
    return tuple(map(int, rgb.groups())) if rgb else COLORS.get(value, default)


def render(source, target):
    frame = json.loads((CAPTURES / f"{source}.json").read_text())
    width, height, cells = frame["width"], frame["height"], frame["cells"]
    background = color(cells[width]["bg"], (0, 0, 0))
    image = Image.new("RGB", (width * 10, height * 23), background)
    draw = ImageDraw.Draw(image)
    # Paint backgrounds first, so continuation cells cannot cover wide glyphs.
    for index, cell in enumerate(cells):
        x, y = (index % width) * 10, (index // width) * 23
        draw.rectangle(
            (x, y, x + 9, y + 22), fill=color(cell["bg"], background)
        )
    for index, cell in enumerate(cells):
        x, y = (index % width) * 10, (index // width) * 23
        text = cell["text"]
        font = BOLD if cell["bold"] else NORMAL
        if any(ord(char) >= 0x1F000 or char in "✅📝" for char in text):
            font = EMOJI
        elif text in ["✦", "✧", "✶", "✷", "✹", "✎"]:
            font = SYMBOL
        draw.text((x, y), text, font=font, fill=color(cell["fg"], (230, 230, 230)))
    # Remove only the empty lower viewport and composer, preserving transcript.
    last = max(
        y
        for y in range(height - 6)
        if any(cell["text"].strip() for cell in cells[y * width : (y + 1) * width])
    )
    image.crop((0, 0, width * 10, (last + 2) * 23)).save(
        OUTPUT / f"output-reference-{target}.png"
    )


if __name__ == "__main__":
    for source, target in [
        ("chatgpt-black-120", "dark"),
        ("nord-80", "nord"),
        ("chatgpt-white-80", "light"),
    ]:
        render(source, target)
