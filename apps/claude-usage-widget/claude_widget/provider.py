"""Claude authentication and usage provider."""

from __future__ import annotations

from typing import Optional

from widget_common.models import Usage
from widget_common.provider import ProviderError

from .api import fetch_usage
from .auth import OAuthToken, RefreshTokenError, load_token


class ClaudeProvider:
    def __init__(self) -> None:
        self._token: Optional[OAuthToken] = None

    def fetch_usage(self) -> Usage:
        try:
            token = self._token if self._token is not None else load_token()
            try:
                usage, token = fetch_usage(token)
            except RefreshTokenError:
                token = load_token()
                usage, token = fetch_usage(token)
            self._token = token
            return usage
        except FileNotFoundError as exc:
            raise ProviderError(
                "Claude Code is not signed in. Run 'claude login' and try again."
            ) from exc
        except RefreshTokenError as exc:
            raise ProviderError(
                "Session expired. Sign in to Claude Code again."
            ) from exc

    def close(self) -> None:
        return None
