"""Shared dark theme and usage status colours."""

from __future__ import annotations


class Theme:
    BG = "#1b1c20"
    TITLEBAR = "#131418"
    CARD = "#24262c"
    BORDER = "#34363d"

    TEXT = "#e9eaee"
    TEXT_MUTED = "#9aa0a8"
    TEXT_DIM = "#6c7079"

    BTN_BG = "#2c2f36"
    BTN_BG_HOV = "#3a3e47"
    BTN_FG = "#e9eaee"
    TRACK = "#2c2f36"

    OK = "#3ecf63"
    WARN_LOW = "#f0c419"
    WARN_HIGH = "#ff9933"
    DANGER = "#e64960"
    NEUTRAL = "#7a818c"

    FONT_FAMILY = "Segoe UI"
    FONT_TITLE = (FONT_FAMILY, 10, "bold")
    FONT_BODY = (FONT_FAMILY, 10)
    FONT_HEAD = (FONT_FAMILY, 11, "bold")
    FONT_PCT = (FONT_FAMILY, 14, "bold")
    FONT_SMALL = (FONT_FAMILY, 8)


def status_color_hex(utilization: float) -> str:
    if utilization >= 0.90:
        return Theme.DANGER
    if utilization >= 0.75:
        return Theme.WARN_HIGH
    if utilization >= 0.50:
        return Theme.WARN_LOW
    return Theme.OK
