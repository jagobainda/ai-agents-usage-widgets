"""Antigravity branding rendered for the title bar and executable."""

from __future__ import annotations

import re

import aggdraw
from PIL import Image


ANTIGRAVITY_GREEN = (52, 168, 83)

_ANTIGRAVITY_PATH = (
    "M19.94 20.59C21.03 21.41 22.67 20.86 21.17 19.36"
    "C16.67 15 17.62 3 12.03 3C6.44 3 7.39 15 2.89 19.36"
    "C1.25 21 3.03 21.41 4.12 20.59"
    "C8.35 17.73 8.07 12.68 12.03 12.68"
    "C15.99 12.68 15.71 17.73 19.94 20.59Z"
)


def _scale_path(path: str, scale: float) -> str:
    return re.sub(
        r"-?(?:\d+\.?\d*|\.\d+)",
        lambda match: f"{float(match.group()) * scale:.4f}",
        path,
    )


def antigravity_logo(
    size: int = 64,
    color: tuple[int, int, int] = ANTIGRAVITY_GREEN,
) -> Image.Image:
    canvas = max(4, size * 4)
    scale = canvas / 24.0
    image = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    draw = aggdraw.Draw(image)
    draw.symbol(
        (0, 0),
        aggdraw.Symbol(_scale_path(_ANTIGRAVITY_PATH, scale)),
        None,
        aggdraw.Brush(color, 255),
    )
    draw.flush()
    return image.resize((size, size), Image.Resampling.LANCZOS)
