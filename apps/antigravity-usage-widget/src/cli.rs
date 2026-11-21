use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use widget_common::process::HiddenProcess;

pub const CHILD_ENV_DISABLE_AUTO_UPDATE: (&str, &str) = ("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(25);
pub const DEFAULT_CLI_TIMEOUT_SECONDS: u64 = 20;
const TERMINATE_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AntigravityCLIError {
    NotFound(String),
    Timeout(String),
    Payload(String),
    Closed(String),
    Cli { message: String, detail: String },
}

impl AntigravityCLIError {
    pub fn cli(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Cli {
            message: message.into(),
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(msg)
            | Self::Timeout(msg)
            | Self::Payload(msg)
            | Self::Closed(msg)
            | Self::Cli { message: msg, .. } => msg,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Cli { detail, .. } => detail,
            _ => "",
        }
    }
}

impl fmt::Display for AntigravityCLIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AntigravityCLIError {}

fn resolve_executable_with(
    which_fn: impl FnOnce(&str) -> Option<PathBuf>,
    local_app_data: Option<OsString>,
) -> Option<String> {
    if let Some(executable) = which_fn("agy") {
        return Some(executable.to_string_lossy().into_owned());
    }
    if let Some(local_app_data) = local_app_data.filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(local_app_data)
            .join("agy")
            .join("bin")
            .join("agy.exe");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

pub fn default_executable() -> Option<String> {
    resolve_executable_with(
        |name| which::which(name).ok(),
        std::env::var_os("LOCALAPPDATA"),
    )
}

pub fn extract_json(stdout: &str) -> Option<Value> {
    let text = stdout.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(text)
        && parsed.is_object()
    {
        return Some(parsed);
    }
    for line in text.lines().rev() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(line)
            && parsed.is_object()
        {
            return Some(parsed);
        }
    }
    None
}

#[derive(Default)]
struct ClientState {
    closed: bool,
    active: Option<Arc<HiddenProcess>>,
}

type ExecutableLookup = Box<dyn Fn() -> Option<String> + Send + Sync>;

pub struct AntigravityCLIClient {
    command: Option<Vec<String>>,
    pub timeout: Duration,
    pub cli_timeout_seconds: u64,
    executable_lookup: ExecutableLookup,
    state: Mutex<ClientState>,
}

impl AntigravityCLIClient {
    pub fn new() -> Self {
        Self {
            command: None,
            timeout: DEFAULT_TIMEOUT,
            cli_timeout_seconds: DEFAULT_CLI_TIMEOUT_SECONDS,
            executable_lookup: Box::new(default_executable),
            state: Mutex::new(ClientState::default()),
        }
    }

    #[cfg(test)]
    pub fn with_command(command: Vec<String>, timeout: Duration) -> Self {
        Self {
            command: Some(command),
            timeout,
            cli_timeout_seconds: DEFAULT_CLI_TIMEOUT_SECONDS,
            executable_lookup: Box::new(default_executable),
            state: Mutex::new(ClientState::default()),
        }
    }

    #[cfg(test)]
    fn with_lookup(lookup: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            command: None,
            timeout: DEFAULT_TIMEOUT,
            cli_timeout_seconds: DEFAULT_CLI_TIMEOUT_SECONDS,
            executable_lookup: Box::new(lookup),
            state: Mutex::new(ClientState::default()),
        }
    }

    pub fn resolve_command(&self) -> Result<Vec<String>, AntigravityCLIError> {
        if let Some(ref command) = self.command {
            return Ok(command.clone());
        }
        let Some(executable) = (self.executable_lookup)() else {
            return Err(AntigravityCLIError::NotFound(
                "Antigravity CLI was not found.".to_string(),
            ));
        };
        let timeout_secs = self.cli_timeout_seconds.max(1);
        Ok(vec![
            executable,
            "--print".to_string(),
            "/usage".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--print-timeout".to_string(),
            format!("{timeout_secs}s"),
        ])
    }

    pub fn fetch_usage_payload(&self) -> Result<Value, AntigravityCLIError> {
        let (process, stdout, stderr) = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return Err(AntigravityCLIError::Closed(
                    "Antigravity CLI client is closed.".to_string(),
                ));
            }
            let command = self.resolve_command()?;
            let Some((program, args)) = command.split_first() else {
                return Err(AntigravityCLIError::NotFound(
                    "Antigravity CLI was not found.".to_string(),
                ));
            };

            let args: Vec<_> = args.iter().map(String::as_str).collect();
            let stdin = File::open("NUL")
                .map_err(|_| AntigravityCLIError::cli("Could not start Antigravity CLI.", ""))?;
            let (process, stdout, stderr) = HiddenProcess::spawn(
                Path::new(program),
                &args,
                stdin,
                Some(CHILD_ENV_DISABLE_AUTO_UPDATE),
            )
            .map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    AntigravityCLIError::NotFound("Antigravity CLI was not found.".to_string())
                } else {
                    AntigravityCLIError::cli("Could not start Antigravity CLI.", "")
                }
            })?;
            let process = Arc::new(process);
            state.active = Some(Arc::clone(&process));
            (process, stdout, stderr)
        };

        let out_rx = read_stream(stdout);
        let err_rx = read_stream(stderr);
        let wait_result = match process.wait(self.timeout) {
            Ok(true) => process.exit_status().map_err(|_| WaitFailure::Io),
            Ok(false) => Err(WaitFailure::Timeout),
            Err(_) => Err(WaitFailure::Io),
        };
        process.terminate_tree();
        if wait_result.is_err() {
            let _ = process.wait(TERMINATE_WAIT);
        }
        {
            let mut state = self.state.lock().unwrap();
            if state
                .active
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &process))
            {
                state.active = None;
            }
        }

        let stdout_res = out_rx
            .recv_timeout(TERMINATE_WAIT)
            .unwrap_or_else(|_| Ok(Vec::new()));
        let stderr_res = err_rx
            .recv_timeout(TERMINATE_WAIT)
            .unwrap_or_else(|_| Ok(Vec::new()));

        let status = match wait_result {
            Ok(status) => status,
            Err(WaitFailure::Timeout) => {
                return Err(AntigravityCLIError::Timeout(
                    "Antigravity CLI timed out.".to_string(),
                ));
            }
            Err(WaitFailure::Io) => {
                return Err(AntigravityCLIError::cli(
                    "Antigravity CLI failed while reading output.",
                    "",
                ));
            }
        };

        let (Ok(stdout_bytes), Ok(stderr_bytes)) = (stdout_res, stderr_res) else {
            return Err(AntigravityCLIError::cli(
                "Antigravity CLI failed while reading output.",
                "",
            ));
        };

        let stdout = String::from_utf8_lossy(&stdout_bytes);
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        let payload = extract_json(&stdout);

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            let mut detail_parts = Vec::new();
            if let Some(ref payload) = payload {
                collect_payload_details(payload, &mut detail_parts);
            }
            let stderr_trimmed = stderr.trim();
            if !stderr_trimmed.is_empty() {
                detail_parts.push(stderr_trimmed.to_string());
            }
            return Err(AntigravityCLIError::cli(
                format!("Antigravity CLI exited with code {code}."),
                detail_parts.join(" "),
            ));
        }

        let Some(payload) = payload else {
            return Err(AntigravityCLIError::Payload(
                "Antigravity CLI returned invalid JSON.".to_string(),
            ));
        };

        if let Some(status_str) = payload.get("status").and_then(Value::as_str)
            && !status_str.eq_ignore_ascii_case("SUCCESS")
        {
            let mut detail_parts = Vec::new();
            collect_payload_details(&payload, &mut detail_parts);
            return Err(AntigravityCLIError::cli(
                format!("Antigravity CLI returned status {status_str}."),
                detail_parts.join(" "),
            ));
        }

        Ok(payload)
    }

    pub fn close(&self) {
        let active = {
            let mut state = self.state.lock().unwrap();
            if state.closed {
                return;
            }
            state.closed = true;
            state.active.take()
        };
        if let Some(active) = active {
            active.terminate_tree();
        }
    }
}

