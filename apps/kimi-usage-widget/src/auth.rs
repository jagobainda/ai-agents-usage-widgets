use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{DEFAULT_BASE_URL, DEFAULT_OAUTH_HOST, OAUTH_CLIENT_ID};
use widget_common::utils::{ascii_json, home_dir};

use crate::error::KimiError;
use widget_common::http::{DEFAULT_TIMEOUTS, Response};

#[derive(Clone)]
pub struct KimiSettings {
    pub home: PathBuf,
    pub base_url: String,
    pub oauth_host: String,
    pub credential_name: String,
}

impl KimiSettings {
    pub fn token_path(&self) -> PathBuf {
        self.home
            .join("credentials")
            .join(format!("{}.json", self.credential_name))
    }

    pub fn usage_url(&self) -> String {
        format!("{}/usages", self.base_url)
    }

    pub fn token_url(&self) -> String {
        format!("{}/api/oauth/token", self.oauth_host)
    }
}

#[derive(Clone)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub expires_in: i64,
    pub source_path: PathBuf,
    pub usage_url: String,
    pub token_url: String,
    pub scope: String,
    pub token_type: String,
}

impl OAuthToken {
    pub(crate) fn is_expired_at(&self, now: f64) -> bool {
        if self.expires_at == 0 {
            return false;
        }
        let threshold = (self.expires_in as f64 * 0.5).max(300.0);
        (self.expires_at as f64) - now < threshold
    }
}

pub fn load_settings(home: Option<&Path>) -> Result<KimiSettings, KimiError> {
    load_settings_with_env(home, |key| env::var(key).ok())
}

fn load_settings_with_env<F>(home: Option<&Path>, env_value: F) -> Result<KimiSettings, KimiError>
where
    F: Fn(&str) -> Option<String>,
{
    let root = home.map(Path::to_path_buf).unwrap_or_else(|| {
        nonempty(env_value("KIMI_CODE_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".kimi-code"))
    });
    let config: toml::Table = match fs::read_to_string(root.join("config.toml")) {
        Ok(contents) => toml::from_str(&contents)
            .map_err(|_| KimiError::Credential("Kimi Code configuration could not be read."))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(_) => {
            return Err(KimiError::Credential(
                "Kimi Code configuration could not be read.",
            ));
        }
    };
    let provider = config
        .get("providers")
        .and_then(|providers| providers.get("managed:kimi-code"));
    let oauth = provider.and_then(|provider| provider.get("oauth"));

    let env_base_url = nonempty(env_value("KIMI_CODE_BASE_URL"));
    let env_oauth_host = nonempty(env_value("KIMI_CODE_OAUTH_HOST"))
        .or_else(|| nonempty(env_value("KIMI_OAUTH_HOST")));
    let has_environment_override = env_base_url.is_some() || env_oauth_host.is_some();
    let base_url = match env_base_url.as_deref() {
        Some(url) => url,
        None => configured_url(
            provider.and_then(|provider| provider.get("base_url")),
            DEFAULT_BASE_URL,
        )?,
    };
    let oauth_host = match env_oauth_host.as_deref() {
        Some(host) => host,
        None if has_environment_override => DEFAULT_OAUTH_HOST,
        None => configured_url(
            oauth.and_then(|oauth| oauth.get("oauth_host")),
            DEFAULT_OAUTH_HOST,
        )?,
    };
    if !has_environment_override
        && oauth
            .and_then(|oauth| oauth.get("storage"))
            .is_some_and(|storage| storage.as_str() != Some("file"))
    {
        return Err(KimiError::Credential(
            "Kimi Code credential storage is not supported.",
        ));
    }
    let base_url = base_url.trim_end_matches('/').to_owned();
    let oauth_host = oauth_host.trim_end_matches('/').to_owned();
    let credential_name = credential_name(&base_url, &oauth_host);
    Ok(KimiSettings {
        home: root,
        base_url,
        oauth_host,
        credential_name,
    })
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn configured_url<'a>(
    value: Option<&'a toml::Value>,
    default: &'a str,
) -> Result<&'a str, KimiError> {
    use toml::Value::{Array, Boolean, Datetime, Float, Integer, String, Table};

    let type_name = match value {
        None | Some(Boolean(false) | Integer(0)) => return Ok(default),
        Some(String(url)) => return Ok(if url.is_empty() { default } else { url }),
        Some(Float(number)) if *number == 0.0 => return Ok(default),
        Some(Array(items)) if items.is_empty() => return Ok(default),
        Some(Table(entries)) if entries.is_empty() => return Ok(default),
        Some(Boolean(_)) => "bool",
        Some(Integer(_)) => "int",
        Some(Float(_)) => "float",
        Some(Array(_)) => "list",
        Some(Table(_)) => "dict",
        Some(Datetime(value)) if value.date.is_none() => "datetime.time",
        Some(Datetime(value)) if value.time.is_none() => "datetime.date",
        Some(Datetime(_)) => "datetime.datetime",
    };
    Err(KimiError::InvalidUrlType(type_name))
}

fn credential_name(base_url: &str, oauth_host: &str) -> String {
    if oauth_host == DEFAULT_OAUTH_HOST && base_url == DEFAULT_BASE_URL {
        return "kimi-code".to_string();
    }
    let input = format!(
        "{{\"oauthHost\":{},\"baseUrl\":{}}}",
        json_string_ascii(oauth_host),
        json_string_ascii(base_url)
    );
    let digest = Sha256::digest(input.as_bytes());
    let prefix: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("kimi-code-env-{prefix}")
}

fn json_string_ascii(value: &str) -> String {
    ascii_json(&serde_json::to_string(value).unwrap())
}

pub fn load_token(settings: &KimiSettings) -> Result<OAuthToken, KimiError> {
    let path = settings.token_path();
    let contents = fs::read_to_string(&path)
        .map_err(|_| KimiError::Credential("Kimi Code credentials could not be read."))?;
    let payload: Value = serde_json::from_str(&contents)
        .map_err(|_| KimiError::Credential("Kimi Code credentials could not be read."))?;
    let object = payload.as_object().ok_or(KimiError::Credential(
        "Kimi Code credentials are malformed.",
    ))?;
    let access_token = object
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or(KimiError::Credential("Kimi Code access token is missing."))?;
    let refresh_token =
        object
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or(KimiError::Credential(
                "Kimi Code refresh token is malformed.",
            ))?;
    Ok(OAuthToken {
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        expires_at: number(object, "expires_at"),
        expires_in: number(object, "expires_in"),
        source_path: path,
        usage_url: settings.usage_url(),
        token_url: settings.token_url(),
        scope: text(object, "scope", ""),
        token_type: text(object, "token_type", "Bearer"),
    })
}

fn number(payload: &Map<String, Value>, key: &str) -> i64 {
    match payload.get(key) {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value.trunc() as i64))
            .unwrap_or(0),
        _ => 0,
    }
}

