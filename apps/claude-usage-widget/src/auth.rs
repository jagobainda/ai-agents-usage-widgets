use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{env, fs, io};

use chrono::Utc;
use serde_json::Value;
use widget_common::{
    ProviderError,
    utils::{ascii_json, home_dir, py_int, truthy},
};

use crate::config::{OAUTH_CLIENT_ID, TOKEN_URL};
use crate::http::{ClaudeRequest, ClaudeResponse, token_request};
use crate::keyring;

const OAUTH: &str = "claudeAiOauth";
const KEYRING_SERVICES: [&str; 3] = ["Claude Code-credentials", "Claude Code", "Claude"];

pub enum AuthError {
    Missing,
    RefreshRejected,
    Other(ProviderError),
}

impl From<ProviderError> for AuthError {
    fn from(error: ProviderError) -> Self {
        Self::Other(error)
    }
}

#[derive(Clone)]
pub struct OAuthToken {
    access_token: Value,
    refresh_token: Value,
    expires_at_ms: i64,
    source_path: Option<PathBuf>,
}

impl OAuthToken {
    pub fn access_token(&self) -> Cow<'_, str> {
        match &self.access_token {
            Value::String(token) => Cow::Borrowed(token),
            Value::Null => Cow::Borrowed("None"),
            Value::Bool(true) => Cow::Borrowed("True"),
            Value::Bool(false) => Cow::Borrowed("False"),
            value => Cow::Owned(value.to_string()),
        }
    }

    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms <= now_ms.saturating_add(60_000)
    }
}

pub struct Credentials {
    paths: [PathBuf; 5],
}

impl Credentials {
    pub fn new() -> Self {
        let home = home_dir();
        let appdata = env::var_os("APPDATA").unwrap_or_default();
        let localappdata = env::var_os("LOCALAPPDATA").unwrap_or_default();
        Self::from_paths(&home, appdata.as_ref(), localappdata.as_ref())
    }

    pub fn from_paths(home: &Path, appdata: &OsStr, localappdata: &OsStr) -> Self {
        Self {
            paths: [
                home.join(".claude").join(".credentials.json"),
                Path::new(appdata).join("Claude").join(".credentials.json"),
                Path::new(appdata)
                    .join("Claude Code")
                    .join(".credentials.json"),
                Path::new(localappdata)
                    .join("Claude")
                    .join(".credentials.json"),
                Path::new(localappdata)
                    .join("Claude Code")
                    .join(".credentials.json"),
            ],
        }
    }

    pub fn load(&self) -> Result<OAuthToken, AuthError> {
        let username = env::var_os("USERNAME").unwrap_or_default();
        self.load_with(&username.to_string_lossy(), |service, user| {
            keyring::password(keyring::read, service, user)
        })
    }

    pub fn load_with(
        &self,
        username: &str,
        password: impl Fn(&str, &str) -> Option<String>,
    ) -> Result<OAuthToken, AuthError> {
        if let Some(path) = self.paths.iter().find(|path| path.is_file()) {
            let contents = fs::read(path).map_err(io_error)?;
            return parse_token(&contents, Some(path.clone())).map_err(Into::into);
        }
        for service in KEYRING_SERVICES {
            for user in ["default", username] {
                if let Some(raw) = password(service, user)
                    && let Ok(token) = parse_token(raw.as_bytes(), None)
                {
                    return Ok(token);
                }
            }
        }
        Err(AuthError::Missing)
    }
}

fn parse_token(bytes: &[u8], source_path: Option<PathBuf>) -> Result<OAuthToken, ProviderError> {
    let data: Value = serde_json::from_slice(bytes)
        .map_err(|_| ProviderError::new("JSONDecodeError: Invalid credentials JSON"))?;
    let oauth = data
        .get(OAUTH)
        .filter(|value| truthy(value))
        .unwrap_or(&data);
    Ok(OAuthToken {
        access_token: required_field(oauth, "accessToken")?.clone(),
        refresh_token: required_field(oauth, "refreshToken")?.clone(),
        expires_at_ms: token_integer(required_field(oauth, "expiresAt")?)?,
        source_path,
    })
}

fn required_field<'a>(object: &'a Value, key: &str) -> Result<&'a Value, ProviderError> {
    object
        .get(key)
        .ok_or_else(|| ProviderError::new(format!("KeyError: '{key}'")))
}

