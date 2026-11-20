"""Antigravity branding rendered for the title bar and executable."""

from __future__ import annotations

import math
import re
from functools import lru_cache
from typing import Optional

import aggdraw
from PIL import Image, ImageDraw, ImageFilter


ANTIGRAVITY_BLUE = (49, 134, 255)
ANTIGRAVITY_LIGHT_BLUE = (116, 155, 255)
ANTIGRAVITY_RED = (252, 65, 61)
ANTIGRAVITY_YELLOW = (251, 188, 4)
ANTIGRAVITY_BRIGHT_YELLOW = (255, 228, 50)
ANTIGRAVITY_PALE_YELLOW = (255, 238, 72)
ANTIGRAVITY_GREEN = (0, 185, 92)

_VIEWBOX_X = 13.75
_VIEWBOX_Y = 14.95
_VIEWBOX_SIZE = 84.0
_WORK_PAD = 64.0
_MASTER_SIZE = 256
_ELLIPSE_STEPS = 48

_ANTIGRAVITY_PATH = (
    "M89.6992 93.695C94.3659 97.195 101.366 94.8617 94.9492 88.445"
    "C75.6992 69.7783 79.7825 18.445 55.8659 18.445"
    "C31.9492 18.445 36.0325 69.7783 16.7825 88.445"
    "C9.78251 95.445 17.3658 97.195 22.0325 93.695"
    "C40.1159 81.445 38.9492 59.8617 55.8659 59.8617"
    "C72.7825 59.8617 71.6159 81.445 89.6992 93.695Z"
)

_BLUE_BLOB_PATH = (
    "M2.56802 149.695C-15.8116 142.48 15.5987 83.1163 23.4093 63.2203"
    "C31.22 43.3244 52.4514 33.0447 70.831 40.26"
    "C89.2107 47.4753 110.996 87.2162 103.185 107.112"
    "C95.3742 127.008 20.9477 156.91 2.56802 149.695Z"
)

_LIGHT_BLUE_BLOB_PATH = (
    "M113.934 75.8079C109.013 81.5509 96.1724 78.6224 85.253 69.2667"
    "C74.3335 59.911 69.4704 47.6711 74.391 41.928"
    "C79.3116 36.185 92.1525 39.1136 103.072 48.4692"
    "C113.991 57.8249 118.855 70.0648 113.934 75.8079Z"
)

_COLOR_LAYERS = (
    ("matrix_ellipse", 22.7873, 26.8098, 22.7873, 26.8098, (-0.112784, 0.99362, -0.99362, -0.112781, 66.2473, -15.5344), ANTIGRAVITY_BRIGHT_YELLOW, 3.89034),
    ("rot_ellipse", 96.491, 35.1231, 29.5007, 30.1492, 76.9243, ANTIGRAVITY_RED, 18.8078),
    ("rot_ellipse", 9.02988, 41.6647, 30.832, 39.9417, 74.1257, ANTIGRAVITY_GREEN, 15.9884),
    ("rot_ellipse", 9.02988, 41.6647, 30.832, 39.9417, 74.1257, ANTIGRAVITY_GREEN, 15.9884),
    ("rot_ellipse", 11.2212, 42.8915, 30.22, 33.2695, 45.6065, ANTIGRAVITY_GREEN, 15.9884),
    ("rot_ellipse", 75.7546, 104.822, 29.0177, 27.943, 76.9243, ANTIGRAVITY_BLUE, 15.1937),
    ("matrix_ellipse", 33.5661, 35.4043, 33.5661, 35.4043, (-0.409539, 0.912293, -0.912294, -0.409537, 101.25, -15.1674), ANTIGRAVITY_YELLOW, 13.7698),
    ("path", _BLUE_BLOB_PATH, ANTIGRAVITY_BLUE, 12.297),
    ("path", _LIGHT_BLUE_BLOB_PATH, ANTIGRAVITY_LIGHT_BLUE, 11.0036),
    ("rot_ellipse", 92.611, 23.7962, 44.2411, 27.5016, 34.0763, ANTIGRAVITY_RED, 9.29385),
    ("rot_ellipse", 23.4949, 29.5887, 23.7071, 13.7869, 112.516, ANTIGRAVITY_PALE_YELLOW, 11.5027),
)


def _transform_path(path: str, offset_x: float, offset_y: float, scale: float) -> str:
    tokens = re.findall(r"[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)", path)
    out: list[str] = []
    coord_index = 0
    for token in tokens:
        if token.isalpha():
            out.append(token)
            coord_index = 0
            continue
        value = float(token)
        if coord_index % 2 == 0:
            transformed = (value - offset_x) * scale
        else:
            transformed = (value - offset_y) * scale
        out.append(f"{transformed:.4f}")
        coord_index += 1
    return " ".join(out)


