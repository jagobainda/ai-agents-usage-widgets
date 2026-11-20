"""Parse JSON usage snapshots returned by Antigravity CLI."""

from __future__ import annotations

import math
import re
from datetime import datetime, timezone
from typing import Optional

from widget_common.models import Usage, UsageLimit
from widget_common.utils import parse_iso_datetime

from .cli import AntigravityPayloadError


_WINDOWS: dict[str, tuple[int, str, str]] = {
    "5h": (0, "5-hour window", "5h"),
    "daily": (1, "Daily window", "1d"),
    "1d": (1, "Daily window", "1d"),
    "24h": (1, "Daily window", "1d"),
    "weekly": (2, "Weekly window", "7d"),
    "7d": (2, "Weekly window", "7d"),
    "monthly": (3, "Monthly window", "month"),
    "30d": (3, "Monthly window", "month"),
}


class AntigravityEmptyUsageError(RuntimeError):
    pass


def _fraction(value: object) -> Optional[float]:
    if value is None or isinstance(value, bool):
        return None
    try:
        fraction = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(fraction):
        return None
    return max(0.0, min(1.0, fraction))


def _clean_group_name(value: object) -> Optional[str]:
    if not isinstance(value, str):
        return None
    cleaned = value.strip()
    if not cleaned:
        return None
    stripped = re.sub(r"\s+models$", "", cleaned, flags=re.IGNORECASE).strip()
    return stripped or cleaned


def _window_meta(window: object, bucket_name: object) -> tuple[int, str, str]:
    normalized = window.strip().lower() if isinstance(window, str) else ""
    if normalized in _WINDOWS:
        return _WINDOWS[normalized]
    if isinstance(bucket_name, str) and bucket_name.strip():
        cleaned = re.sub(
            r"\s+remaining$", "", bucket_name.strip(), flags=re.IGNORECASE
        ).strip()
        if cleaned:
            return (4, cleaned, normalized or cleaned)
    if normalized:
        return (4, f"{normalized} window", normalized)
    return (4, "Usage window", "usage")


def _bucket_remaining(bucket: dict) -> Optional[float]:
    if "remaining_fraction" in bucket:
        return _fraction(bucket.get("remaining_fraction"))
    # Protobuf JSON omits 0.0 float values when a bucket is exhausted.
    reset_time = bucket.get("reset_time")
    if isinstance(reset_time, str) and reset_time.strip():
        return 0.0
    return None


def parse_usage_response(payload: object) -> Usage:
    if not isinstance(payload, dict):
        raise AntigravityPayloadError("Antigravity CLI returned an invalid payload.")

    command = payload.get("command")
    if not isinstance(command, dict):
        raise AntigravityPayloadError("Antigravity CLI response is missing command data.")

    command_name = command.get("name")
    if command_name is not None and command_name != "usage":
        raise AntigravityPayloadError("Antigravity CLI returned an unexpected command.")

    data = command.get("data")
    if not isinstance(data, dict):
        raise AntigravityPayloadError("Antigravity CLI response is missing usage data.")

    groups = data.get("groups")
    if not isinstance(groups, list):
        raise AntigravityPayloadError("Antigravity CLI response is missing usage groups.")

    parsed_groups: list[tuple[Optional[str], list[tuple[int, int, str, str, str, str, float, Optional[datetime]]]]] = []
    for group_index, group in enumerate(groups):
        if not isinstance(group, dict):
            continue
        raw_buckets = group.get("buckets")
        if not isinstance(raw_buckets, list):
            continue
        group_name = _clean_group_name(group.get("name"))
        entries = []
        for bucket_index, bucket in enumerate(raw_buckets):
            if not isinstance(bucket, dict):
                continue
            remaining = _bucket_remaining(bucket)
            if remaining is None:
                continue
            order, window_label, window_short = _window_meta(
                bucket.get("window"),
                bucket.get("name"),
            )
            raw_id = bucket.get("id")
            bucket_id = (
                raw_id.strip()
                if isinstance(raw_id, str) and raw_id.strip()
                else f"group-{group_index}-{window_short}-{bucket_index}"
            )
            window_key = (
                bucket.get("window").strip().lower()
                if isinstance(bucket.get("window"), str)
                else ""
            )
            utilization = max(0.0, min(1.0, 1.0 - remaining))
            resets_at = parse_iso_datetime(bucket.get("reset_time"))
            entries.append(
                (
                    order,
                    bucket_index,
                    bucket_id,
                    window_key,
                    window_label,
                    window_short,
                    utilization,
                    resets_at,
                )
            )
        if entries:
            entries.sort(key=lambda item: (item[0], item[1]))
            parsed_groups.append((group_name, entries))

    if not parsed_groups:
        raise AntigravityEmptyUsageError("Antigravity CLI returned no usage windows.")

    multiple_groups = len(parsed_groups) > 1
    limits: list[UsageLimit] = []
    seen_keys: set[str] = set()
    primary_key: Optional[str] = None

    for group_name, entries in parsed_groups:
        short_group = (
            re.sub(r"\s+and\s+", "/", group_name, flags=re.IGNORECASE)
            if group_name
            else None
        )
        for (
            _order,
            _bucket_index,
            bucket_id,
            window_key,
            window_label,
            window_short,
            utilization,
            resets_at,
        ) in entries:
            key = f"antigravity:{bucket_id}"
            suffix = 2
            while key in seen_keys:
                key = f"antigravity:{bucket_id}-{suffix}"
                suffix += 1
            seen_keys.add(key)

            if multiple_groups and group_name:
                label = f"{window_label} · {group_name}"
                short_label = f"{short_group} {window_short}"
            else:
                label = window_label
                short_label = window_short

            limits.append(
                UsageLimit(
                    key=key,
                    label=label,
                    short_label=short_label,
                    utilization=utilization,
                    resets_at=resets_at,
                    severity="reached" if utilization >= 1.0 else "normal",
                    is_active=True,
                )
            )
            if primary_key is None and window_key == "5h":
                primary_key = key

    return Usage(
        limits=limits,
        fetched_at=datetime.now(timezone.utc),
        primary_key=primary_key or limits[0].key,
    )
