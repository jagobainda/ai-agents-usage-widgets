"""Kimi Code authentication and usage provider."""

from __future__ import annotations

from typing import Callable

import requests
from widget_common.models import Usage
from widget_common.provider import ProviderError

from .api import KimiAPIError, KimiPayloadError, fetch_usage
from .auth import (
    KimiCredentialError,
    KimiSettings,
    OAuthToken,
    RefreshTokenError,
    load_settings,
    load_token,
)


class KimiProvider:
    def __init__(
        self,
        settings_loader: Callable[[], KimiSettings] = load_settings,
        token_loader: Callable[[KimiSettings], OAuthToken] = load_token,
        usage_fetcher: Callable[[OAuthToken], Usage] = fetch_usage,
    ) -> None:
        self._settings_loader = settings_loader
        self._token_loader = token_loader
        self._usage_fetcher = usage_fetcher

    def _load_token(self) -> OAuthToken:
        return self._token_loader(self._settings_loader())

    def fetch_usage(self) -> Usage:
        try:
            token = self._load_token()
            try:
                return self._usage_fetcher(token)
            except RefreshTokenError:
                return self._usage_fetcher(self._load_token())
        except KimiCredentialError as exc:
            raise ProviderError(
                "Kimi Code is not signed in. Run 'kimi login' and try again."
            ) from exc
        except RefreshTokenError as exc:
            raise ProviderError("Session expired. Sign in to Kimi Code again.") from exc
        except KimiAPIError as exc:
            if exc.status_code in (401, 403):
                message = "Kimi Code rejected the saved session. Sign in again."
            elif exc.status_code == 404:
                message = "This Kimi Code account does not provide usage data."
            elif exc.status_code == 429:
                message = "Kimi Code usage is temporarily rate limited. The widget will retry."
            elif exc.status_code >= 500:
                message = "Kimi Code usage is temporarily unavailable. The widget will retry."
            else:
                message = f"Kimi Code could not return usage data (HTTP {exc.status_code})."
            raise ProviderError(message) from exc
        except KimiPayloadError as exc:
            raise ProviderError("Kimi Code returned an incompatible usage response.") from exc
        except requests.Timeout as exc:
            raise ProviderError("Kimi Code usage timed out. The widget will retry.") from exc
        except requests.RequestException as exc:
            raise ProviderError("Kimi Code usage could not be reached. The widget will retry.") from exc

    def close(self) -> None:
        return None