def _ellipse_points(
    cx: float,
    cy: float,
    rx: float,
    ry: float,
    transform_fn,
    offset_x: float,
    offset_y: float,
    scale: float,
) -> list[tuple[float, float]]:
    points: list[tuple[float, float]] = []
    for step in range(_ELLIPSE_STEPS):
        theta = (2.0 * math.pi * step) / _ELLIPSE_STEPS
        px = cx + rx * math.cos(theta)
        py = cy + ry * math.sin(theta)
        tx, ty = transform_fn(px, py)
        points.append(((tx - offset_x) * scale, (ty - offset_y) * scale))
    return points


@lru_cache(maxsize=1)
def _render_multicolor_master() -> Image.Image:
    scale = _MASTER_SIZE / _VIEWBOX_SIZE
    origin_x = _VIEWBOX_X - _WORK_PAD
    origin_y = _VIEWBOX_Y - _WORK_PAD
    work_px = int(round((_VIEWBOX_SIZE + 2.0 * _WORK_PAD) * scale))

    field = Image.new("RGBA", (work_px, work_px), (0, 0, 0, 0))
    for layer in _COLOR_LAYERS:
        shape_mask = Image.new("L", (work_px, work_px), 0)
        kind = layer[0]
        if kind == "rot_ellipse":
            _, cx, cy, rx, ry, angle_deg, color, blur = layer
            rad = math.radians(angle_deg)
            cos_a, sin_a = math.cos(rad), math.sin(rad)

            def _rot(px: float, py: float, _cx=cx, _cy=cy, _c=cos_a, _s=sin_a):
                dx, dy = px - _cx, py - _cy
                return (_cx + dx * _c - dy * _s, _cy + dx * _s + dy * _c)

            pts = _ellipse_points(cx, cy, rx, ry, _rot, origin_x, origin_y, scale)
            ImageDraw.Draw(shape_mask).polygon(pts, fill=255)
        elif kind == "matrix_ellipse":
            _, cx, cy, rx, ry, matrix, color, blur = layer
            a, b, c, d, e, f = matrix

            def _mat(px: float, py: float, _m=(a, b, c, d, e, f)):
                return (_m[0] * px + _m[2] * py + _m[4], _m[1] * px + _m[3] * py + _m[5])

            pts = _ellipse_points(cx, cy, rx, ry, _mat, origin_x, origin_y, scale)
            ImageDraw.Draw(shape_mask).polygon(pts, fill=255)
        else:
            _, path_data, color, blur = layer
            path_rgba = Image.new("RGBA", (work_px, work_px), (0, 0, 0, 0))
            draw = aggdraw.Draw(path_rgba)
            draw.symbol(
                (0, 0),
                aggdraw.Symbol(_transform_path(path_data, origin_x, origin_y, scale)),
                None,
                aggdraw.Brush((255, 255, 255), 255),
            )
            draw.flush()
            shape_mask = path_rgba.split()[3]

        blurred_alpha = shape_mask.filter(ImageFilter.GaussianBlur(radius=blur * scale))
        colored = Image.new("RGBA", (work_px, work_px), color + (255,))
        colored.putalpha(blurred_alpha)
        field.alpha_composite(colored)

    pad_px = int(round(_WORK_PAD * scale))
    cropped = field.crop((pad_px, pad_px, pad_px + _MASTER_SIZE, pad_px + _MASTER_SIZE))

    mask_canvas = _MASTER_SIZE * 2
    mask_rgba = Image.new("RGBA", (mask_canvas, mask_canvas), (0, 0, 0, 0))
    mask_draw = aggdraw.Draw(mask_rgba)
    mask_draw.symbol(
        (0, 0),
        aggdraw.Symbol(_transform_path(_ANTIGRAVITY_PATH, _VIEWBOX_X, _VIEWBOX_Y, mask_canvas / _VIEWBOX_SIZE)),
        None,
        aggdraw.Brush((255, 255, 255), 255),
    )
    mask_draw.flush()
    alpha = mask_rgba.split()[3].resize((_MASTER_SIZE, _MASTER_SIZE), Image.Resampling.LANCZOS)

    result = cropped.copy()
    result.putalpha(alpha)
    return result


def antigravity_logo(
    size: int = 64,
    color: Optional[tuple[int, int, int]] = None,
) -> Image.Image:
    if color is not None:
        canvas = max(4, size * 4)
        image = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
        draw = aggdraw.Draw(image)
        draw.symbol(
            (0, 0),
            aggdraw.Symbol(_transform_path(_ANTIGRAVITY_PATH, _VIEWBOX_X, _VIEWBOX_Y, canvas / _VIEWBOX_SIZE)),
            None,
            aggdraw.Brush(color, 255),
        )
        draw.flush()
        return image.resize((size, size), Image.Resampling.LANCZOS)

    master = _render_multicolor_master()
    if size == _MASTER_SIZE:
        return master.copy()
    return master.resize((size, size), Image.Resampling.LANCZOS)
