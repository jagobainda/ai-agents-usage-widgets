"""Codex branding rendered from the bundled SVG icon."""

from __future__ import annotations

import math
import re
from pathlib import Path
from xml.etree import ElementTree

import aggdraw
from PIL import Image, ImageChops


CODEX_BLUE = (76, 201, 240)
_ICON_PATH = Path(__file__).with_name("codex_icon.svg")
_NUMBER_RE = re.compile(
    r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?"
)


def _path_tokens(path_data: str) -> list[str]:
    """Tokenize SVG path data, including compact arc flags such as ``01``."""
    tokens = []
    command = ""
    argument_index = 0
    index = 0
    while index < len(path_data):
        char = path_data[index]
        if char.isspace() or char == ",":
            index += 1
            continue
        if char.isalpha():
            command = char
            argument_index = 0
            tokens.append(char)
            index += 1
            continue
        if command.upper() == "A" and argument_index % 7 in (3, 4):
            if char not in "01":
                raise ValueError(f"Invalid SVG arc flag: {char}")
            tokens.append(char)
            argument_index += 1
            index += 1
            continue
        match = _NUMBER_RE.match(path_data, index)
        if match is None:
            raise ValueError(f"Invalid SVG path data at offset {index}")
        tokens.append(match.group())
        argument_index += 1
        index = match.end()
    return tokens


def _vector_angle(ux: float, uy: float, vx: float, vy: float) -> float:
    dot = ux * vx + uy * vy
    length = math.hypot(ux, uy) * math.hypot(vx, vy)
    if length == 0:
        return 0.0
    angle = math.acos(max(-1.0, min(1.0, dot / length)))
    return -angle if ux * vy - uy * vx < 0 else angle


def _arc_curves(
    start: tuple[float, float],
    rx: float,
    ry: float,
    rotation: float,
    large_arc: bool,
    sweep: bool,
    end: tuple[float, float],
) -> list[tuple[float, float, float, float, float, float]]:
    """Convert one SVG elliptical arc to cubic Bezier segments."""
    x1, y1 = start
    x2, y2 = end
    rx, ry = abs(rx), abs(ry)
    if (x1, y1) == (x2, y2) or rx == 0 or ry == 0:
        return []

    phi = math.radians(rotation % 360)
    cos_phi, sin_phi = math.cos(phi), math.sin(phi)
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p = cos_phi * dx + sin_phi * dy
    y1p = -sin_phi * dx + cos_phi * dy

    radii_scale = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry)
    if radii_scale > 1:
        scale = math.sqrt(radii_scale)
        rx *= scale
        ry *= scale

    numerator = max(
        0.0,
        rx * rx * ry * ry
        - rx * rx * y1p * y1p
        - ry * ry * x1p * x1p,
    )
    denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    coefficient = 0.0 if denominator == 0 else math.sqrt(numerator / denominator)
    if large_arc == sweep:
        coefficient = -coefficient
    cxp = coefficient * rx * y1p / ry
    cyp = -coefficient * ry * x1p / rx
    cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2
    cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2

    ux, uy = (x1p - cxp) / rx, (y1p - cyp) / ry
    vx, vy = (-x1p - cxp) / rx, (-y1p - cyp) / ry
    start_angle = _vector_angle(1, 0, ux, uy)
    sweep_angle = _vector_angle(ux, uy, vx, vy)
    if not sweep and sweep_angle > 0:
        sweep_angle -= math.tau
    elif sweep and sweep_angle < 0:
        sweep_angle += math.tau

    segment_count = max(1, math.ceil(abs(sweep_angle) / (math.pi / 2)))
    step = sweep_angle / segment_count

    def transform(x: float, y: float) -> tuple[float, float]:
        return (
            cx + rx * (cos_phi * x - sin_phi * y),
            cy + ry * (sin_phi * x + cos_phi * y),
        )

    curves = []
    for index in range(segment_count):
        angle1 = start_angle + index * step
        angle2 = angle1 + step
        alpha = 4 / 3 * math.tan(step / 4)
        cos1, sin1 = math.cos(angle1), math.sin(angle1)
        cos2, sin2 = math.cos(angle2), math.sin(angle2)
        control1 = transform(cos1 - alpha * sin1, sin1 + alpha * cos1)
        control2 = transform(cos2 + alpha * sin2, sin2 - alpha * cos2)
        point2 = transform(cos2, sin2)
        curves.append((*control1, *control2, *point2))
    return curves


