"""Provider-neutral usage data shown by the tray widget."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional


@dataclass(frozen=True)
class UsageLimit:
    key: str
    label: str
    short_label: str
    utilization: float
    resets_at: Optional[datetime]
    severity: str = "normal"
    is_active: bool = True


@dataclass(frozen=True)
class Usage:
    limits: list[UsageLimit]
    fetched_at: datetime
    primary_key: Optional[str] = None
    details: tuple[str, ...] = field(default_factory=tuple)

    @property
    def primary(self) -> Optional[UsageLimit]:
        if self.primary_key is not None:
            for limit in self.limits:
                if limit.key == self.primary_key:
                    return limit
        return self.limits[0] if self.limits else None
