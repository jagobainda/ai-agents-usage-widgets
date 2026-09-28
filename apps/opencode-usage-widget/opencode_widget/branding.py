"""Monochrome OpenCode mark for the title bar and executable."""

from __future__ import annotations

from PIL import Image, ImageDraw


# Colours and geometry are taken from the first glyph of OpenCode's official
# MIT-licensed logo.svg. The glyph is fitted to a square icon canvas.
OPENCODE_GRAY = (101, 99, 99)
OPENCODE_LIGHT_GRAY = (207, 206, 205)


def opencode_logo(
    size: int = 64,
    color: tuple[int, int, int] = OPENCODE_GRAY,
) -> Image.Image:
    supersampling = 4
    canvas = max(4, size * supersampling)
    image = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    # Original glyph bounds are 24 x 30. Preserve that ratio and its 6-unit
    # border while centring it in the square canvas.
    height = int(canvas * 0.84)
    width = int(height * 24 / 30)
    left = (canvas - width) // 2
    top = (canvas - height) // 2
    right = left + width
    bottom = top + height
    draw.rectangle((left, top, right, bottom), fill=color + (255,))

    inner_left = left + width * 6 // 24
    inner_right = left + width * 18 // 24
    inner_top = top + height * 6 // 30
    inner_bottom = top + height * 24 // 30
    draw.rectangle(
        (inner_left, inner_top, inner_right, inner_bottom),
        fill=(0, 0, 0, 0),
    )
    fill_top = top + height * 12 // 30
    draw.rectangle(
        (inner_left, fill_top, inner_right, inner_bottom),
        fill=OPENCODE_LIGHT_GRAY + (255,),
    )
    return image.resize((size, size), Image.Resampling.LANCZOS)
