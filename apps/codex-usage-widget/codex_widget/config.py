"""Codex widget presentation settings."""

from __future__ import annotations

from widget_common.config import WidgetConfig

from .branding import codex_logo


WIDGET_CONFIG = WidgetConfig(
    tray_name="codex-usage",
    display_name="Codex Usage Widget",
    popup_title="Codex Usage Widget",
    tooltip_name="Codex Usage Widget",
    accent="#4CC9F0",
    accent_hover="#176B87",
    logo_factory=codex_logo,
)
