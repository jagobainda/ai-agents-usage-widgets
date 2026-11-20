from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import test_support  # noqa: F401
from antigravity_widget.branding import antigravity_logo
from antigravity_widget.cli import (
    AntigravityCLIClient,
    AntigravityCLIError,
    AntigravityNotFoundError,
    AntigravityPayloadError,
    AntigravityTimeoutError,
)
from antigravity_widget.config import WIDGET_CONFIG
from antigravity_widget.provider import AntigravityProvider
from antigravity_widget.usage import AntigravityEmptyUsageError, parse_usage_response
from widget_common.provider import ProviderError
from widget_common.state import AppState


def _sample_payload() -> dict:
    return {
        "conversation_id": "",
        "status": "SUCCESS",
        "duration_seconds": 0,
        "num_turns": 0,
        "usage": {"total_tokens": 0},
        "command": {
            "name": "usage",
            "data": {
                "groups": [
                    {
                        "name": "Gemini Models",
                        "buckets": [
                            {
                                "id": "gemini-weekly",
                                "name": "Weekly Limit Remaining",
                                "window": "weekly",
                                "remaining_fraction": 0.9925,
                                "reset_time": "2026-11-27T14:32:20Z",
                            },
                            {
                                "id": "gemini-5h",
                                "name": "Five Hour Limit Remaining",
                                "window": "5h",
                                "remaining_fraction": 0.9737,
                                "reset_time": "2026-11-20T19:32:20Z",
                            },
                        ],
                    },
                    {
                        "name": "Claude and GPT models",
                        "buckets": [
                            {
                                "id": "3p-weekly",
                                "name": "Weekly Limit Remaining",
                                "window": "weekly",
                                "remaining_fraction": 1,
                                "reset_time": "2026-11-27T14:57:01Z",
                            },
                            {
                                "id": "3p-5h",
                                "name": "Five Hour Limit Remaining",
                                "window": "5h",
                                "remaining_fraction": 0.4,
                                "reset_time": "2026-11-20T19:57:01Z",
                            },
                        ],
                    },
                    {
                        "name": "Gemini 4 Argon",
                        "buckets": [
                            {
                                "id": "gemini-argon-wtu-weekly",
                                "name": "Gemini 4 Argon Weekly Limit",
                                "window": "weekly",
                                "remaining_fraction": 1,
                                "reset_time": "2026-11-27T14:57:01Z",
                            }
                        ],
                    },
                ]
            },
        },
    }


class FakeIcon:
    def __init__(self) -> None:
        self.icon = None
        self.title = ""

    def stop(self) -> None:
        return None


