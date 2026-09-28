"""Claude-specific API, authentication and presentation settings."""

from __future__ import annotations

import os
from pathlib import Path

from widget_common.config import WidgetConfig

from .branding import claude_logo

USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
TOKEN_URL = "https://console.anthropic.com/v1/oauth/token"
# Public OAuth client id used by Claude Code CLI.
OAUTH_CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"

API_HEADERS_BASE = {
    "anthropic-beta": "oauth-2025-04-20",
    "User-Agent": "claude-code/2.0.31",
}

TOKEN_PATH_CANDIDATES = [
    Path.home() / ".claude" / ".credentials.json",
    Path(os.environ.get("APPDATA", "")) / "Claude" / ".credentials.json",
    Path(os.environ.get("APPDATA", "")) / "Claude Code" / ".credentials.json",
    Path(os.environ.get("LOCALAPPDATA", "")) / "Claude" / ".credentials.json",
    Path(os.environ.get("LOCALAPPDATA", "")) / "Claude Code" / ".credentials.json",
]


WIDGET_CONFIG = WidgetConfig(
    tray_name="claude-usage",
    display_name="Claude Usage Widget",
    popup_title="Claude Code · Usage",
    tooltip_name="Claude Code",
    accent="#D97757",
    accent_hover="#c0613f",
    logo_factory=claude_logo,
)
