"""Kimi Code OAuth configuration, token loading and refresh."""

from __future__ import annotations

import hashlib
import json
import os
import time
from dataclasses import dataclass
from pathlib import Path

import requests

from .config import DEFAULT_BASE_URL, DEFAULT_OAUTH_HOST, OAUTH_CLIENT_ID

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib


class KimiCredentialError(RuntimeError):
    pass


class RefreshTokenError(RuntimeError):
    pass


@dataclass(frozen=True)
class KimiSettings:
    home: Path
    base_url: str
    oauth_host: str
    credential_name: str

    @property
    def token_path(self) -> Path:
        return self.home / "credentials" / f"{self.credential_name}.json"

    @property
    def usage_url(self) -> str:
        return f"{self.base_url}/usages"

    @property
    def token_url(self) -> str:
        return f"{self.oauth_host}/api/oauth/token"


@dataclass(frozen=True)
class OAuthToken:
    access_token: str
    refresh_token: str
    expires_at: int
    expires_in: int
    source_path: Path
    usage_url: str
    token_url: str
    scope: str = ""
    token_type: str = "Bearer"

    @property
    def is_expired(self) -> bool:
        if self.expires_at == 0:
            return False
        threshold = max(300, self.expires_in * 0.5)
        return self.expires_at - time.time() < threshold


def _credential_name(base_url: str, oauth_host: str) -> str:
    if oauth_host == DEFAULT_OAUTH_HOST and base_url == DEFAULT_BASE_URL:
        return "kimi-code"
    digest = hashlib.sha256(
        json.dumps(
            {"oauthHost": oauth_host, "baseUrl": base_url},
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()[:16]
    return f"kimi-code-env-{digest}"


def load_settings(home: Path | None = None) -> KimiSettings:
    root = home or Path(os.environ.get("KIMI_CODE_HOME") or Path.home() / ".kimi-code")
    try:
        with (root / "config.toml").open("rb") as handle:
            config = tomllib.load(handle)
    except FileNotFoundError:
        config = {}
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise KimiCredentialError("Kimi Code configuration could not be read.") from exc

    providers = config.get("providers")
    provider = providers.get("managed:kimi-code") if isinstance(providers, dict) else None
    if not isinstance(provider, dict):
        provider = {}
    oauth = provider.get("oauth")
    if not isinstance(oauth, dict):
        oauth = {}

    env_base_url = os.environ.get("KIMI_CODE_BASE_URL")
    env_oauth_host = os.environ.get("KIMI_CODE_OAUTH_HOST") or os.environ.get("KIMI_OAUTH_HOST")
    configured_base = provider.get("base_url")
    configured_host = None if env_base_url or env_oauth_host else oauth.get("oauth_host")
    base_url = (env_base_url or configured_base or DEFAULT_BASE_URL).rstrip("/")
    oauth_host = (env_oauth_host or configured_host or DEFAULT_OAUTH_HOST).rstrip("/")
    if not env_base_url and not env_oauth_host and oauth.get("storage", "file") != "file":
        raise KimiCredentialError("Kimi Code credential storage is not supported.")
    return KimiSettings(root, base_url, oauth_host, _credential_name(base_url, oauth_host))


def _number(payload: dict, key: str) -> int:
    value = payload.get(key)
    return int(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else 0


def _text(payload: dict, key: str, default: str) -> str:
    value = payload.get(key)
    return value if isinstance(value, str) else default


def load_token(settings: KimiSettings) -> OAuthToken:
    try:
        with settings.token_path.open("r", encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise KimiCredentialError("Kimi Code credentials could not be read.") from exc
    if not isinstance(payload, dict):
        raise KimiCredentialError("Kimi Code credentials are malformed.")
    access_token = payload.get("access_token")
    refresh_token = payload.get("refresh_token")
    if not isinstance(access_token, str) or not access_token:
        raise KimiCredentialError("Kimi Code access token is missing.")
    if not isinstance(refresh_token, str):
        raise KimiCredentialError("Kimi Code refresh token is malformed.")
    return OAuthToken(
        access_token=access_token,
        refresh_token=refresh_token,
        expires_at=_number(payload, "expires_at"),
        expires_in=_number(payload, "expires_in"),
        source_path=settings.token_path,
        usage_url=settings.usage_url,
        token_url=settings.token_url,
        scope=_text(payload, "scope", ""),
        token_type=_text(payload, "token_type", "Bearer"),
    )


def save_token(token: OAuthToken) -> None:
    payload = {
        "access_token": token.access_token,
        "refresh_token": token.refresh_token,
        "expires_at": token.expires_at,
        "scope": token.scope,
        "token_type": token.token_type,
        "expires_in": token.expires_in,
    }
    temporary = token.source_path.with_suffix(".tmp")
    with temporary.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")
    os.replace(temporary, token.source_path)


def refresh_token(token: OAuthToken) -> OAuthToken:
    response = requests.post(
        token.token_url,
        data={
            "client_id": OAUTH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": token.refresh_token,
        },
        headers={"Accept": "application/json"},
        timeout=(5, 15),
    )
    try:
        payload = response.json()
    except ValueError:
        payload = {}
    if not isinstance(payload, dict):
        payload = {}
    if response.status_code != 200:
        if response.status_code in (401, 403) or payload.get("error") == "invalid_grant":
            raise RefreshTokenError("Kimi Code rejected the saved session.")
        response.raise_for_status()
    access_token = payload.get("access_token")
    refresh = payload.get("refresh_token")
    expires_in = _number(payload, "expires_in")
    if not isinstance(access_token, str) or not access_token or not isinstance(refresh, str) or not refresh or expires_in <= 0:
        raise RefreshTokenError("Kimi Code returned an invalid refreshed session.")
    refreshed = OAuthToken(
        access_token=access_token,
        refresh_token=refresh,
        expires_at=int(time.time()) + expires_in,
        expires_in=expires_in,
        source_path=token.source_path,
        usage_url=token.usage_url,
        token_url=token.token_url,
        scope=_text(payload, "scope", token.scope),
        token_type=_text(payload, "token_type", token.token_type),
    )
    save_token(refreshed)
    return refreshed
