use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::WidgetConfig;
use crate::icons::{self, RgbaImage};
use crate::models::Usage;
use crate::provider::UsageProvider;
use crate::utils::format_until;
use crate::utils::round_percent;

const FETCH_DEADLINE: Duration = Duration::from_secs(30);
const RESULT_GRACE: Duration = Duration::from_secs(8);
const INITIAL_ERROR_BACKOFF: Duration = Duration::from_secs(5);
const MAX_TRAY_TITLE: usize = 127;

pub(crate) fn truncate_title(text: &str) -> String {
    if text.chars().count() <= MAX_TRAY_TITLE {
        return text.to_string();
    }
    let prefix: String = text.chars().take(MAX_TRAY_TITLE - 1).collect();
    format!("{}…", prefix.trim_end())
}

pub(crate) fn format_tray_title(header: &str, summary: &str, footer: &str) -> String {
    let fixed = if footer.is_empty() {
        header.to_string()
    } else {
        format!("{header}\n{footer}")
    };
    if summary.is_empty() {
        return truncate_title(&fixed);
    }
    let budget = MAX_TRAY_TITLE as isize - fixed.chars().count() as isize - 1;
    if budget <= 1 {
        return truncate_title(&fixed);
    }
    let budget = budget as usize;
    let fitted = if summary.chars().count() <= budget {
        summary.to_string()
    } else {
        let prefix: String = summary.chars().take(budget - 1).collect();
        format!("{}…", prefix.trim_end())
    };
    if footer.is_empty() {
        format!("{header}\n{fitted}")
    } else {
        format!("{header}\n{fitted}\n{footer}")
    }
}

fn format_usage_title(tooltip_name: &str, usage: &Usage) -> String {
    let summary = usage
        .limits
        .iter()
        .map(|limit| {
            let pct = round_percent(limit.utilization);
            format!("{}: {pct}%", limit.short_label)
        })
        .collect::<Vec<_>>()
        .join("  ·  ");
    let footer = match usage.primary() {
        Some(primary) => format!(
            "{} reset: {}",
            primary.short_label,
            format_until(primary.resets_at)
        ),
        None => String::new(),
    };
    format_tray_title(tooltip_name, &summary, &footer)
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub usage: Option<Usage>,
    pub last_error: Option<String>,
}

struct Event {
    flag: Mutex<bool>,
    cvar: Condvar,
}

impl Event {
    fn new() -> Self {
        Self {
            flag: Mutex::new(false),
            cvar: Condvar::new(),
        }
    }

    fn set(&self) {
        let mut flag = self.flag.lock().unwrap();
        *flag = true;
        self.cvar.notify_all();
    }

    fn clear(&self) {
        *self.flag.lock().unwrap() = false;
    }

    fn is_set(&self) -> bool {
        *self.flag.lock().unwrap()
    }

    fn wait_timeout(&self, timeout: Duration) {
        let flag = self.flag.lock().unwrap();
        if *flag {
            return;
        }
        let _ = self
            .cvar
            .wait_timeout_while(flag, timeout, |set| !*set)
            .unwrap();
    }
}

#[derive(Default)]
struct FetchState {
    in_flight: bool,
    started_at: Option<Instant>,
    seq: u64,
}

fn next_wait_and_backoff(
    has_error: bool,
    error_backoff: Duration,
    refresh_interval: Duration,
) -> (Duration, Duration) {
    if !has_error {
        (refresh_interval, INITIAL_ERROR_BACKOFF)
    } else {
        let wait = error_backoff.min(refresh_interval);
        let next_backoff = (error_backoff * 2).min(refresh_interval);
        (wait, next_backoff)
    }
}

type Listener = Arc<dyn Fn() + Send + Sync>;

pub struct AppState {
    provider: Box<dyn UsageProvider>,
    config: &'static WidgetConfig,
    inner: Mutex<Snapshot>,
    tray_title: Mutex<String>,
    refresh_event: Event,
    stop_event: Event,
    result_ready: Event,
    listeners: Mutex<Vec<Listener>>,
    fetch_state: Mutex<FetchState>,
    force_dispatch: AtomicBool,
    closed: AtomicBool,
}