fn text(payload: &Map<String, Value>, key: &str, default: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

#[derive(Serialize)]
struct SavedToken<'a> {
    access_token: &'a str,
    refresh_token: &'a str,
    expires_at: i64,
    scope: &'a str,
    token_type: &'a str,
    expires_in: i64,
}

pub fn save_token(token: &OAuthToken) -> Result<(), KimiError> {
    let payload = SavedToken {
        access_token: &token.access_token,
        refresh_token: &token.refresh_token,
        expires_at: token.expires_at,
        scope: &token.scope,
        token_type: &token.token_type,
        expires_in: token.expires_in,
    };
    let mut contents = ascii_json(&serde_json::to_string_pretty(&payload).unwrap());
    contents.push('\n');
    let temporary = token.source_path.with_extension("tmp");
    fs::write(&temporary, contents).map_err(KimiError::Io)?;
    fs::rename(temporary, &token.source_path).map_err(KimiError::Io)
}

pub fn refresh_token_with<F>(
    token: &OAuthToken,
    now: i64,
    mut post: F,
) -> Result<OAuthToken, KimiError>
where
    F: FnMut(&str, &[(&str, &str)], &[(&str, &str)], (u64, u64)) -> Result<Response, KimiError>,
{
    let form = [
        ("client_id", OAUTH_CLIENT_ID),
        ("grant_type", "refresh_token"),
        ("refresh_token", token.refresh_token.as_str()),
    ];
    let headers = [("Accept", "application/json")];
    let response = post(&token.token_url, &form, &headers, DEFAULT_TIMEOUTS)?;
    let payload = serde_json::from_str::<Value>(&response.body)
        .ok()
        .and_then(|payload| payload.as_object().cloned())
        .unwrap_or_default();
    if response.status_code != 200 {
        if matches!(response.status_code, 401 | 403)
            || payload.get("error").and_then(Value::as_str) == Some("invalid_grant")
        {
            return Err(KimiError::Refresh("Kimi Code rejected the saved session."));
        }
        return Err(KimiError::Transport);
    }
    let access_token = payload.get("access_token").and_then(Value::as_str);
    let refresh = payload.get("refresh_token").and_then(Value::as_str);
    let expires_in = number(&payload, "expires_in");
    if access_token.is_none_or(str::is_empty)
        || refresh.is_none_or(str::is_empty)
        || expires_in <= 0
    {
        return Err(KimiError::Refresh(
            "Kimi Code returned an invalid refreshed session.",
        ));
    }
    let refreshed = OAuthToken {
        access_token: access_token.unwrap().to_string(),
        refresh_token: refresh.unwrap().to_string(),
        expires_at: now + expires_in,
        expires_in,
        source_path: token.source_path.clone(),
        usage_url: token.usage_url.clone(),
        token_url: token.token_url.clone(),
        scope: text(&payload, "scope", &token.scope),
        token_type: text(&payload, "token_type", &token.token_type),
    };
    save_token(&refreshed)?;
    Ok(refreshed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_settings(home: &Path) -> KimiSettings {
        KimiSettings {
            home: home.to_path_buf(),
            base_url: DEFAULT_BASE_URL.to_string(),
            oauth_host: DEFAULT_OAUTH_HOST.to_string(),
            credential_name: "kimi-code".to_string(),
        }
    }

    pub fn make_token(home: &Path) -> OAuthToken {
        let settings = make_settings(home);
        OAuthToken {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 4_000_000_000,
            expires_in: 3600,
            source_path: settings.token_path(),
            usage_url: settings.usage_url(),
            token_url: settings.token_url(),
            scope: String::new(),
            token_type: "Bearer".to_string(),
        }
    }

    #[test]
    fn test_uses_default_managed_credential() {
        let directory = tempdir().unwrap();
        let settings = load_settings_with_env(Some(directory.path()), |_| None).unwrap();
        assert_eq!(settings.token_path().file_name().unwrap(), "kimi-code.json");
        assert_eq!(
            settings.usage_url(),
            "https://api.kimi.com/coding/v1/usages"
        );
        assert_eq!(
            settings.token_url(),
            "https://auth.kimi.com/api/oauth/token"
        );
    }

    #[test]
    fn test_reads_global_region_and_scoped_credential() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("config.toml"),
            "[providers.\"managed:kimi-code\"]\nbase_url = \"https://api.kimi.ai/coding/v1/\"\n[providers.\"managed:kimi-code\".oauth]\noauth_host = \"https://auth.kimi.ai/\"\n",
        )
        .unwrap();
        let settings = load_settings_with_env(Some(directory.path()), |_| None).unwrap();
        assert_eq!(settings.usage_url(), "https://api.kimi.ai/coding/v1/usages");
        assert_eq!(settings.token_url(), "https://auth.kimi.ai/api/oauth/token");
        assert_eq!(
            settings.token_path().file_name().unwrap(),
            "kimi-code-env-0e4f99c69cc27850.json"
        );
    }

    #[test]
    fn reads_managed_settings_from_inline_tables() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            r#"providers = { 'managed:kimi-code' = { base_url = '', oauth = { oauth_host = 'https://auth.kimi.ai', storage = 'file' } } }"#,
        )
        .unwrap();
        let settings = load_settings_with_env(Some(directory.path()), |_| None).unwrap();
        assert_eq!(settings.base_url, DEFAULT_BASE_URL);
        assert_eq!(settings.oauth_host, "https://auth.kimi.ai");

        fs::write(
            &config_path,
            r#"providers = { 'managed:kimi-code' = { base_url = true } }"#,
        )
        .unwrap();
        assert!(matches!(
            load_settings_with_env(Some(directory.path()), |_| None),
            Err(KimiError::InvalidUrlType("bool"))
        ));
    }

    #[test]
    fn test_environment_overrides_managed_configuration() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("config.toml"),
            "[providers.\"managed:kimi-code\"]\nbase_url = \"https://configured.example/v1\"\n[providers.\"managed:kimi-code\".oauth]\noauth_host = \"https://configured-auth.example\"\n",
        )
        .unwrap();
        let settings = load_settings_with_env(Some(directory.path()), |key| {
            (key == "KIMI_CODE_BASE_URL").then(|| "https://env.example/v1/".to_string())
        })
        .unwrap();
        assert_eq!(settings.usage_url(), "https://env.example/v1/usages");
        assert_eq!(
            settings.token_url(),
            "https://auth.kimi.com/api/oauth/token"
        );
    }

    #[test]
    fn test_loads_kimi_oauth_token() {
        let directory = tempdir().unwrap();
        let settings = make_settings(directory.path());
        fs::create_dir(settings.token_path().parent().unwrap()).unwrap();
        fs::write(
            settings.token_path(),
            r#"{"access_token":"access","refresh_token":"refresh","expires_at":1234,"expires_in":3600}"#,
        )
        .unwrap();
        let token = load_token(&settings).unwrap();
        assert_eq!(token.access_token, "access");
        assert_eq!(token.expires_at, 1234);
    }

    #[test]
    fn test_missing_or_malformed_credentials_are_rejected() {
        let directory = tempdir().unwrap();
        let settings = make_settings(directory.path());
        assert!(matches!(
            load_token(&settings),
            Err(KimiError::Credential(_))
        ));
        fs::create_dir(settings.token_path().parent().unwrap()).unwrap();
        fs::write(settings.token_path(), "{}").unwrap();
        assert!(matches!(
            load_token(&settings),
            Err(KimiError::Credential(_))
        ));
    }

    #[test]
    fn test_refresh_uses_public_client_and_rotates_tokens() {
        let directory = tempdir().unwrap();
        let token = make_token(directory.path());
        fs::create_dir(token.source_path.parent().unwrap()).unwrap();
        let mut captured_form = None;
        let refreshed = refresh_token_with(&token, 1000, |url, form, headers, timeouts| {
            assert_eq!(url, token.token_url);
            assert_eq!(headers, [("Accept", "application/json")]);
            assert_eq!(timeouts, (5, 15));
            captured_form = Some(
                form.iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect::<Vec<_>>(),
            );
            Ok(Response {
                status_code: 200,
                body: r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":7200}"#.to_string(),
            })
        })
        .unwrap();

        assert_eq!(refreshed.access_token, "new-access");
        assert_eq!(refreshed.refresh_token, "new-refresh");
        assert_eq!(refreshed.expires_at, 8200);
        let form = captured_form.unwrap();
        assert!(form.contains(&("client_id".to_string(), OAUTH_CLIENT_ID.to_string())));
        assert!(form.contains(&("grant_type".to_string(), "refresh_token".to_string())));
        assert!(form.contains(&("refresh_token".to_string(), "refresh".to_string())));
        assert_eq!(
            fs::read_to_string(&refreshed.source_path).unwrap(),
            "{\n  \"access_token\": \"new-access\",\n  \"refresh_token\": \"new-refresh\",\n  \"expires_at\": 8200,\n  \"scope\": \"\",\n  \"token_type\": \"Bearer\",\n  \"expires_in\": 7200\n}\n"
        );
        assert!(!refreshed.source_path.with_extension("tmp").exists());
    }

    #[test]
    fn test_invalid_grant_requires_a_new_login() {
        let directory = tempdir().unwrap();
        let token = make_token(directory.path());
        let result = refresh_token_with(&token, 1000, |_, _, _, _| {
            Ok(Response {
                status_code: 400,
                body: r#"{"error":"invalid_grant"}"#.to_string(),
            })
        });
        assert!(matches!(result, Err(KimiError::Refresh(_))));
    }

    #[test]
    fn test_expiration_uses_half_lifetime_with_minimum() {
        let mut token = make_token(Path::new("unused"));
        token.expires_in = 600;
        token.expires_at = 1300;
        assert!(!token.is_expired_at(999.0));
        assert!(token.is_expired_at(1001.0));
        token.expires_in = 7200;
        token.expires_at = 4600;
        assert!(!token.is_expired_at(999.0));
        assert!(token.is_expired_at(1001.0));
        token.expires_at = 0;
        assert!(!token.is_expired_at(f64::MAX));
    }
}
