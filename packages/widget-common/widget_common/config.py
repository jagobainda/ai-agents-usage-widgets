"""Configuration objects shared by the tray shell and popup."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable

from PIL import Image


@dataclass(frozen=True)
class WidgetConfig:
    tray_name: str
    display_name: str
    popup_title: str
    tooltip_name: str
    accent: str
    accent_hover: str
    logo_factory: Callable[[int], Image.Image]
    refresh_interval_seconds: float = 60.0