fn token_integer(value: &Value) -> Result<i64, ProviderError> {
    value
        .as_bool()
        .map(i64::from)
        .or_else(|| py_int(Some(value)))
        .ok_or_else(|| ProviderError::new("ValueError: Invalid OAuth expiration"))
}

pub fn refresh_token(
    token: OAuthToken,
    send: &impl Fn(ClaudeRequest) -> Result<ClaudeResponse, ProviderError>,
) -> Result<OAuthToken, AuthError> {
    let refresh = serde_json::to_string(&token.refresh_token)
        .map_err(|_| ProviderError::new("TypeError: Could not encode the OAuth request"))?;
    let body = format!(
        r#"{{"grant_type": "refresh_token", "refresh_token": {}, "client_id": "{OAUTH_CLIENT_ID}"}}"#,
        ascii_json(&refresh)
    );
    let response = send(token_request(body)?)?;
    if response.status == ureq::http::StatusCode::BAD_REQUEST {
        return Err(AuthError::RefreshRejected);
    }
    let payload = response.json(TOKEN_URL)?;
    if !payload.is_object() {
        return Err(ProviderError::new("TypeError: Expected an OAuth response object").into());
    }
    let expires_in = payload
        .get("expires_in")
        .map(token_integer)
        .transpose()?
        .unwrap_or(3600);
    let expires_at_ms = expires_in
        .checked_mul(1000)
        .and_then(|ms| Utc::now().timestamp_millis().checked_add(ms))
        .ok_or_else(|| ProviderError::new("OverflowError: OAuth expiration is out of range"))?;
    let refreshed = OAuthToken {
        access_token: required_field(&payload, "access_token")?.clone(),
        refresh_token: payload
            .get("refresh_token")
            .cloned()
            .unwrap_or(token.refresh_token),
        expires_at_ms,
        source_path: token.source_path,
    };
    save_token(&refreshed)?;
    Ok(refreshed)
}

fn save_token(token: &OAuthToken) -> Result<(), AuthError> {
    let Some(path) = &token.source_path else {
        return Ok(());
    };
    static WRITE: Mutex<()> = Mutex::new(());
    let _write = WRITE.lock().unwrap_or_else(|error| error.into_inner());
    let input = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let mut data = match input {
        Some(Value::Object(data)) => data,
        None => serde_json::Map::new(),
        _ => return Err(ProviderError::new("TypeError: Expected a credentials object").into()),
    };
    if !data.get(OAUTH).is_some_and(truthy) {
        data.insert(OAUTH.to_owned(), Value::Object(serde_json::Map::new()));
    }
    let Some(Value::Object(oauth)) = data.get_mut(OAUTH) else {
        return Err(ProviderError::new("TypeError: Expected an OAuth credentials object").into());
    };
    for (key, value) in [
        ("accessToken", token.access_token.clone()),
        ("refreshToken", token.refresh_token.clone()),
        ("expiresAt", Value::from(token.expires_at_ms)),
    ] {
        oauth.insert(key.to_owned(), value);
    }
    let output = serde_json::to_string_pretty(&data)
        .map_err(|_| ProviderError::new("TypeError: Could not encode the credentials JSON"))?;
    let output = ascii_json(&output);
    #[cfg(windows)]
    let output = output.replace('\n', "\r\n");
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, output).map_err(io_error)?;
    fs::rename(tmp, path).map_err(io_error)?;
    Ok(())
}

