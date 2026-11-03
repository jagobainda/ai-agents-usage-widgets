"""Kimi Code usage endpoint client and response parser."""

from __future__ import annotations

import math
from datetime import datetime, timezone
from typing import Callable

import requests
from widget_common.models import Usage, UsageLimit
from widget_common.utils import parse_iso_datetime

from .auth import OAuthToken, refresh_token


_WINDOWS = (
    ("limit_5h", "5-hour window", "5h"),
    ("limit_7d", "Weekly window", "7d"),
    ("limit_month_total", "Monthly window", "month"),
)


class KimiAPIError(RuntimeError):
    def __init__(self, status_code: int) -> None:
        super().__init__(f"Kimi usage endpoint returned HTTP {status_code}.")
        self.status_code = status_code


class KimiPayloadError(RuntimeError):
    pass


def _ratio(value: object) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        ratio = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(ratio):
        return None
    return max(0.0, min(1.0, ratio))


def parse_usage_response(payload: object) -> Usage:
    container = payload.get("usages") if isinstance(payload, dict) else None
    limits: list[UsageLimit] = []
    if isinstance(container, dict):
        for key, label, short_label in _WINDOWS:
            window = container.get(key)
            if not isinstance(window, dict):
                continue
            utilization = _ratio(window.get("used_ratio"))
            if utilization is None:
                continue
            limits.append(
                UsageLimit(
                    key=f"kimi:{key}",
                    label=label,
                    short_label=short_label,
                    utilization=utilization,
                    resets_at=parse_iso_datetime(window.get("reset_time")),
                    severity="reached" if utilization >= 1.0 else "normal",
                    is_active=True,
                )
            )

    if not limits:
        raise KimiPayloadError("Kimi Code returned no usable usage windows.")

    details: tuple[str, ...] = ()
    month_code = container.get("limit_month_code")
    if isinstance(month_code, dict):
        ratio = _ratio(month_code.get("used_ratio"))
        if ratio is not None and any(item.key == "kimi:limit_month_total" for item in limits):
            details = (f"Monthly · Kimi Code {int(round(ratio * 100))}% used",)

    return Usage(
        limits=limits,
        fetched_at=datetime.now(timezone.utc),
        primary_key="kimi:limit_5h",
        details=details,
    )


def fetch_usage(
    token: OAuthToken,
    request_get: Callable[..., requests.Response] = requests.get,
) -> Usage:
    if token.is_expired:
        token = refresh_token(token)
    headers = {
        "Authorization": f"Bearer {token.access_token}",
        "Accept": "application/json",
        "User-Agent": "KimiUsageWidget/1.0",
    }
    response = request_get(token.usage_url, headers=headers, timeout=(5, 15))
    if response.status_code == 401:
        token = refresh_token(token)
        headers["Authorization"] = f"Bearer {token.access_token}"
        response = request_get(token.usage_url, headers=headers, timeout=(5, 15))
    if response.status_code != 200:
        raise KimiAPIError(response.status_code)
    try:
        payload = response.json()
    except ValueError as exc:
        raise KimiPayloadError("Kimi Code returned invalid JSON.") from exc
    return parse_usage_response(payload)