impl AppState {
    pub fn new(provider: Box<dyn UsageProvider>, config: &'static WidgetConfig) -> Arc<Self> {
        let initial_title = truncate_title(&format!("{} – loading…", config.tooltip_name));
        Arc::new(Self {
            provider,
            config,
            inner: Mutex::new(Snapshot::default()),
            tray_title: Mutex::new(initial_title),
            refresh_event: Event::new(),
            stop_event: Event::new(),
            result_ready: Event::new(),
            listeners: Mutex::new(Vec::new()),
            fetch_state: Mutex::new(FetchState::default()),
            force_dispatch: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }

    pub fn config(&self) -> &'static WidgetConfig {
        self.config
    }

    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().clone()
    }

    pub fn tray_title(&self) -> String {
        self.tray_title.lock().unwrap().clone()
    }

    pub fn tray_icon(&self) -> RgbaImage {
        let snapshot = self.snapshot();
        if snapshot.last_error.is_some() {
            icons::error_icon()
        } else if let Some(ref usage) = snapshot.usage {
            icons::usage_icon(usage, self.config.accent)
        } else {
            icons::loading_icon()
        }
    }

    pub fn add_listener(&self, callback: impl Fn() + Send + Sync + 'static) {
        self.listeners.lock().unwrap().push(Arc::new(callback));
    }

    fn notify(&self) {
        let callbacks = self.listeners.lock().unwrap().clone();
        for callback in callbacks {
            let _ = panic::catch_unwind(AssertUnwindSafe(|| callback()));
        }
    }

    pub fn trigger_refresh(&self) {
        self.force_dispatch.store(true, Ordering::Release);
        self.refresh_event.set();
    }

    pub fn stop(&self) {
        self.stop_event.set();
        self.result_ready.set();
        self.refresh_event.set();
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = panic::catch_unwind(AssertUnwindSafe(|| self.provider.close()));
    }

    pub fn start(self: &Arc<Self>) {
        let state = Arc::clone(self);
        let name = format!("{}-scheduler", self.config.tray_name);
        let _ = thread::Builder::new().name(name).spawn(move || {
            state.worker_loop();
        });
    }

    pub fn worker_loop(self: &Arc<Self>) {
        let mut error_backoff = INITIAL_ERROR_BACKOFF;
        while !self.stop_event.is_set() {
            let force = self.force_dispatch.swap(false, Ordering::AcqRel);
            self.result_ready.clear();
            self.dispatch_fetch(force);
            self.result_ready.wait_timeout(RESULT_GRACE);

            let (wait_duration, next_backoff) = next_wait_and_backoff(
                self.snapshot().last_error.is_some(),
                error_backoff,
                self.config.refresh_interval,
            );
            error_backoff = next_backoff;

            self.refresh_event.wait_timeout(wait_duration);
            self.refresh_event.clear();
        }
    }

    fn publish_success(&self, usage: Usage) {
        let title = format_usage_title(self.config.tooltip_name, &usage);
        {
            let mut snap = self.inner.lock().unwrap();
            snap.usage = Some(usage);
            snap.last_error = None;
        }
        *self.tray_title.lock().unwrap() = title;
        self.result_ready.set();
        self.notify();
    }

    fn publish_error(&self, message: String) {
        let title = truncate_title(&format!("{} – error\n{message}", self.config.tooltip_name));
        {
            let mut snap = self.inner.lock().unwrap();
            snap.last_error = Some(message);
        }
        *self.tray_title.lock().unwrap() = title;
        self.result_ready.set();
        self.notify();
    }