enum WaitFailure {
    Timeout,
    Io,
}

fn read_stream(mut stream: impl Read + Send + 'static) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stream.read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    receive
}

fn collect_payload_details(payload: &Value, out: &mut Vec<String>) {
    if let Some(obj) = payload.as_object() {
        for key in ["error", "message", "response"] {
            if let Some(val) = obj.get(key).and_then(Value::as_str) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    fn fake_agy_command(mode: &str) -> Vec<String> {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fake_agy.cmd");
        vec![script.to_string_lossy().into_owned(), mode.to_string()]
    }

    #[test]
    fn test_fetches_json_from_subprocess() {
        let client =
            AntigravityCLIClient::with_command(fake_agy_command("sample"), Duration::from_secs(5));
        let payload = client.fetch_usage_payload().unwrap();
        assert_eq!(payload["status"], "SUCCESS");
    }

    #[test]
    fn test_extracts_json_after_non_json_log_lines() {
        let client = AntigravityCLIClient::with_command(
            fake_agy_command("noisy-sample"),
            Duration::from_secs(5),
        );
        let payload = client.fetch_usage_payload().unwrap();
        assert_eq!(payload["command"]["name"], "usage");
    }

    #[test]
    fn test_disables_agy_auto_update_in_child_process() {
        let client =
            AntigravityCLIClient::with_command(fake_agy_command("env"), Duration::from_secs(5));
        let payload = client.fetch_usage_payload().unwrap();
        assert_eq!(payload["value"], "true");
    }

    #[test]
    fn test_missing_executable_raises_not_found() {
        let client = AntigravityCLIClient::with_lookup(|| resolve_executable_with(|_| None, None));
        let err = client.fetch_usage_payload().unwrap_err();
        assert!(matches!(err, AntigravityCLIError::NotFound(_)));
    }

    #[test]
    fn test_falls_back_to_localappdata_agy_path() {
        let temp = tempfile::tempdir().unwrap();
        let agy_path = temp.path().join("agy").join("bin").join("agy.exe");
        fs::create_dir_all(agy_path.parent().unwrap()).unwrap();
        fs::write(&agy_path, "").unwrap();

        let dir_os = temp.path().as_os_str().to_os_string();
        let client = AntigravityCLIClient::with_lookup(move || {
            resolve_executable_with(|_| None, Some(dir_os.clone()))
        });
        let resolved = client.resolve_command().unwrap();

        assert_eq!(resolved[0], agy_path.to_string_lossy());
        assert!(resolved.iter().any(|arg| arg == "/usage"));
        assert!(resolved.iter().any(|arg| arg == "json"));
        assert_eq!(
            resolved,
            vec![
                agy_path.to_string_lossy().into_owned(),
                "--print".to_string(),
                "/usage".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--print-timeout".to_string(),
                "20s".to_string(),
            ]
        );
    }

    #[test]
    fn test_timeout_kills_subprocess_and_raises() {
        let client = AntigravityCLIClient::with_command(
            fake_agy_command("sleep"),
            Duration::from_millis(150),
        );
        let err = client.fetch_usage_payload().unwrap_err();
        assert!(matches!(err, AntigravityCLIError::Timeout(_)));
    }

    #[test]
    fn test_non_zero_exit_and_error_status_raise_cli_error() {
        let client = AntigravityCLIClient::with_command(
            fake_agy_command("auth-error"),
            Duration::from_secs(5),
        );
        let err = client.fetch_usage_payload().unwrap_err();
        assert!(matches!(err, AntigravityCLIError::Cli { .. }));
        assert!(err.detail().contains("Authentication required"));

        let status_client = AntigravityCLIClient::with_command(
            fake_agy_command("status-error"),
            Duration::from_secs(5),
        );
        let status_err = status_client.fetch_usage_payload().unwrap_err();
        assert!(matches!(status_err, AntigravityCLIError::Cli { .. }));
        assert!(status_err.detail().contains("rate limit"));
    }

    #[test]
    fn test_orphan_child_does_not_block_stdout_and_is_cleaned_up() {
        let start = Instant::now();
        let client = AntigravityCLIClient::with_command(
            fake_agy_command("orphan-child"),
            Duration::from_secs(5),
        );
        let payload = client.fetch_usage_payload().unwrap();
        assert_eq!(payload["status"], "SUCCESS");
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
