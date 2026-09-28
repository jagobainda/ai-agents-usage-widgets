from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import mock_open, patch

import requests

import test_support  # noqa: F401
from opencode_widget.api import (
    OpenCodeAPIError,
    OpenCodePayloadError,
    fetch_usage,
    parse_usage_response,
)
from opencode_widget.auth import OpenCodeCredentialError, load_api_key
from opencode_widget.provider import OpenCodeProvider
from widget_common.provider import ProviderError


class FakeResponse:
    def __init__(self, status_code: int, payload=None, json_error: Exception | None = None):
        self.status_code = status_code
        self._payload = payload
        self._json_error = json_error

    def json(self):
        if self._json_error is not None:
            raise self._json_error
        return self._payload


class OpenCodeAuthTests(unittest.TestCase):
    def test_loads_only_opencode_go_api_credential(self) -> None:
        contents = (
            '{"opencode":{"type":"api","key":"zen-secret"},'
            '"opencode-go":{"type":"api","key":" go-secret "}}'
        )
        with patch.object(Path, "open", mock_open(read_data=contents)):
            self.assertEqual(load_api_key(Path("auth.json")), "go-secret")

    def test_missing_malformed_and_non_api_credentials_are_rejected(self) -> None:
        cases = (
            ("missing", None),
            ("malformed", "{"),
            ("wrong-provider", '{"opencode":{"type":"api","key":"x"}}'),
            ("oauth", '{"opencode-go":{"type":"oauth","key":"x"}}'),
            ("empty", '{"opencode-go":{"type":"api","key":" "}}'),
        )
        for name, contents in cases:
            with self.subTest(case=name):
                opened = (
                    patch.object(Path, "open", side_effect=FileNotFoundError())
                    if contents is None
                    else patch.object(Path, "open", mock_open(read_data=contents))
                )
                with opened, self.assertRaises(OpenCodeCredentialError):
                    load_api_key(Path("auth.json"))


class OpenCodeUsageParsingTests(unittest.TestCase):
    def test_parses_all_windows_in_stable_order(self) -> None:
        usage = parse_usage_response(
            {
                "usage": {
                    "monthly": {
                        "status": "ok",
                        "percent": 6,
                        "resetsAt": "2026-10-01T00:00:00.000Z",
                    },
                    "rolling": {
                        "status": "ok",
                        "percent": 3,
                        "resetsAt": "2026-09-22T16:00:00.000Z",
                    },
                    "weekly": {
                        "status": "rate-limited",
                        "percent": 100,
                        "resetsAt": None,
                    },
                }
            }
        )

        self.assertEqual(
            [item.key for item in usage.limits],
            ["opencode-go:rolling", "opencode-go:weekly", "opencode-go:monthly"],
        )
        self.assertEqual(usage.primary.key, "opencode-go:rolling")
        self.assertEqual(usage.limits[1].severity, "reached")
        self.assertEqual(usage.details, ("Plan · OpenCode Go",))
        self.assertEqual(
            usage.limits[0].resets_at.isoformat(),
            "2026-09-22T16:00:00+00:00",
        )
        self.assertIsNone(usage.limits[1].resets_at)

    def test_whole_percent_values_are_not_treated_as_fractions(self) -> None:
        usage = parse_usage_response(
            {
                "usage": {
                    "rolling": {"percent": 0},
                    "weekly": {"percent": 1},
                    "monthly": {"percent": 100},
                }
            }
        )
        self.assertEqual(
            [item.utilization for item in usage.limits],
            [0.0, 0.01, 1.0],
        )

    def test_percentages_are_clamped_and_unknown_fields_ignored(self) -> None:
        usage = parse_usage_response(
            {
                "usage": {
                    "rolling": {"percent": -3, "future": True},
                    "weekly": {"percent": 140},
                    "monthly": {"percent": "invalid"},
                    "futureWindow": {"percent": 50},
                },
                "newTopLevel": True,
            }
        )
        self.assertEqual([item.utilization for item in usage.limits], [0.0, 1.0])

    def test_no_usable_windows_is_an_incompatible_payload(self) -> None:
        for payload in (None, {}, {"usage": {}}, {"usage": {"rolling": {"percent": True}}}):
            with self.subTest(payload=payload):
                with self.assertRaises(OpenCodePayloadError):
                    parse_usage_response(payload)


class OpenCodeClientTests(unittest.TestCase):
    def test_fetch_sends_bearer_key_and_timeouts(self) -> None:
        captured = {}

        def request_get(url, **kwargs):
            captured["url"] = url
            captured.update(kwargs)
            return FakeResponse(200, {"usage": {"rolling": {"percent": 12}}})

        usage = fetch_usage("secret", request_get=request_get)
        self.assertEqual(usage.primary.utilization, 0.12)
        self.assertEqual(captured["timeout"], (5, 15))
        self.assertEqual(captured["headers"]["Authorization"], "Bearer secret")

    def test_non_200_and_invalid_json_are_classified(self) -> None:
        with self.assertRaises(OpenCodeAPIError) as raised:
            fetch_usage("secret", request_get=lambda *_args, **_kwargs: FakeResponse(403))
        self.assertEqual(raised.exception.status_code, 403)

        with self.assertRaises(OpenCodePayloadError):
            fetch_usage(
                "secret",
                request_get=lambda *_args, **_kwargs: FakeResponse(
                    200, json_error=ValueError("bad json")
                ),
            )


class OpenCodeProviderTests(unittest.TestCase):
    def test_maps_auth_and_endpoint_errors_to_safe_messages(self) -> None:
        cases = (
            (OpenCodeCredentialError("missing"), "not connected"),
            (OpenCodeAPIError(401), "rejected"),
            (OpenCodeAPIError(403), "subscription"),
            (OpenCodeAPIError(429), "rate limited"),
            (OpenCodeAPIError(503), "unavailable"),
            (OpenCodePayloadError("bad"), "incompatible"),
            (requests.Timeout(), "timed out"),
            (requests.ConnectionError(), "could not be reached"),
        )
        for error, expected in cases:
            with self.subTest(error=type(error).__name__):
                def fail(_key, error=error):
                    raise error

                provider = OpenCodeProvider(lambda: "secret", fail)
                if isinstance(error, OpenCodeCredentialError):
                    provider = OpenCodeProvider(
                        lambda error=error: (_ for _ in ()).throw(error),
                        lambda _key: None,
                    )
                with self.assertRaises(ProviderError) as raised:
                    provider.fetch_usage()
                self.assertIn(expected, str(raised.exception))


if __name__ == "__main__":
    unittest.main()
