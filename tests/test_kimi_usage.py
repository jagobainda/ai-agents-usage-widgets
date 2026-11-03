from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import requests

import test_support  # noqa: F401
from kimi_widget.api import (
    KimiAPIError,
    KimiPayloadError,
    fetch_usage,
    parse_usage_response,
)
from kimi_widget.auth import (
    KimiCredentialError,
    KimiSettings,
    OAuthToken,
    RefreshTokenError,
    load_settings,
    load_token,
    refresh_token,
)
from kimi_widget.provider import KimiProvider
from widget_common.models import Usage
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

    def raise_for_status(self):
        if self.status_code >= 400:
            raise requests.HTTPError(str(self.status_code))


def make_settings(home: Path) -> KimiSettings:
    return KimiSettings(
        home=home,
        base_url="https://api.kimi.com/coding/v1",
        oauth_host="https://auth.kimi.com",
        credential_name="kimi-code",
    )


def make_token(home: Path, expires_at: int = 4_000_000_000) -> OAuthToken:
    settings = make_settings(home)
    return OAuthToken(
        access_token="access",
        refresh_token="refresh",
        expires_at=expires_at,
        expires_in=3600,
        source_path=settings.token_path,
        usage_url=settings.usage_url,
        token_url=settings.token_url,
    )


class KimiSettingsTests(unittest.TestCase):
    def test_uses_default_managed_credential(self) -> None:
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            settings = load_settings(Path(directory))
        self.assertEqual(settings.token_path.name, "kimi-code.json")
        self.assertEqual(settings.usage_url, "https://api.kimi.com/coding/v1/usages")
        self.assertEqual(settings.token_url, "https://auth.kimi.com/api/oauth/token")

    def test_reads_global_region_and_scoped_credential(self) -> None:
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            root = Path(directory)
            (root / "config.toml").write_text(
                '[providers."managed:kimi-code"]\n'
                'base_url = "https://api.kimi.ai/coding/v1/"\n'
                '[providers."managed:kimi-code".oauth]\n'
                'oauth_host = "https://auth.kimi.ai/"\n',
                encoding="utf-8",
            )
            settings = load_settings(root)
        self.assertEqual(settings.usage_url, "https://api.kimi.ai/coding/v1/usages")
        self.assertEqual(settings.token_url, "https://auth.kimi.ai/api/oauth/token")
        self.assertTrue(settings.token_path.name.startswith("kimi-code-env-"))

    def test_environment_overrides_managed_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "config.toml").write_text(
                '[providers."managed:kimi-code"]\n'
                'base_url = "https://configured.example/v1"\n'
                '[providers."managed:kimi-code".oauth]\n'
                'oauth_host = "https://configured-auth.example"\n',
                encoding="utf-8",
            )
            with patch.dict(
                os.environ,
                {"KIMI_CODE_BASE_URL": "https://env.example/v1/"},
                clear=True,
            ):
                settings = load_settings(root)
        self.assertEqual(settings.usage_url, "https://env.example/v1/usages")
        self.assertEqual(settings.token_url, "https://auth.kimi.com/api/oauth/token")


