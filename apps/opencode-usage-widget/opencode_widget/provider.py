"""OpenCode Go usage provider."""

from __future__ import annotations

from typing import Callable

import requests
from widget_common.models import Usage
from widget_common.provider import ProviderError

from .api import OpenCodeAPIError, OpenCodePayloadError, fetch_usage
from .auth import OpenCodeCredentialError, load_api_key


_CONNECT_MESSAGE = (
    "OpenCode Go is not connected. Run OpenCode, use '/connect', "
    "and select OpenCode Go."
)


class OpenCodeProvider:
    def __init__(
        self,
        api_key_loader: Callable[[], str] = load_api_key,
        usage_fetcher: Callable[[str], Usage] = fetch_usage,
    ) -> None:
        self._api_key_loader = api_key_loader
        self._usage_fetcher = usage_fetcher

    def fetch_usage(self) -> Usage:
        try:
            return self._usage_fetcher(self._api_key_loader())
        except OpenCodeCredentialError as exc:
            raise ProviderError(_CONNECT_MESSAGE) from exc
        except OpenCodeAPIError as exc:
            if exc.status_code == 401:
                message = (
                    "OpenCode Go rejected the saved API key. Reconnect it with '/connect'."
                )
            elif exc.status_code == 403:
                message = "An active OpenCode Go subscription is required."
            elif exc.status_code == 429:
                message = "OpenCode Go usage is temporarily rate limited. The widget will retry."
            elif exc.status_code >= 500:
                message = "OpenCode Go usage is temporarily unavailable. The widget will retry."
            else:
                message = f"OpenCode Go could not return usage data (HTTP {exc.status_code})."
            raise ProviderError(message) from exc
        except OpenCodePayloadError as exc:
            raise ProviderError(
                "OpenCode Go returned an incompatible usage response."
            ) from exc
        except requests.Timeout as exc:
            raise ProviderError("OpenCode Go usage timed out. The widget will retry.") from exc
        except requests.RequestException as exc:
            raise ProviderError(
                "OpenCode Go usage could not be reached. The widget will retry."
            ) from exc

    def close(self) -> None:
        return None