class AntigravityUsageParsingTests(unittest.TestCase):
    def test_parses_multi_group_usage_and_orders_short_windows_first(self) -> None:
        usage = parse_usage_response(_sample_payload())

        self.assertEqual(
            [item.key for item in usage.limits],
            [
                "antigravity:gemini-5h",
                "antigravity:gemini-weekly",
                "antigravity:3p-5h",
                "antigravity:3p-weekly",
                "antigravity:gemini-argon-wtu-weekly",
            ],
        )
        self.assertEqual(
            [item.label for item in usage.limits],
            [
                "5-hour window · Gemini",
                "Weekly window · Gemini",
                "5-hour window · Claude and GPT",
                "Weekly window · Claude and GPT",
                "Weekly window · Gemini 4 Argon",
            ],
        )
        self.assertEqual(usage.primary.key, "antigravity:gemini-5h")
        self.assertAlmostEqual(usage.limits[0].utilization, 0.0263, places=4)
        self.assertAlmostEqual(usage.limits[1].utilization, 0.0075, places=4)
        self.assertAlmostEqual(usage.limits[2].utilization, 0.6, places=4)
        self.assertEqual(usage.limits[3].utilization, 0.0)
        self.assertEqual(
            usage.limits[0].resets_at.isoformat(),
            "2026-11-20T19:32:20+00:00",
        )

    def test_single_group_omits_group_suffix_and_falls_back_primary_to_first(self) -> None:
        usage = parse_usage_response(
            {
                "status": "SUCCESS",
                "command": {
                    "name": "usage",
                    "data": {
                        "groups": [
                            {
                                "name": "Gemini Models",
                                "buckets": [
                                    {
                                        "id": "gemini-weekly",
                                        "window": "weekly",
                                        "remaining_fraction": 0.25,
                                        "reset_time": "2026-11-27T14:32:20Z",
                                    }
                                ],
                            }
                        ]
                    },
                },
            }
        )
        self.assertEqual(len(usage.limits), 1)
        self.assertEqual(usage.limits[0].label, "Weekly window")
        self.assertEqual(usage.limits[0].short_label, "7d")
        self.assertEqual(usage.primary.key, "antigravity:gemini-weekly")
        self.assertAlmostEqual(usage.primary.utilization, 0.75)

    def test_omitted_remaining_fraction_with_reset_time_means_exhausted(self) -> None:
        usage = parse_usage_response(
            {
                "status": "SUCCESS",
                "command": {
                    "name": "usage",
                    "data": {
                        "groups": [
                            {
                                "name": "Gemini Models",
                                "buckets": [
                                    {
                                        "id": "gemini-5h",
                                        "window": "5h",
                                        "reset_time": "2026-11-20T19:32:20Z",
                                    }
                                ],
                            }
                        ]
                    },
                },
            }
        )
        self.assertEqual(usage.primary.utilization, 1.0)
        self.assertEqual(usage.primary.severity, "reached")

    def test_rejects_incompatible_and_empty_payloads(self) -> None:
        for bad in (
            None,
            {},
            {"command": "usage"},
            {"command": {"name": "credits", "data": {"groups": []}}},
            {"command": {"name": "usage", "data": {}}},
        ):
            with self.subTest(bad=bad), self.assertRaises(AntigravityPayloadError):
                parse_usage_response(bad)

        for empty in (
            {"command": {"name": "usage", "data": {"groups": []}}},
            {"command": {"name": "usage", "data": {"groups": [{"name": "G", "buckets": [{"remaining_fraction": True}]}]}}},
        ):
            with self.subTest(empty=empty), self.assertRaises(AntigravityEmptyUsageError):
                parse_usage_response(empty)


class AntigravityCLIClientTests(unittest.TestCase):
    def test_fetches_json_from_subprocess(self) -> None:
        payload = _sample_payload()
        client = AntigravityCLIClient(
            command=[sys.executable, "-c", f"import json; print(json.dumps({payload!r}))"],
            timeout=5.0,
        )
        self.assertEqual(client.fetch_usage_payload()["status"], "SUCCESS")

    def test_extracts_json_after_non_json_log_lines(self) -> None:
        payload = _sample_payload()
        script = f"import json; print('I1120 startup'); print(json.dumps({payload!r}))"
        client = AntigravityCLIClient(command=[sys.executable, "-c", script], timeout=5.0)
        self.assertEqual(client.fetch_usage_payload()["command"]["name"], "usage")

    def test_missing_executable_raises_not_found(self) -> None:
        with patch("antigravity_widget.cli.shutil.which", return_value=None), patch.dict(
            os.environ, {}, clear=True
        ):
            client = AntigravityCLIClient()
            with self.assertRaises(AntigravityNotFoundError):
                client.fetch_usage_payload()

    def test_falls_back_to_localappdata_agy_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            agy_path = Path(directory) / "agy" / "bin" / "agy.exe"
            agy_path.parent.mkdir(parents=True)
            agy_path.write_text("", encoding="utf-8")
            with patch("antigravity_widget.cli.shutil.which", return_value=None), patch.dict(
                os.environ, {"LOCALAPPDATA": directory}, clear=True
            ):
                client = AntigravityCLIClient()
                resolved = client._resolve_command()
        self.assertEqual(resolved[0], str(agy_path))
        self.assertIn("/usage", resolved)
        self.assertIn("json", resolved)

    def test_timeout_kills_subprocess_and_raises(self) -> None:
        client = AntigravityCLIClient(
            command=[sys.executable, "-c", "import time; time.sleep(10)"],
            timeout=0.15,
        )
        with self.assertRaises(AntigravityTimeoutError):
            client.fetch_usage_payload()

    def test_non_zero_exit_and_error_status_raise_cli_error(self) -> None:
        client = AntigravityCLIClient(
            command=[sys.executable, "-c", "import sys; sys.stderr.write('Authentication required'); sys.exit(1)"],
            timeout=5.0,
        )
        with self.assertRaises(AntigravityCLIError) as raised:
            client.fetch_usage_payload()
        self.assertIn("Authentication required", raised.exception.detail)

        status_client = AntigravityCLIClient(
            command=[
                sys.executable,
                "-c",
                "import json; print(json.dumps({'status': 'ERROR', 'error': 'rate limit exceeded'}))",
            ],
            timeout=5.0,
        )
        with self.assertRaises(AntigravityCLIError) as raised_status:
            status_client.fetch_usage_payload()
        self.assertIn("rate limit", raised_status.exception.detail)