    fn dispatch_fetch(self: &Arc<Self>, force: bool) {
        let sequence = {
            let mut fetch = self.fetch_state.lock().unwrap();
            let now = Instant::now();
            let in_flight = fetch.in_flight
                && fetch
                    .started_at
                    .is_some_and(|started| now.duration_since(started) < FETCH_DEADLINE);
            if in_flight && !force {
                return;
            }
            fetch.seq += 1;
            fetch.in_flight = true;
            fetch.started_at = Some(now);
            fetch.seq
        };

        let state = Arc::clone(self);
        let name = format!("{}-fetch-{sequence}", self.config.tray_name);
        let _ = thread::Builder::new().name(name).spawn(move || {
            state.run_fetch(sequence);
        });
    }

    fn finish_in_flight(&self, sequence: u64) -> bool {
        let mut fetch = self.fetch_state.lock().unwrap();
        let is_latest = sequence == fetch.seq;
        if is_latest {
            fetch.in_flight = false;
        }
        is_latest
    }

    fn run_fetch(&self, sequence: u64) {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| self.provider.fetch_usage()));
        if !self.finish_in_flight(sequence) {
            return;
        }
        match outcome {
            Ok(Ok(usage)) => self.publish_success(usage),
            Ok(Err(err)) => self.publish_error(err.to_string()),
            Err(payload) => self.publish_error(format_panic_payload(payload)),
        }
    }
}