def _svg_paths(path_data: str, scale: float) -> list[aggdraw.Path]:
    """Parse the commands used by the bundled icon into aggdraw paths."""
    tokens = _path_tokens(path_data)
    paths: list[aggdraw.Path] = []
    path: aggdraw.Path | None = None
    index = 0
    command = ""
    current = (0.0, 0.0)
    start = (0.0, 0.0)

    def number() -> float:
        nonlocal index
        value = float(tokens[index])
        index += 1
        return value

    def point(relative: bool) -> tuple[float, float]:
        x, y = number(), number()
        if relative:
            x += current[0]
            y += current[1]
        return x, y

    while index < len(tokens):
        if tokens[index].isalpha():
            command = tokens[index]
            index += 1
        relative = command.islower()
        op = command.upper()

        if op == "M":
            current = point(relative)
            start = current
            path = aggdraw.Path()
            path.moveto(current[0] * scale, current[1] * scale)
            paths.append(path)
            command = "l" if relative else "L"
        elif op == "L":
            current = point(relative)
            assert path is not None
            path.lineto(current[0] * scale, current[1] * scale)
        elif op == "H":
            x = number() + (current[0] if relative else 0)
            current = (x, current[1])
            assert path is not None
            path.lineto(current[0] * scale, current[1] * scale)
        elif op == "C":
            values = [number() for _ in range(6)]
            if relative:
                values = [
                    value + current[offset % 2]
                    for offset, value in enumerate(values)
                ]
            current = (values[4], values[5])
            assert path is not None
            path.curveto(*(value * scale for value in values))
        elif op == "A":
            rx, ry, rotation = number(), number(), number()
            large_arc, sweep = bool(number()), bool(number())
            end = point(relative)
            assert path is not None
            curves = _arc_curves(
                current, rx, ry, rotation, large_arc, sweep, end
            )
            if curves:
                for curve in curves:
                    path.curveto(*(value * scale for value in curve))
            else:
                path.lineto(end[0] * scale, end[1] * scale)
            current = end
        elif op == "Z":
            assert path is not None
            path.close()
            current = start
            command = ""
        else:
            raise ValueError(f"Unsupported SVG path command: {command}")
    return paths


def codex_logo(
    size: int = 64,
    color: tuple[int, int, int] = CODEX_BLUE,
) -> Image.Image:
    """Render the bundled Codex SVG with crisp, even-odd filled edges."""
    root = ElementTree.parse(_ICON_PATH).getroot()
    view_box = [float(value) for value in root.attrib["viewBox"].split()]
    path_element = root.find(".//{*}path")
    if path_element is None or "d" not in path_element.attrib:
        raise ValueError(f"No SVG path found in {_ICON_PATH}")

    supersampling = 4
    canvas = max(4, size * supersampling)
    scale = canvas / max(view_box[2], view_box[3])
    paths = _svg_paths(path_element.attrib["d"], scale)

    mask = Image.new("1", (canvas, canvas), 0)
    for path in paths:
        subpath_mask = Image.new("L", (canvas, canvas), 0)
        draw = aggdraw.Draw(subpath_mask)
        draw.path(path, None, aggdraw.Brush(255))
        draw.flush()
        mask = ImageChops.logical_xor(mask, subpath_mask.convert("1"))

    image = Image.new("RGBA", (canvas, canvas), color + (255,))
    image.putalpha(mask.convert("L"))
    return image.resize((size, size), Image.Resampling.LANCZOS)
