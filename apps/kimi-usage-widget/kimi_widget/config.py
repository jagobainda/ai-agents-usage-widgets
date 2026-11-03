"""Kimi widget endpoints, authentication and presentation settings."""

from __future__ import annotations

from widget_common.config import WidgetConfig

from .branding import kimi_logo


DEFAULT_BASE_URL = "https://api.kimi.com/coding/v1"
DEFAULT_OAUTH_HOST = "https://auth.kimi.com"
OAUTH_CLIENT_ID = "17e5f671-d194-4dfb-9706-5516cb48c098"

WIDGET_CONFIG = WidgetConfig(
    tray_name="kimi-usage",
    display_name="Kimi Usage Widget",
    popup_title="Kimi Code · Usage",
    tooltip_name="Kimi Code",
    accent="#FFFFFF",
    accent_hover="#3A3A3A",
    logo_factory=kimi_logo,
)
