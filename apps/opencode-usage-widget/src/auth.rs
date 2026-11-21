use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use widget_common::utils::home_dir;

use crate::error::OpenCodeError;

pub fn default_auth_path() -> PathBuf {
    home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json")
}

pub fn load_api_key(path: &Path) -> Result<String, OpenCodeError> {
    let contents = fs::read_to_string(path)
        .map_err(|_| OpenCodeError::Credential("OpenCode credentials could not be read."))?;
    let payload: Value = serde_json::from_str(&contents)
        .map_err(|_| OpenCodeError::Credential("OpenCode credentials could not be read."))?;
    let object = payload.as_object().ok_or(OpenCodeError::Credential(
        "OpenCode credentials are malformed.",
    ))?;
    let credential = object
        .get("opencode-go")
        .and_then(Value::as_object)
        .filter(|credential| credential.get("type").and_then(Value::as_str) == Some("api"))
        .ok_or(OpenCodeError::Credential("OpenCode Go is not connected."))?;
    credential
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .ok_or(OpenCodeError::Credential("OpenCode Go API key is missing."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_loads_only_opencode_go_api_credential() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        fs::write(
            &path,
            r#"{"opencode":{"type":"api","key":"zen-secret"},"opencode-go":{"type":"api","key":" go-secret "}}"#,
        )
        .unwrap();

        assert_eq!(load_api_key(&path).unwrap(), "go-secret");
    }

    #[test]
    fn test_missing_malformed_and_non_api_credentials_are_rejected() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auth.json");
        let cases = [
            None,
            Some("{"),
            Some(r#"{"opencode":{"type":"api","key":"x"}}"#),
            Some(r#"{"opencode-go":{"type":"oauth","key":"x"}}"#),
            Some(r#"{"opencode-go":{"type":"api","key":" "}}"#),
        ];

        for contents in cases {
            if let Some(contents) = contents {
                fs::write(&path, contents).unwrap();
            }
            assert!(matches!(
                load_api_key(&path),
                Err(OpenCodeError::Credential(_))
            ));
        }
    }
}