fn format_panic_payload(payload: Box<dyn Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        format!("RuntimeError: {msg}")
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        format!("RuntimeError: {msg}")
    } else {
        "RuntimeError: provider panicked".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::config::{DEFAULT_REFRESH_INTERVAL, Logo};
    use crate::models::UsageLimit;
    use crate::provider::ProviderError;

    static TEST_CONFIG: WidgetConfig = WidgetConfig {
        app_name: "AntigravityUsageWidget",
        tray_name: "antigravity-usage",
        display_name: "Antigravity Usage Widget",
        popup_title: "Antigravity · Usage",
        tooltip_name: "Antigravity",
        accent: "#34A853",
        accent_hover: "#1E5F30",
        logo: Logo {
            svg: b"",
            scale: 1.0,
        },
        refresh_interval: DEFAULT_REFRESH_INTERVAL,
    };

    fn sample_limit(key: &str, short_label: &str, utilization: f64) -> UsageLimit {
        UsageLimit {
            key: key.to_string(),
            label: short_label.to_string(),
            short_label: short_label.to_string(),
            utilization,
            resets_at: Some(Utc.with_ymd_and_hms(2026, 11, 27, 14, 32, 20).unwrap()),
            severity: "normal".to_string(),
            is_active: true,
        }
    }

    fn sample_multi_group_usage() -> Usage {
        Usage {
            limits: vec![
                sample_limit("antigravity:gemini-5h", "Gemini 5h", 0.0263),
                sample_limit("antigravity:gemini-weekly", "Gemini 7d", 0.0075),
                sample_limit("antigravity:3p-5h", "Claude/GPT 5h", 0.6),
                sample_limit("antigravity:3p-weekly", "Claude/GPT 7d", 0.0),
                sample_limit(
                    "antigravity:gemini-argon-wtu-weekly",
                    "Gemini 4 Argon 7d",
                    0.0,
                ),
            ],
            fetched_at: Utc.with_ymd_and_hms(2026, 11, 20, 14, 32, 20).unwrap(),
            primary_key: Some("antigravity:gemini-5h".to_string()),
            details: Vec::new(),
        }
    }

    struct NoopProvider;
    impl UsageProvider for NoopProvider {
        fn fetch_usage(&self) -> Result<Usage, ProviderError> {
            Ok(sample_multi_group_usage())
        }
    }

    #[test]
    fn tray_tooltip_stays_within_windows_limit_for_multi_group_usage() {
        let state = AppState::new(Box::new(NoopProvider), &TEST_CONFIG);
        assert_eq!(state.tray_title(), "Antigravity – loading…");

        state.publish_success(sample_multi_group_usage());
        let title = state.tray_title();
        assert!(title.chars().count() <= MAX_TRAY_TITLE);
        assert!(title.starts_with("Antigravity\n"));
        assert!(title.contains("reset:"));

        state.publish_error("x".repeat(200));
        let err_title = state.tray_title();
        assert!(err_title.chars().count() <= MAX_TRAY_TITLE);
        assert!(err_title.starts_with("Antigravity – error\n"));
        assert!(err_title.ends_with('…'));
    }

    #[test]
    fn tooltip_format_and_truncation_edge_cases() {
        assert_eq!(truncate_title("short"), "short");

        let exact = "a".repeat(MAX_TRAY_TITLE);
        assert_eq!(truncate_title(&exact), exact);

        let over_with_spaces = format!("{}{}   ", "a".repeat(120), "b".repeat(5));
        let truncated = truncate_title(&over_with_spaces);
        assert!(truncated.chars().count() <= MAX_TRAY_TITLE);
        assert_eq!(truncated, format!("{}{}…", "a".repeat(120), "b".repeat(5)));

        let formatted = format_tray_title("Claude Code", "5h: 37%  ·  7d: 81%", "5h reset: 2h 05m");
        assert_eq!(
            formatted,
            "Claude Code\n5h: 37%  ·  7d: 81%\n5h reset: 2h 05m"
        );

        let no_summary = format_tray_title("Claude Code", "", "5h reset: now");
        assert_eq!(no_summary, "Claude Code\n5h reset: now");

        let huge_header = "H".repeat(126);
        let no_budget = format_tray_title(&huge_header, "5h: 10%", "footer");
        assert!(no_budget.chars().count() <= MAX_TRAY_TITLE);
    }

    #[test]
    fn preserves_last_valid_usage_when_fetch_fails_or_panics() {
        let state = AppState::new(Box::new(NoopProvider), &TEST_CONFIG);
        let valid = sample_multi_group_usage();
        state.publish_success(valid.clone());
        assert_eq!(state.snapshot().usage, Some(valid.clone()));
        assert_eq!(state.snapshot().last_error, None);

        state.publish_error("Service unavailable".to_string());
        let snap = state.snapshot();
        assert_eq!(snap.usage, Some(valid.clone()));
        assert_eq!(snap.last_error.as_deref(), Some("Service unavailable"));
        assert_eq!(
            state.tray_title(),
            "Antigravity – error\nService unavailable"
        );

        struct PanicProvider;
        impl UsageProvider for PanicProvider {
            fn fetch_usage(&self) -> Result<Usage, ProviderError> {
                panic!("unexpected failure");
            }
        }

        let panic_state = AppState::new(Box::new(PanicProvider), &TEST_CONFIG);
        panic_state.publish_success(valid.clone());
        panic_state.fetch_state.lock().unwrap().seq = 1;
        panic_state.run_fetch(1);

        let snap = panic_state.snapshot();
        assert_eq!(snap.usage, Some(valid));
        assert_eq!(
            snap.last_error.as_deref(),
            Some("RuntimeError: unexpected failure")
        );
    }

    #[test]
    fn stop_is_idempotent_and_closes_provider_once() {
        struct CountingCloseProvider {
            closes: Arc<AtomicUsize>,
        }
        impl UsageProvider for CountingCloseProvider {
            fn fetch_usage(&self) -> Result<Usage, ProviderError> {
                Ok(sample_multi_group_usage())
            }
            fn close(&self) {
                self.closes.fetch_add(1, Ordering::SeqCst);
            }
        }

        let closes = Arc::new(AtomicUsize::new(0));
        let state = AppState::new(
            Box::new(CountingCloseProvider {
                closes: Arc::clone(&closes),
            }),
            &TEST_CONFIG,
        );

        let mut handles = Vec::new();
        for _ in 0..4 {
            let s = Arc::clone(&state);
            handles.push(thread::spawn(move || s.stop()));
        }
        for h in handles {
            h.join().unwrap();
        }
        state.stop();

        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn trigger_refresh_forces_immediate_fetch() {
        struct SequenceProvider {
            calls: Arc<AtomicUsize>,
        }
        impl UsageProvider for SequenceProvider {
            fn fetch_usage(&self) -> Result<Usage, ProviderError> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(Usage {
                    limits: vec![sample_limit("k", "5h", n as f64 * 0.1)],
                    fetched_at: Utc::now(),
                    primary_key: Some("k".to_string()),
                    details: Vec::new(),
                })
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let state = AppState::new(
            Box::new(SequenceProvider {
                calls: Arc::clone(&calls),
            }),
            &TEST_CONFIG,
        );
        let (tx, rx) = mpsc::channel();
        state.add_listener(move || {
            let _ = tx.send(());
        });

        state.start();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        state.trigger_refresh();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        state.stop();
    }

    #[test]
    fn discards_stale_fetch_results_when_forced_fetch_supersedes() {
        struct ControlledProvider {
            calls: AtomicUsize,
            slow_started_tx: Mutex<Option<mpsc::Sender<()>>>,
            slow_release_rx: Mutex<Option<mpsc::Receiver<()>>>,
        }
        impl UsageProvider for ControlledProvider {
            fn fetch_usage(&self) -> Result<Usage, ProviderError> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 1 {
                    if let Some(tx) = self.slow_started_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    if let Some(rx) = self.slow_release_rx.lock().unwrap().take() {
                        let _ = rx.recv_timeout(Duration::from_secs(2));
                    }
                    Ok(Usage {
                        limits: vec![sample_limit("k", "5h", 0.90)],
                        fetched_at: Utc::now(),
                        primary_key: Some("k".to_string()),
                        details: Vec::new(),
                    })
                } else {
                    Ok(Usage {
                        limits: vec![sample_limit("k", "5h", 0.25)],
                        fetched_at: Utc::now(),
                        primary_key: Some("k".to_string()),
                        details: Vec::new(),
                    })
                }
            }
        }

        let (slow_started_tx, slow_started_rx) = mpsc::channel();
        let (slow_release_tx, slow_release_rx) = mpsc::channel();
        let (notify_tx, notify_rx) = mpsc::channel();

        let state = AppState::new(
            Box::new(ControlledProvider {
                calls: AtomicUsize::new(0),
                slow_started_tx: Mutex::new(Some(slow_started_tx)),
                slow_release_rx: Mutex::new(Some(slow_release_rx)),
            }),
            &TEST_CONFIG,
        );
        state.add_listener(move || {
            let _ = notify_tx.send(());
        });

        state.dispatch_fetch(false);
        slow_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();

        // Non-forced dispatch while fetch #1 is still within deadline must be ignored.
        state.dispatch_fetch(false);
        assert_eq!(state.fetch_state.lock().unwrap().seq, 1);

        // Forced dispatch launches fetch #2 and increments sequence to 2.
        state.dispatch_fetch(true);
        assert_eq!(state.fetch_state.lock().unwrap().seq, 2);

        notify_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            state
                .snapshot()
                .usage
                .unwrap()
                .primary()
                .unwrap()
                .utilization,
            0.25
        );

        // Let stale fetch #1 finish; its result (0.90) must be discarded.
        slow_release_tx.send(()).unwrap();
        assert!(notify_rx.recv_timeout(Duration::from_millis(80)).is_err());
        assert_eq!(
            state
                .snapshot()
                .usage
                .unwrap()
                .primary()
                .unwrap()
                .utilization,
            0.25
        );
    }

    #[test]
    fn error_backoff_sequence_and_reset_on_success() {
        let interval = Duration::from_secs(60);
        let mut backoff = INITIAL_ERROR_BACKOFF;
        let mut waits = Vec::new();
        for _ in 0..6 {
            let (wait, next) = next_wait_and_backoff(true, backoff, interval);
            waits.push(wait.as_secs());
            backoff = next;
        }
        assert_eq!(waits, vec![5, 10, 20, 40, 60, 60]);

        let (wait, reset_backoff) = next_wait_and_backoff(false, backoff, interval);
        assert_eq!(wait, interval);
        assert_eq!(reset_backoff, Duration::from_secs(5));
    }
}
