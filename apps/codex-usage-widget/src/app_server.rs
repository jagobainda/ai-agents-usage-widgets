use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, BufReader, PipeWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Number, Value, json};

use crate::config::{CLIENT_NAME, REQUEST_RETRIES, REQUEST_TIMEOUT, RETRY_BACKOFF, WIDGET_CONFIG};
use crate::process::find_codex;
use crate::value_text::display_value;
use widget_common::process::{HiddenProcess, wait};
use widget_common::utils::truthy;

const NOTIFICATION_CAPACITY: usize = 32;

const NOT_RUNNING: AppServerError = AppServerError::Transport("Codex App Server is not running");
const LOST_CONNECTION: AppServerError =
    AppServerError::Transport("Codex App Server connection was lost");
const EXITED: AppServerError = AppServerError::Transport("Codex App Server exited unexpectedly");
const STOPPED: AppServerError = AppServerError::Transport("Codex App Server stopped");
const START_FAILED: AppServerError = AppServerError::Transport("Could not start Codex App Server");

#[derive(Clone, Debug, PartialEq)]
pub enum AppServerError {
    NotFound,
    Transport(&'static str),
    Timeout(&'static str),
    Closed,
    Rpc {
        code: Option<Number>,
        unsupported: bool,
        sign_in: bool,
    },
}

impl fmt::Display for AppServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("Codex CLI was not found in PATH"),
            Self::Transport(message) => formatter.write_str(message),
            Self::Timeout(method) => write!(
                formatter,
                "Timed out waiting for App Server method {method}"
            ),
            Self::Closed => formatter.write_str("App Server client is closed"),
            Self::Rpc {
                code: Some(code), ..
            } => write!(formatter, "App Server error {code}"),
            Self::Rpc { code: None, .. } => formatter.write_str("App Server error None"),
        }
    }
}

impl std::error::Error for AppServerError {}

type Response = Result<Value, AppServerError>;
type SharedRouter = Arc<Mutex<MessageRouter>>;

struct MessageRouter {
    pending: HashMap<u64, Sender<Response>>,
    notifications: SyncSender<Value>,
    failure: Option<AppServerError>,
}

impl MessageRouter {
    fn new(notifications: SyncSender<Value>) -> Self {
        Self {
            pending: HashMap::new(),
            notifications,
            failure: None,
        }
    }

    fn register(&mut self, id: u64) -> Receiver<Response> {
        let (send, receive) = mpsc::channel();
        if let Some(failure) = &self.failure {
            let _ = send.send(Err(failure.clone()));
        } else {
            self.pending.insert(id, send);
        }
        receive
    }

    fn dispatch(&mut self, message: Value) {
        if let Some(id) = message.get("id") {
            if let Some(id) = response_id(id)
                && let Some(queue) = self.pending.remove(&id)
            {
                let _ = queue.send(Ok(message));
            }
        } else if message.get("method").and_then(Value::as_str).is_some() {
            let _ = self.notifications.try_send(message);
        }
    }

    fn fail_all(&mut self, error: AppServerError) {
        for (_, queue) in self.pending.drain() {
            let _ = queue.send(Err(error.clone()));
        }
        self.failure = Some(error);
    }
}

fn response_id(value: &Value) -> Option<u64> {
    match value {
        Value::Bool(value) => Some(u64::from(*value)),
        Value::Number(number) => number.as_u64().or_else(|| {
            let number = number.as_f64()?;
            (number >= 0.0 && number.fract() == 0.0 && number < u64::MAX as f64)
                .then_some(number as u64)
        }),
        _ => None,
    }
}

fn read_messages(mut stream: impl BufRead, router: &Mutex<MessageRouter>) {
    let mut line = Vec::new();
    while let Ok(bytes) = stream.fill_buf() {
        if bytes.is_empty() {
            break;
        }
        if let Some(end) = bytes.iter().position(|byte| matches!(byte, b'\r' | b'\n')) {
            line.extend_from_slice(&bytes[..end]);
            dispatch_line(&line, router);
            line.clear();
            stream.consume(end + 1);
        } else {
            line.extend_from_slice(bytes);
            let consumed = bytes.len();
            stream.consume(consumed);
        }
    }
    dispatch_line(&line, router);
    router.lock().unwrap().fail_all(EXITED);
}

