"""OpenCode Go usage endpoint client and response parser."""

from __future__ import annotations

import math
from datetime import datetime, timezone
from typing import Callable

import requests
from widget_common.models import Usage, UsageLimit
from widget_common.utils import parse_iso_datetime

from .config import USAGE_URL


_WINDOWS = (
    ("rolling", "5-hour window", "5h"),
    ("weekly", "Weekly window", "7d"),
    ("monthly", "Monthly window", "month"),
)


class OpenCodeAPIError(RuntimeError):
    def __init__(self, status_code: int) -> None:
        super().__init__(f"OpenCode usage endpoint returned HTTP {status_code}.")
        self.status_code = status_code


class OpenCodePayloadError(RuntimeError):
    """The endpoint returned JSON without any usable usage windows."""


def _percentage(value: object) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        percent = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(percent):
        return None
    return max(0.0, min(100.0, percent)) / 100.0


def parse_usage_response(payload: object) -> Usage:
    container = payload.get("usage") if isinstance(payload, dict) else None
    limits: list[UsageLimit] = []
    if isinstance(container, dict):
        for key, label, short_label in _WINDOWS:
            window = container.get(key)
            if not isinstance(window, dict):
                continue
            utilization = _percentage(window.get("percent"))
            if utilization is None:
                continue
            status = window.get("status")
            limits.append(
                UsageLimit(
                    key=f"opencode-go:{key}",
                    label=label,
                    short_label=short_label,
                    utilization=utilization,
                    resets_at=parse_iso_datetime(window.get("resetsAt")),
                    severity="reached" if status == "rate-limited" else "normal",
                    is_active=True,
                )
            )

    if not limits:
        raise OpenCodePayloadError("OpenCode Go returned no usable usage windows.")

    return Usage(
        limits=limits,
        fetched_at=datetime.now(timezone.utc),
        primary_key="opencode-go:rolling",
        details=("Plan · OpenCode Go",),
    )


def fetch_usage(
    api_key: str,
    request_get: Callable[..., requests.Response] = requests.get,
) -> Usage:
    response = request_get(
        USAGE_URL,
        headers={
            "Authorization": f"Bearer {api_key}",
            "Accept": "application/json",
            "User-Agent": "OpenCodeUsageWidget/1.0",
        },
        timeout=(5, 15),
    )
    if response.status_code != 200:
        raise OpenCodeAPIError(response.status_code)
    try:
        payload = response.json()
    except (ValueError, requests.exceptions.JSONDecodeError) as exc:
        raise OpenCodePayloadError("OpenCode Go returned invalid JSON.") from exc
    return parse_usage_response(payload)
