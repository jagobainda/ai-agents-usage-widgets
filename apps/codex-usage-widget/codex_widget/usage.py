"""Parse account rate-limit snapshots returned by Codex App Server."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any, Optional

from widget_common.models import Usage, UsageLimit
from widget_common.utils import (
    datetime_from_unix,
    duration_label,
    duration_short_label,
)


def _percentage(value: object) -> float:
    if isinstance(value, bool):
        return 0.0
    try:
        return max(0.0, min(100.0, float(value))) / 100.0
    except (TypeError, ValueError):
        return 0.0


def _readable_name(value: object) -> Optional[str]:
    if not isinstance(value, str) or not value.strip():
        return None
    return value.strip().replace("_", " ").replace("-", " ").title()


def _details_for_snapshots(snapshots: list[tuple[str, dict[str, Any]]]) -> tuple[str, ...]:
    plan_detail: Optional[str] = None
    credits_detail: Optional[str] = None
    limit_reached = False
    for _limit_id, snapshot in snapshots:
        plan_type = snapshot.get("planType")
        credits = snapshot.get("credits")
        if plan_detail is None and isinstance(plan_type, str) and plan_type:
            plan_detail = f"Plan · {_readable_name(plan_type) or plan_type}"
        if credits_detail is None and isinstance(credits, dict):
            if credits.get("unlimited") is True:
                credits_detail = "Credits · Unlimited"
            elif credits.get("hasCredits") is True and credits.get("balance") is not None:
                credits_detail = f"Credits · {credits.get('balance')}"
        limit_reached = limit_reached or bool(snapshot.get("rateLimitReachedType"))
    details = [detail for detail in (plan_detail, credits_detail) if detail]
    if limit_reached:
        details.append("A usage limit has been reached")
    return tuple(details)


def parse_rate_limits_response(payload: object) -> Usage:
    """Parse legacy and multi-bucket App Server responses defensively."""
    if not isinstance(payload, dict):
        payload = {}
    if isinstance(payload.get("result"), dict):
        payload = payload["result"]

    snapshots: list[tuple[str, dict[str, Any]]] = []
    by_limit_id = payload.get("rateLimitsByLimitId")
    if isinstance(by_limit_id, dict) and by_limit_id:
        for map_key, raw_snapshot in by_limit_id.items():
            if not isinstance(raw_snapshot, dict):
                continue
            limit_id = raw_snapshot.get("limitId") or map_key or "limit"
            snapshots.append((str(limit_id), raw_snapshot))
    else:
        raw_snapshot = payload.get("rateLimits")
        if isinstance(raw_snapshot, dict):
            limit_id = raw_snapshot.get("limitId") or "codex"
            snapshots.append((str(limit_id), raw_snapshot))

    limits: list[UsageLimit] = []
    primary_key: Optional[str] = None
    multiple_buckets = len(snapshots) > 1
    for bucket_index, (limit_id, snapshot) in enumerate(snapshots, start=1):
        display_name = _readable_name(snapshot.get("limitName"))
        if display_name is None and multiple_buckets and limit_id.lower() != "codex":
            display_name = _readable_name(limit_id) or f"Limit {bucket_index}"
        window_slots = ["primary", "secondary"]
        window_slots.extend(
            key
            for key, value in snapshot.items()
            if key not in window_slots
            and isinstance(value, dict)
            and "usedPercent" in value
        )
        for slot in window_slots:
            window = snapshot.get(slot)
            if not isinstance(window, dict):
                continue
            minutes = window.get("windowDurationMins")
            label = duration_label(minutes)
            short_label = duration_short_label(minutes)
            if display_name:
                label = f"{label} · {display_name}"
                short_label = f"{short_label} {display_name}"
            key = f"{limit_id}:{slot}"
            limits.append(
                UsageLimit(
                    key=key,
                    label=label,
                    short_label=short_label,
                    utilization=_percentage(window.get("usedPercent")),
                    resets_at=datetime_from_unix(window.get("resetsAt")),
                    severity="reached" if snapshot.get("rateLimitReachedType") else "normal",
                    is_active=True,
                )
            )
            if primary_key is None and slot == "primary":
                primary_key = key
            if limit_id.lower() == "codex" and slot == "primary":
                primary_key = key

    return Usage(
        limits=limits,
        fetched_at=datetime.now(timezone.utc),
        primary_key=primary_key,
        details=_details_for_snapshots(snapshots),
    )