fn dispatch_line(line: &[u8], router: &Mutex<MessageRouter>) {
    if let Ok(message) = serde_json::from_str(String::from_utf8_lossy(line).trim()) {
        router.lock().unwrap().dispatch(message);
    }
}

#[derive(Serialize)]
struct Request<'a> {
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

fn write_message(mut stream: impl Write, request: &Request<'_>) -> Result<(), AppServerError> {
    let mut line = serde_json::to_vec(request).map_err(|_| LOST_CONNECTION)?;
    line.extend(b"\r\n");
    stream
        .write_all(&line)
        .and_then(|()| stream.flush())
        .map_err(|_| LOST_CONNECTION)
}

struct Server {
    process: HiddenProcess,
    stdin: PipeWriter,
    router: SharedRouter,
    readers: [JoinHandle<()>; 2],
}

impl Server {
    fn spawn(path: PathBuf, notifications: SyncSender<Value>) -> Result<Self, AppServerError> {
        let (child_stdin, stdin) = io::pipe().map_err(|_| START_FAILED)?;
        let (process, stdout, mut stderr) =
            HiddenProcess::spawn(&path, &["app-server"], child_stdin, None).map_err(|error| {
                match error.kind() {
                    io::ErrorKind::NotFound => AppServerError::NotFound,
                    _ => START_FAILED,
                }
            })?;
        let router = Arc::new(Mutex::new(MessageRouter::new(notifications)));
        let stdout_router = Arc::clone(&router);
        let stdout_reader = thread::Builder::new()
            .name("codex-app-server-stdout".into())
            .spawn(move || read_messages(BufReader::new(stdout), &stdout_router))
            .map_err(|_| START_FAILED)?;
        let stderr_reader = thread::Builder::new()
            .name("codex-app-server-stderr".into())
            .spawn(move || {
                let _ = io::copy(&mut stderr, &mut io::sink());
            })
            .map_err(|_| START_FAILED)?;
        Ok(Self {
            process,
            stdin,
            router,
            readers: [stdout_reader, stderr_reader],
        })
    }

    fn send(&mut self, request: &Request<'_>) -> Result<(), AppServerError> {
        if !self.process.is_running() {
            return Err(NOT_RUNNING);
        }
        write_message(&mut self.stdin, request)
    }

    fn begin_request(
        &mut self,
        id: u64,
        method: &'static str,
        params: Option<Value>,
    ) -> Result<Receiver<Response>, AppServerError> {
        let response = self.router.lock().unwrap().register(id);
        if let Err(error) = self.send(&Request {
            method,
            id: Some(id),
            params,
        }) {
            self.router.lock().unwrap().pending.remove(&id);
            return Err(error);
        }
        Ok(response)
    }

    fn stop(self) {
        self.router.lock().unwrap().fail_all(STOPPED);
        drop(self.stdin);
        let _ = self.process.wait(Duration::from_millis(500));
        self.process.terminate_tree();
        let _ = self.process.wait(Duration::from_secs(2));
        for reader in self.readers {
            if wait(&reader, Duration::from_millis(500)).unwrap_or(false) {
                let _ = reader.join();
            }
        }
    }
}

fn wait_for_result(
    response: Receiver<Response>,
    router: &SharedRouter,
    id: u64,
    method: &'static str,
    timeout: Duration,
) -> Response {
    let message = match response.recv_timeout(timeout) {
        Ok(message) => message?,
        Err(RecvTimeoutError::Timeout) => {
            router.lock().unwrap().pending.remove(&id);
            return Err(AppServerError::Timeout(method));
        }
        Err(RecvTimeoutError::Disconnected) => return Err(EXITED),
    };
    if let Some(error) = message
        .get("error")
        .filter(|error| error.as_object().is_some())
    {
        return Err(rpc_error(error));
    }
    let Value::Object(mut fields) = message else {
        unreachable!("the router only delivers objects");
    };
    fields
        .shift_remove("result")
        .ok_or(AppServerError::Transport(
            "App Server returned an invalid response",
        ))
}

