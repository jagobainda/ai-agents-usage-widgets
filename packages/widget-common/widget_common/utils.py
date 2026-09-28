"""Date and time helpers shared by both providers."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional


def parse_iso_datetime(value: object) -> Optional[datetime]:
    if not isinstance(value, str) or not value:
        return None
    try:
        if value.endswith("Z"):
            value = value[:-1] + "+00:00"
        parsed = datetime.fromisoformat(value)
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        return parsed
    except (TypeError, ValueError):
        return None


def datetime_from_unix(value: object) -> Optional[datetime]:
    if value is None or isinstance(value, bool):
        return None
    try:
        return datetime.fromtimestamp(float(value), timezone.utc)
    except (OSError, OverflowError, TypeError, ValueError):
        return None


def format_until(value: Optional[datetime]) -> str:
    if value is None:
        return "—"
    if value.tzinfo is None:
        value = value.replace(tzinfo=timezone.utc)
    total = int((value - datetime.now(timezone.utc)).total_seconds())
    if total <= 0:
        return "now"
    hours, remainder = divmod(total, 3600)
    minutes, _ = divmod(remainder, 60)
    if hours >= 24:
        days, hours = divmod(hours, 24)
        return f"{days}d {hours}h {minutes}m"
    return f"{hours}h {minutes:02d}m"


def duration_label(minutes: object) -> str:
    """Return a human-readable label without assuming fixed quota windows."""
    if minutes is None or isinstance(minutes, bool):
        return "Usage window"
    try:
        total = int(minutes)
    except (TypeError, ValueError):
        return "Usage window"
    if total <= 0:
        return "Usage window"
    if total >= 2 * 7 * 24 * 60 and total % (7 * 24 * 60) == 0:
        count = total // (7 * 24 * 60)
        return f"{count}-week window"
    if total % (24 * 60) == 0:
        count = total // (24 * 60)
        return f"{count}-day window"
    if total % 60 == 0:
        count = total // 60
        return f"{count}-hour window"
    return f"{total}-minute window"


def duration_short_label(minutes: object) -> str:
    if minutes is None or isinstance(minutes, bool):
        return "usage"
    try:
        total = int(minutes)
    except (TypeError, ValueError):
        return "usage"
    if total <= 0:
        return "usage"
    if total >= 2 * 7 * 24 * 60 and total % (7 * 24 * 60) == 0:
        return f"{total // (7 * 24 * 60)}w"
    if total % (24 * 60) == 0:
        return f"{total // (24 * 60)}d"
    if total % 60 == 0:
        return f"{total // 60}h"
    return f"{total}m"
