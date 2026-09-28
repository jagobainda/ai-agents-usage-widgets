"""OpenCode widget endpoints, paths and presentation settings."""

from __future__ import annotations

from pathlib import Path

from widget_common.config import WidgetConfig

from .branding import opencode_logo


USAGE_URL = "https://opencode.ai/zen/go/v1/usage"
AUTH_PATH = Path.home() / ".local" / "share" / "opencode" / "auth.json"

WIDGET_CONFIG = WidgetConfig(
    tray_name="opencode-usage",
    display_name="OpenCode Usage Widget",
    popup_title="OpenCode Usage Widget",
    tooltip_name="OpenCode Usage Widget",
    accent="#808080",
    accent_hover="#3A3A3A",
    logo_factory=opencode_logo,
)