fn rpc_error(error: &Value) -> AppServerError {
    let code = match error.get("code") {
        Some(Value::Number(code)) => Some(code.clone()),
        _ => None,
    };
    let message = error
        .get("message")
        .filter(|message| truthy(message))
        .map_or_else(String::new, |message| display_value(message).to_lowercase());
    let unsupported = code.as_ref().and_then(Number::as_f64) == Some(-32601.0)
        || message.contains("method not found");
    let sign_in = [
        "unauthorized",
        "not logged",
        "login",
        "sign in",
        "auth",
        "credential",
    ]
    .into_iter()
    .any(|term| message.contains(term));
    AppServerError::Rpc {
        code,
        unsupported,
        sign_in,
    }
}

#[derive(Default)]
struct State {
    server: Option<Server>,
    closed: bool,
}

pub struct CodexAppServerClient {
    state: Mutex<State>,
    next_id: AtomicU64,
    timeout: Duration,
    retries: u32,
    backoff: Duration,
    resolve: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
    notifications: SyncSender<Value>,
    _notification_receiver: Mutex<Receiver<Value>>,
}

impl CodexAppServerClient {
    pub fn new() -> Self {
        Self::with_options(
            REQUEST_TIMEOUT,
            REQUEST_RETRIES,
            RETRY_BACKOFF,
            find_codex,
            thread::sleep,
        )
    }

    fn with_options(
        timeout: Duration,
        retries: u32,
        backoff: Duration,
        resolve: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
        sleep: impl Fn(Duration) + Send + Sync + 'static,
    ) -> Self {
        let (notifications, receive) = mpsc::sync_channel(NOTIFICATION_CAPACITY);
        Self {
            state: Mutex::new(State::default()),
            next_id: AtomicU64::new(1),
            timeout,
            retries,
            backoff,
            resolve: Box::new(resolve),
            sleep: Box::new(sleep),
            notifications,
            _notification_receiver: Mutex::new(receive),
        }
    }

    fn start(&self, state: &mut State) -> Result<(), AppServerError> {
        if state.closed {
            return Err(AppServerError::Closed);
        }
        if state
            .server
            .as_ref()
            .is_some_and(|server| server.process.is_running())
        {
            return Ok(());
        }
        if let Some(server) = state.server.take() {
            server.stop();
        }
        let executable = (self.resolve)().ok_or(AppServerError::NotFound)?;
        let mut server = Server::spawn(executable, self.notifications.clone())?;
        if let Err(error) = self.initialize(&mut server) {
            server.stop();
            return Err(error);
        }
        state.server = Some(server);
        Ok(())
    }

    fn initialize(&self, server: &mut Server) -> Result<(), AppServerError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let method = "initialize";
        let response = server.begin_request(
            id,
            method,
            Some(json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "title": WIDGET_CONFIG.display_name,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {},
            })),
        )?;
        wait_for_result(response, &server.router, id, method, self.timeout)?;
        server.send(&Request {
            method: "initialized",
            id: None,
            params: Some(json!({})),
        })
    }

    pub fn request(
        &self,
        method: &'static str,
        params: Option<Value>,
        retries: Option<u32>,
    ) -> Response {
        let retries = retries.unwrap_or(self.retries);
        for attempt in 0..=retries {
            let mut origin = None;
            match self.request_once(method, params.clone(), &mut origin) {
                Err(error @ (AppServerError::Timeout(_) | AppServerError::Transport(_))) => {
                    if let Some(origin) = origin {
                        let mut state = self.state.lock().unwrap();
                        if state
                            .server
                            .as_ref()
                            .is_some_and(|server| Arc::ptr_eq(&server.router, &origin))
                            && let Some(server) = state.server.take()
                        {
                            server.stop();
                        }
                    }
                    if attempt == retries {
                        return Err(error);
                    }
                    (self.sleep)(self.backoff.saturating_mul(2_u32.saturating_pow(attempt)));
                }
                result => return result,
            }
        }
        unreachable!("there is always an initial attempt")
    }

    fn request_once(
        &self,
        method: &'static str,
        params: Option<Value>,
        origin: &mut Option<SharedRouter>,
    ) -> Response {
        let mut state = self.state.lock().unwrap();
        self.start(&mut state)?;
        let server = state.server.as_mut().ok_or(NOT_RUNNING)?;
        let router = Arc::clone(&server.router);
        *origin = Some(Arc::clone(&router));
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let response = server.begin_request(id, method, params)?;
        drop(state);
        wait_for_result(response, &router, id, method, self.timeout)
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        if let Some(server) = state.server.take() {
            server.stop();
        }
    }
}

