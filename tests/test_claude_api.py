from __future__ import annotations

import unittest

import test_support  # noqa: F401
from claude_widget.api import parse_limits


class ClaudeUsageParsingTests(unittest.TestCase):
    def test_parses_current_limits_array(self) -> None:
        limits = parse_limits(
            {
                "limits": [
                    {
                        "kind": "session",
                        "percent": 37,
                        "resets_at": "2026-09-21T18:30:00Z",
                        "severity": "normal",
                        "is_active": True,
                    },
                    {
                        "kind": "weekly_scoped",
                        "percent": 81,
                        "resets_at": None,
                        "scope": {"model": {"display_name": "Opus"}},
                        "unexpected": "ignored",
                    },
                ],
                "unknown_top_level": {"future": True},
            }
        )

        self.assertEqual([limit.key for limit in limits], ["session", "weekly_scoped"])
        self.assertEqual(limits[0].label, "5-hour window")
        self.assertAlmostEqual(limits[0].utilization, 0.37)
        self.assertEqual(limits[0].resets_at.isoformat(), "2026-09-21T18:30:00+00:00")
        self.assertEqual(limits[1].label, "7-day · Opus")
        self.assertIsNone(limits[1].resets_at)

    def test_parses_legacy_blocks(self) -> None:
        limits = parse_limits(
            {
                "five_hour": {"utilization": 12.5, "resets_at": "2026-01-01T00:00:00Z"},
                "seven_day": {"utilization": 64, "resets_at": "bad timestamp"},
            }
        )

        self.assertEqual([limit.key for limit in limits], ["five_hour", "seven_day"])
        self.assertAlmostEqual(limits[0].utilization, 0.125)
        self.assertIsNone(limits[1].resets_at)

    def test_unknown_limit_kind_is_kept(self) -> None:
        [limit] = parse_limits({"limits": [{"kind": "future_quota", "percent": None}]})
        self.assertEqual(limit.label, "Future Quota")
        self.assertEqual(limit.utilization, 0.0)


if __name__ == "__main__":
    unittest.main()
