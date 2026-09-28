from __future__ import annotations

import unittest
from datetime import timezone

import test_support  # noqa: F401
from codex_widget.usage import parse_rate_limits_response
from widget_common.utils import datetime_from_unix, duration_label, duration_short_label


class CodexUsageParsingTests(unittest.TestCase):
    def test_primary_and_secondary_windows(self) -> None:
        usage = parse_rate_limits_response(
            {
                "rateLimits": {
                    "limitId": "codex",
                    "planType": "plus",
                    "primary": {
                        "usedPercent": 25,
                        "windowDurationMins": 300,
                        "resetsAt": 1_790_000_000,
                    },
                    "secondary": {
                        "usedPercent": 60,
                        "windowDurationMins": 10_080,
                        "resetsAt": 1_790_500_000,
                    },
                }
            }
        )

        self.assertEqual([item.label for item in usage.limits], ["5-hour window", "7-day window"])
        self.assertEqual(usage.primary.key, "codex:primary")
        self.assertAlmostEqual(usage.limits[1].utilization, 0.60)
        self.assertEqual(usage.details, ("Plan · Plus",))

    def test_multiple_limit_ids_are_not_duplicated_by_legacy_view(self) -> None:
        usage = parse_rate_limits_response(
            {
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {"usedPercent": 1, "windowDurationMins": 300},
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "primary": {"usedPercent": 31, "windowDurationMins": 300},
                        "secondary": {"usedPercent": 42, "windowDurationMins": 10_080},
                    },
                    "codex_other": {
                        "limitId": "codex_other",
                        "limitName": "Fast models",
                        "primary": {"usedPercent": 17, "windowDurationMins": 60},
                    },
                },
            }
        )

        self.assertEqual(len(usage.limits), 3)
        self.assertEqual(usage.limits[0].utilization, 0.31)
        self.assertEqual(usage.limits[2].label, "1-hour window · Fast Models")

    def test_missing_and_unknown_fields_are_tolerated(self) -> None:
        usage = parse_rate_limits_response(
            {
                "result": {
                    "rateLimits": {
                        "newField": [1, 2, 3],
                        "primary": {"usedPercent": "not a number", "future": True},
                        "secondary": None,
                    },
                    "anotherFutureField": True,
                }
            }
        )
        self.assertEqual(len(usage.limits), 1)
        self.assertEqual(usage.limits[0].label, "Usage window")
        self.assertEqual(usage.limits[0].utilization, 0.0)
        self.assertIsNone(usage.limits[0].resets_at)

    def test_future_window_field_is_rendered(self) -> None:
        usage = parse_rate_limits_response(
            {
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {"usedPercent": 10, "windowDurationMins": 300},
                    "burst": {"usedPercent": 20, "windowDurationMins": 30},
                }
            }
        )
        self.assertEqual([item.key for item in usage.limits], ["codex:primary", "codex:burst"])
        self.assertEqual(usage.limits[1].label, "30-minute window")

    def test_unix_timestamp_conversion(self) -> None:
        converted = datetime_from_unix(0)
        self.assertEqual(converted.tzinfo, timezone.utc)
        self.assertEqual(converted.isoformat(), "1970-01-01T00:00:00+00:00")
        self.assertIsNone(datetime_from_unix("invalid"))

    def test_window_labels_are_derived_from_duration(self) -> None:
        cases = [
            (15, "15-minute window", "15m"),
            (60, "1-hour window", "1h"),
            (300, "5-hour window", "5h"),
            (1_440, "1-day window", "1d"),
            (10_080, "7-day window", "7d"),
            (20_160, "2-week window", "2w"),
            (None, "Usage window", "usage"),
        ]
        for minutes, long_label, short_label in cases:
            with self.subTest(minutes=minutes):
                self.assertEqual(duration_label(minutes), long_label)
                self.assertEqual(duration_short_label(minutes), short_label)


if __name__ == "__main__":
    unittest.main()