class KimiAuthTests(unittest.TestCase):
    def test_loads_kimi_oauth_token(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            settings = make_settings(Path(directory))
            settings.token_path.parent.mkdir()
            settings.token_path.write_text(
                json.dumps(
                    {
                        "access_token": "access",
                        "refresh_token": "refresh",
                        "expires_at": 1234,
                        "expires_in": 3600,
                    }
                ),
                encoding="utf-8",
            )
            token = load_token(settings)
        self.assertEqual(token.access_token, "access")
        self.assertEqual(token.expires_at, 1234)

    def test_missing_or_malformed_credentials_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            settings = make_settings(Path(directory))
            with self.assertRaises(KimiCredentialError):
                load_token(settings)
            settings.token_path.parent.mkdir()
            settings.token_path.write_text("{}", encoding="utf-8")
            with self.assertRaises(KimiCredentialError):
                load_token(settings)

    def test_refresh_uses_public_client_and_rotates_tokens(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            token = make_token(Path(directory))
            response = FakeResponse(
                200,
                {"access_token": "new-access", "refresh_token": "new-refresh", "expires_in": 7200},
            )
            with patch("kimi_widget.auth.requests.post", return_value=response) as post, patch(
                "kimi_widget.auth.save_token"
            ) as save:
                refreshed = refresh_token(token)
        self.assertEqual(refreshed.access_token, "new-access")
        self.assertEqual(refreshed.refresh_token, "new-refresh")
        self.assertEqual(post.call_args.kwargs["data"]["grant_type"], "refresh_token")
        save.assert_called_once_with(refreshed)

    def test_invalid_grant_requires_a_new_login(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            token = make_token(Path(directory))
            with patch(
                "kimi_widget.auth.requests.post",
                return_value=FakeResponse(400, {"error": "invalid_grant"}),
            ), self.assertRaises(RefreshTokenError):
                refresh_token(token)


class KimiUsageParsingTests(unittest.TestCase):
    def test_parses_available_windows_in_stable_order(self) -> None:
        usage = parse_usage_response(
            {
                "usages": {
                    "limit_month_total": {
                        "used_ratio": 0.25,
                        "reset_time": "2026-10-01T00:00:00Z",
                    },
                    "limit_month_code": {"used_ratio": 0.1},
                    "limit_7d": {"used_ratio": 1.2, "reset_time": None},
                    "limit_5h": {
                        "used_ratio": 0.125,
                        "reset_time": "2026-09-22T16:00:00Z",
                    },
                }
            }
        )
        self.assertEqual(
            [item.key for item in usage.limits],
            ["kimi:limit_5h", "kimi:limit_7d", "kimi:limit_month_total"],
        )
        self.assertEqual(usage.primary.utilization, 0.125)
        self.assertEqual(usage.limits[1].utilization, 1.0)
        self.assertEqual(usage.limits[1].severity, "reached")
        self.assertEqual(usage.details, ("Monthly · Kimi Code 10% used",))
        self.assertEqual(
            usage.limits[0].resets_at.isoformat(),
            "2026-09-22T16:00:00+00:00",
        )

    def test_omits_missing_and_invalid_windows(self) -> None:
        usage = parse_usage_response(
            {"usages": {"limit_5h": {"used_ratio": "0.4"}, "limit_7d": {"used_ratio": True}}}
        )
        self.assertEqual(len(usage.limits), 1)
        self.assertEqual(usage.primary.utilization, 0.4)
        for payload in (None, {}, {"usages": {}}, {"usages": {"limit_5h": {}}}):
            with self.subTest(payload=payload), self.assertRaises(KimiPayloadError):
                parse_usage_response(payload)


class KimiClientTests(unittest.TestCase):
    def test_fetch_sends_bearer_token_and_timeouts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            token = make_token(Path(directory))
            captured = {}

            def request_get(url, **kwargs):
                captured["url"] = url
                captured.update(kwargs)
                return FakeResponse(200, {"usages": {"limit_5h": {"used_ratio": 0.12}}})

            usage = fetch_usage(token, request_get=request_get)
        self.assertEqual(usage.primary.utilization, 0.12)
        self.assertEqual(captured["timeout"], (5, 15))
        self.assertEqual(captured["headers"]["Authorization"], "Bearer access")

    def test_non_200_and_invalid_json_are_classified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            token = make_token(Path(directory))
            with self.assertRaises(KimiAPIError) as raised:
                fetch_usage(token, request_get=lambda *_args, **_kwargs: FakeResponse(403))
            self.assertEqual(raised.exception.status_code, 403)
            with self.assertRaises(KimiPayloadError):
                fetch_usage(
                    token,
                    request_get=lambda *_args, **_kwargs: FakeResponse(
                        200, json_error=ValueError("bad json")
                    ),
                )


class KimiProviderTests(unittest.TestCase):
    def test_maps_auth_api_and_network_errors_to_safe_messages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            token = make_token(Path(directory))
            cases = (
                (RefreshTokenError("expired"), "Session expired"),
                (KimiAPIError(403), "rejected"),
                (KimiAPIError(404), "does not provide"),
                (KimiAPIError(429), "rate limited"),
                (KimiAPIError(503), "unavailable"),
                (KimiPayloadError("bad"), "incompatible"),
                (requests.Timeout(), "timed out"),
                (requests.ConnectionError(), "could not be reached"),
            )
            for error, expected in cases:
                with self.subTest(error=type(error).__name__):
                    def fail(_token, error=error):
                        raise error

                    provider = KimiProvider(lambda: make_settings(Path(directory)), lambda _settings: token, fail)
                    with self.assertRaises(ProviderError) as raised:
                        provider.fetch_usage()
                    self.assertIn(expected, str(raised.exception))

    def test_missing_credentials_prompt_for_login(self) -> None:
        provider = KimiProvider(
            lambda: (_ for _ in ()).throw(KimiCredentialError("missing")),
            lambda _settings: None,
            lambda _token: None,
        )
        with self.assertRaises(ProviderError) as raised:
            provider.fetch_usage()
        self.assertIn("kimi login", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
