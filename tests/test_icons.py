from __future__ import annotations

import unittest
from datetime import datetime

import test_support  # noqa: F401
from widget_common.icons import usage_icon
from widget_common.models import Usage, UsageLimit


class TrayIconTests(unittest.TestCase):
    def test_usage_digits_use_only_the_provider_color_at_every_usage_level(self) -> None:
        for color in ("#D97757", "#4CC9F0", "#808080"):
            expected_rgb = tuple(bytes.fromhex(color[1:]))
            for utilization in (0.0, 0.5, 0.95, 1.0):
                with self.subTest(color=color, utilization=utilization):
                    usage = Usage(
                        limits=[
                            UsageLimit(
                                key="primary",
                                label="Primary",
                                short_label="Primary",
                                utilization=utilization,
                                resets_at=None,
                            )
                        ],
                        fetched_at=datetime.now(),
                    )
                    image = usage_icon(usage, color)
                    visible_pixels = [
                        pixel for pixel in image.get_flattened_data() if pixel[3]
                    ]

                    self.assertTrue(visible_pixels)
                    self.assertEqual(
                        {pixel[:3] for pixel in visible_pixels},
                        {expected_rgb},
                    )


if __name__ == "__main__":
    unittest.main()