class AntigravityProviderTests(unittest.TestCase):
    def test_maps_cli_errors_to_safe_messages(self) -> None:
        cases = (
            (AntigravityNotFoundError("missing"), "Install 'agy'"),
            (AntigravityTimeoutError("slow"), "timed out"),
            (AntigravityPayloadError("bad json"), "incompatible"),
            (AntigravityCLIError("exit 1", "Authentication required: token ABC123"), "not signed in"),
            (AntigravityCLIError("exit 1", "HTTP 429 RESOURCE_EXHAUSTED"), "rate limited"),
            (AntigravityCLIError("exit 1", "flag provided but not defined: -output-format"), "agy update"),
            (AntigravityCLIError("exit 1", "dial tcp: lookup failed"), "could not be reached"),
            (AntigravityCLIError("exit 1", "internal secret stack"), "could not return usage data"),
        )
        for error, expected in cases:
            with self.subTest(expected=expected):
                class FailingClient:
                    def fetch_usage_payload(self):
                        raise error

                    def close(self) -> None:
                        return None

                provider = AntigravityProvider(client=FailingClient())
                with self.assertRaises(ProviderError) as raised:
                    provider.fetch_usage()
                self.assertIn(expected, str(raised.exception))
                self.assertNotIn("ABC123", str(raised.exception))
                self.assertNotIn("internal secret", str(raised.exception))


class AntigravityBrandingAndTooltipTests(unittest.TestCase):
    def test_logo_renders_official_multicolor_gradient(self) -> None:
        image = antigravity_logo(64)
        self.assertEqual(image.size, (64, 64))
        visible = [pixel for pixel in image.get_flattened_data() if pixel[3] > 200]
        self.assertTrue(visible)
        self.assertTrue(any(r > 180 and g < 120 and b < 120 for r, g, b, _ in visible))
        self.assertTrue(any(r > 190 and g > 170 and b < 110 for r, g, b, _ in visible))
        self.assertTrue(any(g > 150 and r < 110 and b < 140 for r, g, b, _ in visible))
        self.assertTrue(any(b > 200 and r < 140 for r, g, b, _ in visible))

    def test_tray_tooltip_stays_within_windows_limit_for_multi_group_usage(self) -> None:
        class StaticProvider:
            def fetch_usage(self):
                return parse_usage_response(_sample_payload())

            def close(self) -> None:
                return None

        state = AppState(StaticProvider(), WIDGET_CONFIG)
        state.icon = FakeIcon()
        state._publish_success(parse_usage_response(_sample_payload()))

        self.assertLessEqual(len(state.icon.title), 127)
        self.assertTrue(state.icon.title.startswith("Antigravity\n"))
        self.assertIn("reset:", state.icon.title)

        state._publish_error("x" * 200)
        self.assertLessEqual(len(state.icon.title), 127)


if __name__ == "__main__":
    unittest.main()