fn io_error(error: io::Error) -> AuthError {
    match error.kind() {
        io::ErrorKind::NotFound => AuthError::Missing,
        io::ErrorKind::PermissionDenied => {
            ProviderError::new("PermissionError: Permission denied for Claude Code credentials")
                .into()
        }
        _ => ProviderError::new("OSError: Claude Code credentials file operation failed").into(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use tempfile::tempdir;
    use ureq::http::StatusCode;

    const TOKENS: &str =
        r#"{"accessToken":"fixture","refreshToken":"old-refresh","expiresAt":"300000"}"#;

    #[test]
    fn checks_files_in_order_and_uses_the_first_existing_file() {
        let dir = tempdir().unwrap();
        let appdata = dir.path().join("roaming");
        let localappdata = dir.path().join("local");
        let files =
            Credentials::from_paths(dir.path(), appdata.as_os_str(), localappdata.as_os_str());
        assert_eq!(
            files.paths,
            [
                dir.path().join(".claude/.credentials.json"),
                appdata.join("Claude/.credentials.json"),
                appdata.join("Claude Code/.credentials.json"),
                localappdata.join("Claude/.credentials.json"),
                localappdata.join("Claude Code/.credentials.json"),
            ]
        );
        let home = &files.paths[0];
        let roaming = &files.paths[1];
        for path in [home, roaming] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        fs::write(home, "invalid").unwrap();
        fs::write(roaming, format!(r#"{{"claudeAiOauth":{TOKENS}}}"#)).unwrap();
        let no_keyring = |_: &str, _: &str| panic!("a credentials file exists");
        assert!(matches!(
            files.load_with("test", no_keyring),
            Err(AuthError::Other(_))
        ));
        fs::remove_file(home).unwrap();
        let Ok(token) = files.load_with("test", no_keyring) else {
            panic!("expected credentials from APPDATA");
        };
        assert_eq!(token.source_path.as_deref(), Some(roaming.as_path()));
    }

    #[test]
    fn checks_keyring_pairs_in_order_and_never_persists_the_result() {
        let dir = tempdir().unwrap();
        let files =
            Credentials::from_paths(dir.path(), dir.path().as_os_str(), dir.path().as_os_str());
        let pairs = RefCell::new(Vec::new());
        let token = files.load_with("test", |service, user| {
            pairs
                .borrow_mut()
                .push((service.to_owned(), user.to_owned()));
            match (service, user) {
                ("Claude Code-credentials", "default") => Some("{bad".to_owned()),
                ("Claude", "test") => Some(TOKENS.to_owned()),
                _ => None,
            }
        });
        let Ok(token) = token else {
            panic!("expected keyring credentials");
        };
        let expected: Vec<_> = KEYRING_SERVICES
            .into_iter()
            .flat_map(|service| {
                ["default", "test"].map(|user| (service.to_owned(), user.to_owned()))
            })
            .collect();
        assert_eq!(pairs.into_inner(), expected);
        assert_eq!(token.source_path, None);
        assert!(!token.is_expired(239_999));
        assert!(token.is_expired(240_000));

        let before = Utc::now().timestamp_millis() + 3_600_000;
        let refreshed = refresh_token(token, &|_| {
            Ok(ClaudeResponse {
                status: StatusCode::OK,
                body: br#"{"access_token":"new-access"}"#.to_vec(),
            })
        });
        let Ok(refreshed) = refreshed else {
            panic!("expected the keyring token to refresh");
        };
        let after = Utc::now().timestamp_millis() + 3_600_000;
        assert_eq!(refreshed.refresh_token, "old-refresh");
        assert!((before..=after).contains(&refreshed.expires_at_ms));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn an_explicit_null_refresh_token_replaces_the_previous_value() {
        let token = parse_token(TOKENS.as_bytes(), None).unwrap();
        let Ok(token) = refresh_token(token, &|_| {
            Ok(ClaudeResponse {
                status: StatusCode::OK,
                body: br#"{"access_token":"updated","refresh_token":null}"#.to_vec(),
            })
        }) else {
            panic!("expected the token to refresh");
        };
        assert_eq!(token.refresh_token, Value::Null);
        assert!(matches!(
            refresh_token(token, &|request| {
                let request = request.as_http();
                let body: Value = serde_json::from_str(request.body().as_deref().unwrap()).unwrap();
                assert_eq!(body.get("refresh_token"), Some(&Value::Null));
                Ok(ClaudeResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: Vec::new(),
                })
            }),
            Err(AuthError::RefreshRejected)
        ));
    }

    #[test]
    fn replaces_the_file_with_v1_format_and_preserves_other_fields() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(
            &path,
            r#"{"z":{"last":"Mé🔑","first":1},"claudeAiOauth":{"keep":true,"refreshToken":"old"},"a":[2,3]}"#,
        )
        .unwrap();
        let token = parse_token(TOKENS.as_bytes(), Some(path.clone())).unwrap();
        assert!(save_token(&token).is_ok());
        let expected = r#"{
  "z": {
    "last": "M\u00e9\ud83d\udd11",
    "first": 1
  },
  "claudeAiOauth": {
    "keep": true,
    "refreshToken": "old-refresh",
    "accessToken": "fixture",
    "expiresAt": 300000
  },
  "a": [
    2,
    3
  ]
}"#;
        #[cfg(windows)]
        let expected = expected.replace('\n', "\r\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        assert!(!path.with_extension("json.tmp").exists());
    }
}
