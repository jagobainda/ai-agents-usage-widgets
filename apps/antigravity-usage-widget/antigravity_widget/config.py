"""Antigravity widget presentation settings."""

from __future__ import annotations

from widget_common.config import WidgetConfig

from .branding import antigravity_logo


WIDGET_CONFIG = WidgetConfig(
    tray_name="antigravity-usage",
    display_name="Antigravity Usage Widget",
    popup_title="Antigravity · Usage",
    tooltip_name="Antigravity",
    accent="#34A853",
    accent_hover="#1E5F30",
    logo_factory=antigravity_logo,
)
