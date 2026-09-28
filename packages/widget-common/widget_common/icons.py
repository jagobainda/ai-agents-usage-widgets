"""Provider-neutral numeric tray icon rendering."""

from __future__ import annotations

from PIL import Image, ImageColor, ImageDraw, ImageFont

from .models import Usage


ICON_SIZE = 64


def _font(size: int) -> ImageFont.ImageFont:
    for name in ("segoeuib.ttf", "arialbd.ttf", "seguisb.ttf", "arial.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def make_icon(
    text: str,
    color: tuple[int, int, int],
    size: int = ICON_SIZE,
) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    bbox = (0, 0, 0, 0)
    font = _font(24)
    for font_size in (
        int(size * 0.88),
        int(size * 0.75),
        int(size * 0.62),
        int(size * 0.50),
        int(size * 0.38),
    ):
        font = _font(font_size)
        bbox = draw.textbbox((0, 0), text, font=font)
        width, height = bbox[2] - bbox[0], bbox[3] - bbox[1]
        if width <= size - 2 and height <= size - 2:
            break
    width, height = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (size - width) / 2 - bbox[0]
    y = (size - height) / 2 - bbox[1]
    draw.text((x, y), text, fill=color + (255,), font=font)
    return image


def usage_icon(
    usage: Usage,
    color: str,
    size: int = ICON_SIZE,
) -> Image.Image:
    primary = usage.primary
    utilization = primary.utilization if primary is not None else 0.0
    return make_icon(
        str(int(round(utilization * 100))),
        ImageColor.getrgb(color),
        size=size,
    )


def loading_icon(size: int = ICON_SIZE) -> Image.Image:
    return make_icon("…", (122, 129, 140), size=size)


def error_icon(size: int = ICON_SIZE) -> Image.Image:
    return make_icon("!", (122, 129, 140), size=size)
