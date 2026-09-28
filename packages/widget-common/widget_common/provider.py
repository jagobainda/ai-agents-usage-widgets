"""Provider contract and public-facing provider errors."""

from __future__ import annotations

from typing import Protocol

from .models import Usage


class ProviderError(RuntimeError):
    """An error message safe to show in the widget UI."""


class UsageProvider(Protocol):
    def fetch_usage(self) -> Usage:
        ...

    def close(self) -> None:
        ...