impl Drop for CodexAppServerClient {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use std::path::Path;
    use tempfile::TempDir;
    use widget_common::UsageProvider;
    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};

    use crate::provider::CodexProvider;

    const NOTICE: &str = "account/rateLimits/updated";

    fn message(raw: &str) -> Value {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn correlates_out_of_order_responses_by_id() {
        let (notifications, _) = mpsc::sync_channel(NOTIFICATION_CAPACITY);
        let mut router = MessageRouter::new(notifications);
        let first = router.register(1);
        let second = router.register(2);
        router.dispatch(message(r#"{"id":2,"result":"second"}"#));
        router.dispatch(message(r#"{"id":1,"result":"first"}"#));
        let first = first.try_recv().unwrap().unwrap();
        let second = second.try_recv().unwrap().unwrap();
        assert_eq!(first.get("result").and_then(Value::as_str), Some("first"));
        assert_eq!(second.get("result").and_then(Value::as_str), Some("second"));
    }

    #[test]
    fn keeps_interleaved_notification() {
        let (notifications, receive) = mpsc::sync_channel(NOTIFICATION_CAPACITY);
        let router = Mutex::new(MessageRouter::new(notifications));
        let response = router.lock().unwrap().register(9);
        let input = b"\r\ninvalid JSON\r\n{\"method\":\"account/rateLimits/updated\",\"params\":{}}\r{\"id\":9,\"result\":{\"ok\":true}}";
        read_messages(Cursor::new(input), &router);
        let notice = receive.try_recv().unwrap();
        assert_eq!(notice.get("method").and_then(Value::as_str), Some(NOTICE));
        assert!(
            notice
                .get("params")
                .unwrap()
                .as_object()
                .unwrap()
                .is_empty()
        );
        let response = response.try_recv().unwrap().unwrap();
        assert!(
            response
                .get("result")
                .unwrap()
                .get("ok")
                .unwrap()
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn full_notification_queue_does_not_block_responses() {
        let (notifications, receive) = mpsc::sync_channel(NOTIFICATION_CAPACITY);
        let mut router = MessageRouter::new(notifications);
        let response = router.register(1);
        for _ in 0..=NOTIFICATION_CAPACITY {
            router.dispatch(json!({"method": NOTICE}));
        }
        router.dispatch(json!({"id": 1, "result": "ok"}));
        assert_eq!(response.try_recv().unwrap().unwrap()["result"], "ok");
        assert_eq!(receive.try_iter().count(), NOTIFICATION_CAPACITY);
    }

    #[test]
    fn missing_codex_executable_is_reported() {
        let client = CodexAppServerClient::with_options(
            Duration::from_millis(100),
            0,
            Duration::ZERO,
            || None,
            thread::sleep,
        );
        assert!(matches!(
            client.request("account/rateLimits/read", None, None),
            Err(AppServerError::NotFound)
        ));
        client.close();
    }

    #[test]
    fn notification_can_arrive_before_response() {
        let fake = FakeServer::new("interleave");
        let client = fake.client(Duration::from_secs(1), 0, Duration::ZERO, thread::sleep);
        let response = client.request("test/read", None, None).unwrap();
        assert!(response.get("ok").unwrap().as_bool().unwrap());
        let notification = client
            ._notification_receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            notification.get("method").and_then(Value::as_str),
            Some(NOTICE)
        );
        client.close();
    }

    #[test]
    fn request_timeout_stops_process() {
        let fake = FakeServer::new("hang");
        let client = fake.client(Duration::from_millis(200), 0, Duration::ZERO, thread::sleep);
        assert!(matches!(
            client.request("test/hang", None, None),
            Err(AppServerError::Timeout("test/hang"))
        ));
        assert!(client.state.lock().unwrap().server.is_none());
        let pid = fs::read_to_string(fake.marker())
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // SAFETY: The PID was recorded by this test's fake server.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if process.is_null() {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_INVALID_PARAMETER as i32)
            );
        } else {
            // SAFETY: OpenProcess returned a new owned handle.
            let process = unsafe { OwnedHandle::from_raw_handle(process) };
            assert!(wait(&process, Duration::from_secs(1)).unwrap());
        }
        client.close();
    }

    #[test]
    fn dead_process_is_restarted_with_backoff() {
        let fake = FakeServer::new("restart");
        let sleeps = Arc::new(Mutex::new(Vec::new()));
        let recorded_sleeps = Arc::clone(&sleeps);
        let client = fake.client(
            Duration::from_secs(1),
            1,
            Duration::from_millis(10),
            move |delay| {
                recorded_sleeps.lock().unwrap().push(delay);
            },
        );
        let response = client.request("test/restart", None, None).unwrap();
        assert!(response.get("restarted").unwrap().as_bool().unwrap());
        assert_eq!(
            fs::read_to_string(fake.marker()).unwrap(),
            "first process exited"
        );
        assert_eq!(*sleeps.lock().unwrap(), [Duration::from_millis(10)]);
        client.close();
    }

    #[test]
    fn close_terminates_subprocess() {
        let fake = FakeServer::new("normal");
        let client = fake.client(Duration::from_secs(1), 0, Duration::ZERO, thread::sleep);
        let process = {
            let mut state = client.state.lock().unwrap();
            client.start(&mut state).unwrap();
            let process = &state.server.as_ref().unwrap().process;
            assert!(process.is_running());
            std::os::windows::io::AsHandle::as_handle(process)
                .try_clone_to_owned()
                .unwrap()
        };
        client.close();
        assert!(client.state.lock().unwrap().server.is_none());
        assert!(wait(&process, Duration::ZERO).unwrap());
        assert_eq!(fs::read_to_string(fake.marker()).unwrap(), "stdin closed");
        client.close();
    }

    #[test]
    fn provider_checks_account_without_refreshing_when_no_windows_are_available() {
        for (mode, expected) in [
            (
                "unsigned",
                "Codex is not signed in. Run 'codex login' and try again.",
            ),
            ("signed", "Codex App Server returned no usage windows."),
            ("account_rpc", "Codex App Server returned no usage windows."),
        ] {
            let fake = FakeServer::new(mode);
            let provider = CodexProvider::with_client(fake.client(
                Duration::from_secs(1),
                0,
                Duration::ZERO,
                thread::sleep,
            ));
            assert_eq!(provider.fetch_usage().unwrap_err().to_string(), expected);
            provider.close();
        }
    }

    struct FakeServer {
        directory: TempDir,
    }

    impl FakeServer {
        fn new(mode: &str) -> Self {
            let directory = tempfile::Builder::new()
                .prefix("codex test & %CODEX_WIDGET_TEST_UNSET% ")
                .tempdir()
                .unwrap();
            let fake = Self { directory };
            let exe = std::env::current_exe().unwrap();
            let marker = fake.marker();
            fs::write(fake.launcher(), format!(
                "@echo off\r\nif not \"%~1\"==\"app-server\" exit /b 8\r\nset \"CODEX_WIDGET_TEST_MODE={mode}\"\r\nset \"CODEX_WIDGET_TEST_MARKER={}\"\r\n\"{}\" --exact app_server::tests::fake_server --ignored --nocapture\r\nexit /b %errorlevel%\r\n",
                marker.to_string_lossy().replace('%', "%%"),
                exe.to_string_lossy().replace('%', "%%")
            )).unwrap();
            fake
        }

        fn launcher(&self) -> PathBuf {
            self.directory.path().join("codex.cmd")
        }

        fn marker(&self) -> PathBuf {
            self.directory.path().join("marker")
        }

        fn client(
            &self,
            timeout: Duration,
            retries: u32,
            backoff: Duration,
            sleep: impl Fn(Duration) + Send + Sync + 'static,
        ) -> CodexAppServerClient {
            let launcher = self.launcher();
            CodexAppServerClient::with_options(
                timeout,
                retries,
                backoff,
                move || Some(launcher.clone()),
                sleep,
            )
        }
    }

    #[test]
    #[ignore = "fixture launched only by the app-server tests"]
    fn fake_server() {
        let mode = std::env::var("CODEX_WIDGET_TEST_MODE").unwrap();
        let marker = std::env::var_os("CODEX_WIDGET_TEST_MARKER").unwrap();
        let marker = Path::new(&marker);
        println!();
        let mut responses = io::stdout().lock();
        let mut id = None;
        let mut initialized = false;
        for raw in io::stdin().lock().lines() {
            let request: Value = serde_json::from_str(&raw.unwrap()).unwrap();
            let method = request["method"].as_str().unwrap();
            let request_id = request["id"].as_u64();
            let result = match method {
                "initialize" => {
                    assert!(!initialized);
                    assert_eq!(
                        request["params"],
                        json!({
                            "clientInfo": {
                                "name": "codex_usage_widget",
                                "title": "Codex Usage Widget",
                                "version": env!("CARGO_PKG_VERSION"),
                            },
                            "capabilities": {},
                        })
                    );
                    if mode == "hang" {
                        fs::write(marker, std::process::id().to_string()).unwrap();
                    }
                    let expected_id = if mode == "restart" && marker.exists() {
                        3
                    } else {
                        1
                    };
                    assert_eq!(request_id, Some(expected_id));
                    id = request_id;
                    json!({"userAgent": "test"})
                }
                "initialized" => {
                    assert!(!initialized);
                    assert!(request.get("id").is_none());
                    assert_eq!(request["params"], json!({}));
                    initialized = true;
                    continue;
                }
                _ => {
                    assert!(initialized);
                    assert_eq!(request_id, id.map(|value| value + 1));
                    id = request_id;
                    match mode.as_str() {
                        "interleave" => {
                            writeln!(
                                responses,
                                "{}",
                                json!({"method": NOTICE, "params": {"test": true}})
                            )
                            .unwrap();
                            json!({"ok": true})
                        }
                        "hang" => {
                            thread::sleep(Duration::from_secs(10));
                            continue;
                        }
                        "restart" if !marker.exists() => {
                            fs::write(marker, "first process exited").unwrap();
                            std::process::exit(7);
                        }
                        "restart" => json!({"restarted": true}),
                        "unsigned" | "signed" | "account_rpc" => {
                            if method == "account/rateLimits/read" {
                                assert!(request.get("params").is_none());
                                json!({})
                            } else {
                                assert_eq!(method, "account/read");
                                assert_eq!(request["params"], json!({"refreshToken": false}));
                                if mode == "account_rpc" {
                                    writeln!(responses, "{}", json!({
                                        "id": request_id,
                                        "error": {"code": -32601, "message": "Method not found"},
                                    })).unwrap();
                                    responses.flush().unwrap();
                                    continue;
                                }
                                let account = if mode == "signed" {
                                    json!({})
                                } else {
                                    Value::Null
                                };
                                json!({"requiresOpenaiAuth": true, "account": account})
                            }
                        }
                        _ => json!({}),
                    }
                }
            };
            writeln!(responses, "{}", json!({"id": request_id, "result": result})).unwrap();
            responses.flush().unwrap();
        }
        if mode == "normal" {
            fs::write(marker, "stdin closed").unwrap();
        }
    }
}
