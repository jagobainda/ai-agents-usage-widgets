"""Antigravity usage provider backed by the official ``agy`` CLI."""

from __future__ import annotations

from widget_common.models import Usage
from widget_common.provider import ProviderError

from .cli import (
    AntigravityCLIClient,
    AntigravityCLIError,
    AntigravityNotFoundError,
    AntigravityPayloadError,
    AntigravityTimeoutError,
)
from .usage import AntigravityEmptyUsageError, parse_usage_response


def _cli_provider_error(error: AntigravityCLIError) -> ProviderError:
    detail = f"{error} {error.detail}".lower()
    if any(
        term in detail
        for term in (
            "unauthorized",
            "unauthenticated",
            "authentication required",
            "not logged",
            "login",
            "sign in",
            "credential",
            "oauth",
        )
    ):
        return ProviderError(
            "Antigravity is not signed in. Run 'agy' to sign in and try again."
        )
    if any(term in detail for term in ("429", "rate limit", "resource_exhausted")):
        return ProviderError(
            "Antigravity usage is temporarily rate limited. The widget will retry."
        )
    if any(
        term in detail
        for term in (
            "unknown flag",
            "flag provided but not defined",
            "unknown command",
            "unsupported output format",
        )
    ):
        return ProviderError(
            "This Antigravity CLI version does not support JSON usage output. Run 'agy update'."
        )
    if any(
        term in detail
        for term in (
            "connection",
            "network",
            "dns",
            "offline",
            "unreachable",
            "dial tcp",
            "deadline exceeded",
            "timeout",
            "unavailable",
        )
    ):
        return ProviderError(
            "Antigravity usage could not be reached. The widget will retry."
        )
    return ProviderError(
        "Antigravity CLI could not return usage data. The widget will retry."
    )


class AntigravityProvider:
    def __init__(self, client: AntigravityCLIClient | None = None) -> None:
        self.client = client or AntigravityCLIClient()

    def fetch_usage(self) -> Usage:
        try:
            payload = self.client.fetch_usage_payload()
            return parse_usage_response(payload)
        except ProviderError:
            raise
        except AntigravityNotFoundError as exc:
            raise ProviderError(
                "Antigravity CLI is not installed or is not in PATH. Install 'agy' first."
            ) from exc
        except AntigravityTimeoutError as exc:
            raise ProviderError(
                "Antigravity CLI timed out. The widget will retry."
            ) from exc
        except AntigravityPayloadError as exc:
            raise ProviderError(
                "Antigravity CLI returned an incompatible usage response."
            ) from exc
        except AntigravityEmptyUsageError as exc:
            raise ProviderError(
                "Antigravity CLI returned no usage windows."
            ) from exc
        except AntigravityCLIError as exc:
            raise _cli_provider_error(exc) from exc

    def close(self) -> None:
        self.client.close()
