use chrono::{DateTime, Local, TimeDelta};
use log::{LevelFilter, debug, error, info, warn};
use serde::Deserialize;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, TermLogger, TerminalMode, WriteLogger,
};
use std::{fs::read_to_string, io::LineWriter, time::Duration};
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Log target prefix of this crate (library and binary).
const CRATE_LOG_TARGET: &str = "link_monitor";

/// Structure for representing configuration from config.toml
///
/// # Examples
///
/// ```rust
/// use link_monitor::AppConfig;
///
/// let config_toml = r#"
/// log_file = "logs/test.log"
/// log_to_console = true
/// check_interval_seconds = 30
/// max_retries = 2
/// failure_threshold = 3
/// request_timeout_seconds = 5
/// retry_delay_seconds = 2
/// ping_target = ["https://example.com"]
/// "#;
///
/// let config: AppConfig = toml::from_str(config_toml).unwrap();
/// assert_eq!(config.max_retries, 2);
/// assert_eq!(config.ping_target[0], "https://example.com");
/// ```
#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub log_file: String,
    pub log_to_console: bool,
    pub check_interval_seconds: u64,
    pub max_retries: u32,
    pub failure_threshold: u32,
    pub request_timeout_seconds: u64,
    pub retry_delay_seconds: u64,
    pub ping_target: Vec<String>,
}

/// Loads the configuration from a TOML file.
///
/// # Errors
/// Returns an error string if the file cannot be read or parsed, if
/// `ping_target` is empty or any of its URLs is invalid, or if the
/// timing/retry parameters are zero.
///
/// # Examples
///
/// ```rust
/// use link_monitor::load_config;
/// use std::fs::File;
/// use std::io::Write;
///
/// // Create a temporary config file for the test
/// let mut file = File::create("dummy_config.toml").unwrap();
/// let toml_content = r#"
/// log_file = "test.log"
/// log_to_console = false
/// check_interval_seconds = 1
/// max_retries = 2
/// failure_threshold = 1
/// request_timeout_seconds = 5
/// retry_delay_seconds = 2
/// ping_target = ["https://example.com"]
/// "#;
/// file.write_all(toml_content.as_bytes()).unwrap();
///
/// // Load the config
/// let config = load_config("dummy_config.toml").expect("Failed to load config");
/// assert_eq!(config.ping_target.len(), 1);
/// assert_eq!(config.ping_target[0], "https://example.com");
///
/// // Clean up
/// std::fs::remove_file("dummy_config.toml").unwrap();
/// ```
pub fn load_config(path: &str) -> Result<AppConfig, String> {
    let config_content = read_to_string(path).map_err(|e| {
        format!(
            "Failed to read {}: {e}. Make sure the file exists in the project root.",
            path
        )
    })?;
    let config: AppConfig = toml::from_str(&config_content)
        .map_err(|e| format!("Failed to parse {}: {e}. Check the file syntax.", path))?;

    // Without targets every round would fail and a false outage would be reported.
    if config.ping_target.is_empty() {
        return Err("ping_target must contain at least one URL".to_string());
    }

    // Validate ping_target URLs
    for target in &config.ping_target {
        let url = match url::Url::parse(target) {
            Ok(url) => url,
            Err(_) => return Err(format!("Invalid URL in ping_target: '{}'", target)),
        };
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(format!(
                "ping_target must use http or https scheme: '{}'",
                target
            ));
        }
    }

    // Validate timing/retry parameters: zero values would disable the pause
    // between checks or the retry/threshold logic entirely.
    if config.check_interval_seconds == 0
        || config.failure_threshold == 0
        || config.request_timeout_seconds == 0
        || config.retry_delay_seconds == 0
    {
        return Err(
            "check_interval_seconds, failure_threshold, request_timeout_seconds \
             and retry_delay_seconds must be greater than 0"
                .to_string(),
        );
    }

    Ok(config)
}

/// Initializes the logging system to write to a file and optionally to the console.
///
/// Messages of this crate are logged from `Debug` up, messages of dependencies
/// (reqwest, hyper, rustls, ...) only from `Info` up.
///
/// # Examples
///
/// ```rust,no_run
/// use link_monitor::init_logger;
///
/// // Initialize logger logging to "app.log" and the console
/// init_logger("app.log", true).expect("Failed to initialize logger");
/// ```
pub fn init_logger(log_file_path: &str, log_to_console: bool) -> Result<(), String> {
    use std::fs::{OpenOptions, create_dir_all};
    use std::path::Path;

    // create directory for log file if it doesn't exist
    if let Some(parent) = Path::new(log_file_path).parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)
            .map_err(|e| format!("Failed to create log directory '{:?}': {}", parent, e))?;
    }

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .map_err(|e| format!("Failed to open log file '{log_file_path}': {e}"))?;

    // Each output gets two loggers: one for this crate at Debug and one for
    // everything else at Info.
    let own = ConfigBuilder::new()
        .add_filter_allow_str(CRATE_LOG_TARGET)
        .build();
    let deps = ConfigBuilder::new()
        .add_filter_ignore_str(CRATE_LOG_TARGET)
        .build();

    // Both file loggers append to the same file; `LineWriter` writes every
    // line with a single call, so lines of the two loggers never interleave.
    let deps_log_file = log_file
        .try_clone()
        .map_err(|e| format!("Failed to open log file '{log_file_path}': {e}"))?;
    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![
        WriteLogger::new(LevelFilter::Debug, own.clone(), LineWriter::new(log_file)),
        WriteLogger::new(
            LevelFilter::Info,
            deps.clone(),
            LineWriter::new(deps_log_file),
        ),
    ];

    if log_to_console {
        loggers.push(TermLogger::new(
            LevelFilter::Debug,
            own,
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ));
        loggers.push(TermLogger::new(
            LevelFilter::Info,
            deps,
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ));
    }

    CombinedLogger::init(loggers).map_err(|e| format!("Failed to initialize logger: {e}"))?;

    Ok(())
}

/// Represents the result of a single target check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The server sent an HTTP response. Any status code (including 4xx/5xx)
    /// proves that DNS, TCP and TLS work, so the connection counts as up.
    Reachable {
        /// Status code of the response.
        status: reqwest::StatusCode,
    },
    /// No HTTP response was received (DNS, connect, TLS or timeout error).
    Unreachable {
        /// The error together with its chain of causes.
        error: String,
    },
}

/// Builds the HTTP client used for connectivity checks.
///
/// Idle connections are not kept, so every check resolves DNS and opens a new
/// TCP/TLS connection instead of reusing one that might hide an outage.
///
/// # Errors
/// Returns an error if the TLS backend cannot be initialized.
///
/// # Examples
///
/// ```rust
/// use link_monitor::build_client;
/// use std::time::Duration;
///
/// let client = build_client(Duration::from_secs(5)).expect("client should build");
/// # drop(client);
/// ```
pub fn build_client(request_timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(request_timeout)
        .pool_max_idle_per_host(0)
        .build()
}

/// Sends a single request to `target`.
///
/// Returns [`CheckResult::Reachable`] if any HTTP response arrives, or
/// [`CheckResult::Unreachable`] with the error otherwise.
///
/// # Examples
///
/// ```rust,no_run
/// use link_monitor::{build_client, check_target};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let client = build_client(Duration::from_secs(5)).unwrap();
///     let result = check_target(&client, "https://example.com").await;
///     println!("{result:?}");
/// }
/// ```
pub async fn check_target(client: &reqwest::Client, target: &str) -> CheckResult {
    match client.get(target).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                debug!("Target '{target}' is reachable but answered with status {status}");
            }
            CheckResult::Reachable { status }
        }
        Err(e) => CheckResult::Unreachable {
            error: error_chain(&e),
        },
    }
}

/// Outcome of one monitoring round, see [`check_round`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundResult {
    /// At least one target answered.
    Reachable,
    /// No target answered on any attempt.
    Unreachable {
        /// `(target, error)` from the last attempt, in the order of the targets.
        failures: Vec<(String, String)>,
    },
}

/// Runs one monitoring round.
///
/// All `targets` are checked concurrently and the round succeeds as soon as
/// any of them answers, so a slow or dead target does not delay the result.
/// Only if every target fails is the whole attempt repeated, up to
/// `max_retries` more times with `retry_delay` in between.
///
/// # Examples
///
/// ```rust,no_run
/// use link_monitor::{RoundResult, build_client, check_round};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let client = build_client(Duration::from_secs(5)).unwrap();
///     let targets = vec!["https://example.com".to_string(), "https://example.org".to_string()];
///     let result = check_round(&client, &targets, 2, Duration::from_secs(1)).await;
///     assert_eq!(result, RoundResult::Reachable);
/// }
/// ```
pub async fn check_round(
    client: &reqwest::Client,
    targets: &[String],
    max_retries: u32,
    retry_delay: Duration,
) -> RoundResult {
    let mut failures: Vec<(usize, String)> = Vec::new();
    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(retry_delay).await;
        }

        let mut checks = JoinSet::new();
        for (index, target) in targets.iter().enumerate() {
            let client = client.clone();
            let target = target.clone();
            checks.spawn(async move { (index, check_target(&client, &target).await) });
        }

        failures.clear();
        while let Some(joined) = checks.join_next().await {
            match joined {
                // Dropping `checks` aborts the requests that are still running.
                Ok((_, CheckResult::Reachable { .. })) => return RoundResult::Reachable,
                Ok((index, CheckResult::Unreachable { error })) => {
                    debug!(
                        "Request to target '{}' failed (attempt {}/{}): {error}",
                        targets[index],
                        attempt + 1,
                        max_retries + 1
                    );
                    failures.push((index, error));
                }
                Err(e) => error!("Connectivity check task failed: {e}"),
            }
        }
    }

    failures.sort_unstable_by_key(|(index, _)| *index);
    RoundResult::Unreachable {
        failures: failures
            .into_iter()
            .map(|(index, error)| (targets[index].clone(), error))
            .collect(),
    }
}

/// Formats an error with all of its sources, e.g.
/// `error sending request: client error (Connect): Connection refused`.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// A change of the connectivity state reported by [`ConnectivityTracker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateChange {
    /// `failure_threshold` rounds in a row have failed.
    Outage {
        /// Start of the first failed round of this outage.
        since: DateTime<Local>,
    },
    /// A round succeeded after an outage.
    Restored {
        /// Start of the first failed round of the outage.
        since: DateTime<Local>,
        /// Time from `since` until the successful round.
        duration: TimeDelta,
    },
}

/// Tracks consecutive failed rounds and decides when the connection is
/// considered down or restored.
///
/// # Examples
///
/// ```rust
/// use chrono::{Local, TimeDelta};
/// use link_monitor::{ConnectivityTracker, StateChange};
///
/// let start = Local::now();
/// let mut tracker = ConnectivityTracker::new(2);
///
/// assert_eq!(tracker.record_round(false, start), None);
/// let outage = tracker.record_round(false, start + TimeDelta::seconds(30));
/// assert_eq!(outage, Some(StateChange::Outage { since: start }));
///
/// let restored = tracker.record_round(true, start + TimeDelta::seconds(90));
/// assert_eq!(
///     restored,
///     Some(StateChange::Restored { since: start, duration: TimeDelta::seconds(90) })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ConnectivityTracker {
    failure_threshold: u32,
    consecutive_failures: u32,
    first_failure_at: Option<DateTime<Local>>,
    outage_since: Option<DateTime<Local>>,
}

impl ConnectivityTracker {
    /// Creates a tracker that reports an outage after `failure_threshold`
    /// failed rounds in a row. The connection is initially assumed to be up.
    pub fn new(failure_threshold: u32) -> Self {
        Self {
            failure_threshold,
            consecutive_failures: 0,
            first_failure_at: None,
            outage_since: None,
        }
    }

    /// Returns `true` unless an outage is currently in progress.
    pub fn is_online(&self) -> bool {
        self.outage_since.is_none()
    }

    /// Records the result of a round that started at `started_at` and returns
    /// the state change it caused, if any.
    pub fn record_round(
        &mut self,
        success: bool,
        started_at: DateTime<Local>,
    ) -> Option<StateChange> {
        if success {
            self.consecutive_failures = 0;
            self.first_failure_at = None;
            return self.outage_since.take().map(|since| StateChange::Restored {
                since,
                duration: started_at - since,
            });
        }

        let first_failure_at = *self.first_failure_at.get_or_insert(started_at);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.is_online() && self.consecutive_failures >= self.failure_threshold {
            self.outage_since = Some(first_failure_at);
            return Some(StateChange::Outage {
                since: first_failure_at,
            });
        }
        None
    }
}

/// Formats a duration as e.g. `1h 02m 05s`, `4m 20s` or `35s`.
fn format_duration(duration: TimeDelta) -> String {
    let total = duration.num_seconds().max(0);
    let (hours, minutes, seconds) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// Runs the internet connectivity monitoring loop.
///
/// A round starts every `check_interval_seconds` (measured from the start of
/// the previous round, so slow rounds do not stretch the period). The loop
/// stops gracefully on SIGINT (`Ctrl+C`) and, on Unix, on SIGTERM
/// (`docker stop`, systemd).
///
/// # Errors
/// Returns an error if the HTTP client cannot be built or the signal handlers
/// cannot be installed.
///
/// # Examples
///
/// ```rust,no_run
/// use link_monitor::{AppConfig, run_monitor_loop};
///
/// #[tokio::main]
/// async fn main() {
///     let config: AppConfig = toml::from_str(r#"
///         log_file = "test.log"
///         log_to_console = false
///         check_interval_seconds = 30
///         max_retries = 2
///         failure_threshold = 3
///         request_timeout_seconds = 5
///         retry_delay_seconds = 2
///         ping_target = ["https://example.com"]
///     "#).unwrap();
///
///     let _ = run_monitor_loop(&config).await;
/// }
/// ```
pub async fn run_monitor_loop(
    config: &AppConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let client = build_client(Duration::from_secs(config.request_timeout_seconds))?;
    let retry_delay = Duration::from_secs(config.retry_delay_seconds);
    let mut tracker = ConnectivityTracker::new(config.failure_threshold);

    let mut ticker = tokio::time::interval(Duration::from_secs(config.check_interval_seconds));
    // If a round takes longer than the interval, start the next one right
    // after it instead of firing a burst of missed rounds.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            // Poll the signal first so the handlers are installed before the
            // first round and a pending shutdown always wins.
            biased;
            signal = &mut shutdown => {
                info!("{} received, stopping monitoring loop.", signal?);
                break;
            }
            () = async {
                ticker.tick().await;
                let round_started = Local::now();
                let round =
                    check_round(&client, &config.ping_target, config.max_retries, retry_delay)
                        .await;
                report_round(&mut tracker, &round, round_started);
            } => {}
        }
    }
    Ok(())
}

/// Waits for a shutdown request and returns the name of the received signal:
/// SIGINT (`Ctrl+C`) or, on Unix, SIGTERM.
async fn shutdown_signal() -> std::io::Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map(|()| "SIGINT"),
            _ = sigterm.recv() => Ok("SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.map(|()| "Ctrl+C")
    }
}

/// Feeds a round result into `tracker` and logs failed rounds and state changes.
fn report_round(
    tracker: &mut ConnectivityTracker,
    round: &RoundResult,
    started_at: DateTime<Local>,
) {
    let details = match round {
        RoundResult::Reachable => None,
        RoundResult::Unreachable { failures } => {
            let details = failures
                .iter()
                .map(|(target, error)| format!("'{target}': {error}"))
                .collect::<Vec<_>>()
                .join("; ");
            warn!("No target reachable: {details}");
            Some(details)
        }
    };

    match tracker.record_round(details.is_none(), started_at) {
        Some(StateChange::Outage { since }) => {
            error!(
                "Internet outage detected: no target reachable since {} \
                 ({} failed rounds in a row). Last errors: {}. \
                 Please check network connection/DNS settings.",
                since.format(TIMESTAMP_FORMAT),
                tracker.failure_threshold,
                details.unwrap_or_default(),
            );
        }
        Some(StateChange::Restored { since, duration }) => {
            info!(
                "Internet connection restored after an outage of {} (since {}).",
                format_duration(duration),
                since.format(TIMESTAMP_FORMAT),
            );
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_config_load() {
        let config_content = r#"
log_file = "test_log.txt"
log_to_console = false
check_interval_seconds = 1
max_retries = 2
failure_threshold = 1
request_timeout_seconds = 5
retry_delay_seconds = 2
ping_target = ["https://example.com", "https://example.org"]
"#;
        let mut file = File::create("test_config.toml").expect("Failed to create test config");
        file.write_all(config_content.as_bytes())
            .expect("Failed to write test config");

        let config_str =
            std::fs::read_to_string("test_config.toml").expect("Failed to read test config");
        let config: Result<AppConfig, _> = toml::from_str(&config_str);
        assert!(config.is_ok(), "Config should parse correctly");
        let config = config.unwrap();
        assert_eq!(config.ping_target.len(), 2);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.failure_threshold, 1);

        std::fs::remove_file("test_config.toml").ok();
    }

    #[test]
    fn test_load_config_rejects_zero_values() {
        let config_content = r#"
log_file = "test_log.txt"
log_to_console = false
check_interval_seconds = 0
max_retries = 2
failure_threshold = 1
request_timeout_seconds = 5
retry_delay_seconds = 2
ping_target = ["https://example.com"]
"#;
        let mut file = File::create("test_zero_config.toml").expect("Failed to create test config");
        file.write_all(config_content.as_bytes())
            .expect("Failed to write test config");

        let load_result = load_config("test_zero_config.toml");

        std::fs::remove_file("test_zero_config.toml").ok();

        assert!(
            load_result.is_err(),
            "Config loading should fail when check_interval_seconds is 0"
        );
    }

    #[test]
    fn test_load_config_invalid_url() {
        let config_content = r#"
log_file = "test_log.txt"
log_to_console = false
check_interval_seconds = 1
max_retries = 2
failure_threshold = 1
request_timeout_seconds = 5
retry_delay_seconds = 2
ping_target = ["ftp://invalid-url.com", "not-a-url"]
"#;
        let mut file =
            File::create("test_invalid_config.toml").expect("Failed to create test config");
        file.write_all(config_content.as_bytes())
            .expect("Failed to write test config");

        let load_result = load_config("test_invalid_config.toml");

        std::fs::remove_file("test_invalid_config.toml").ok();

        assert!(
            load_result.is_err(),
            "Config loading should fail due to invalid URLs"
        );
    }

    #[test]
    fn test_load_config_rejects_empty_targets() {
        let config_content = r#"
log_file = "test_log.txt"
log_to_console = false
check_interval_seconds = 1
max_retries = 2
failure_threshold = 1
request_timeout_seconds = 5
retry_delay_seconds = 2
ping_target = []
"#;
        let mut file =
            File::create("test_empty_targets_config.toml").expect("Failed to create test config");
        file.write_all(config_content.as_bytes())
            .expect("Failed to write test config");

        let load_result = load_config("test_empty_targets_config.toml");

        std::fs::remove_file("test_empty_targets_config.toml").ok();

        assert!(
            load_result.is_err(),
            "Config loading should fail when ping_target is empty"
        );
    }

    fn at(seconds: i64) -> DateTime<Local> {
        DateTime::from_timestamp(1_700_000_000 + seconds, 0)
            .expect("valid timestamp")
            .with_timezone(&Local)
    }

    #[test]
    fn tracker_reports_outage_once_since_first_failed_round() {
        let mut tracker = ConnectivityTracker::new(3);

        assert_eq!(tracker.record_round(false, at(0)), None);
        assert_eq!(tracker.record_round(false, at(30)), None);
        assert_eq!(
            tracker.record_round(false, at(60)),
            Some(StateChange::Outage { since: at(0) })
        );
        assert!(!tracker.is_online());
        assert_eq!(tracker.record_round(false, at(90)), None);
    }

    #[test]
    fn tracker_success_resets_failure_streak() {
        let mut tracker = ConnectivityTracker::new(2);

        assert_eq!(tracker.record_round(false, at(0)), None);
        assert_eq!(tracker.record_round(true, at(30)), None);
        assert_eq!(tracker.record_round(false, at(60)), None);
        assert_eq!(
            tracker.record_round(false, at(90)),
            Some(StateChange::Outage { since: at(60) })
        );
    }

    #[test]
    fn tracker_reports_restore_with_outage_duration() {
        let mut tracker = ConnectivityTracker::new(1);

        assert_eq!(
            tracker.record_round(false, at(0)),
            Some(StateChange::Outage { since: at(0) })
        );
        assert_eq!(
            tracker.record_round(true, at(150)),
            Some(StateChange::Restored {
                since: at(0),
                duration: TimeDelta::seconds(150),
            })
        );
        assert!(tracker.is_online());
        assert_eq!(tracker.record_round(true, at(180)), None);
    }

    #[test]
    fn format_duration_uses_largest_units() {
        assert_eq!(format_duration(TimeDelta::seconds(35)), "35s");
        assert_eq!(format_duration(TimeDelta::seconds(260)), "4m 20s");
        assert_eq!(format_duration(TimeDelta::seconds(3725)), "1h 02m 05s");
    }

    mod network {
        use super::super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Instant;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn client(timeout: Duration) -> reqwest::Client {
            build_client(timeout).expect("client should build")
        }

        /// A URL on a local port that nothing listens on, so connecting is refused.
        fn refused_url() -> String {
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|listener| listener.local_addr())
                .expect("free port")
                .port();
            format!("http://127.0.0.1:{port}/")
        }

        /// A server whose responses arrive after `delay`.
        async fn slow_server(
            delay: Duration,
            expected_requests: impl Into<wiremock::Times>,
        ) -> MockServer {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_delay(delay))
                .expect(expected_requests)
                .mount(&server)
                .await;
            server
        }

        #[tokio::test]
        async fn http_error_status_counts_as_reachable() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(503))
                .expect(1)
                .mount(&server)
                .await;

            let result = check_target(&client(Duration::from_secs(5)), &server.uri()).await;

            assert_eq!(
                result,
                CheckResult::Reachable {
                    status: reqwest::StatusCode::SERVICE_UNAVAILABLE
                }
            );
        }

        #[tokio::test]
        async fn timeout_is_reported_as_unreachable() {
            let server = slow_server(Duration::from_secs(2), 1).await;

            let result = check_target(&client(Duration::from_millis(100)), &server.uri()).await;

            assert!(
                matches!(result, CheckResult::Unreachable { .. }),
                "expected Unreachable, got {result:?}"
            );
        }

        #[tokio::test]
        async fn unreachable_error_includes_root_cause() {
            let result = check_target(&client(Duration::from_secs(5)), &refused_url()).await;

            let CheckResult::Unreachable { error } = result else {
                panic!("expected Unreachable, got {result:?}");
            };
            assert!(
                error.to_lowercase().contains("refused"),
                "error should contain the root cause, got: {error}"
            );
        }

        #[tokio::test]
        async fn every_check_opens_a_new_connection() {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local addr");
            let connections = Arc::new(AtomicUsize::new(0));

            let accepted = Arc::clone(&connections);
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        // Answer every request on this keep-alive connection.
                        while let Ok(n) = socket.read(&mut buf).await {
                            if n == 0
                                || socket
                                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                                    .await
                                    .is_err()
                            {
                                break;
                            }
                        }
                    });
                }
            });

            let client = client(Duration::from_secs(5));
            let url = format!("http://{addr}/");
            for _ in 0..2 {
                assert_eq!(
                    check_target(&client, &url).await,
                    CheckResult::Reachable {
                        status: reqwest::StatusCode::OK
                    }
                );
            }

            assert_eq!(connections.load(Ordering::SeqCst), 2);
        }

        #[tokio::test]
        async fn round_does_not_wait_for_a_slow_target() {
            let slow = slow_server(Duration::from_secs(3), 0..=1).await;
            let fast = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&fast)
                .await;

            let started = Instant::now();
            let result = check_round(
                &client(Duration::from_secs(5)),
                &[slow.uri(), fast.uri()],
                0,
                Duration::ZERO,
            )
            .await;

            assert_eq!(result, RoundResult::Reachable);
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "round waited for the slow target: {:?}",
                started.elapsed()
            );
        }

        #[tokio::test]
        async fn round_tries_other_targets_before_retrying() {
            let working = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&working)
                .await;

            let started = Instant::now();
            let result = check_round(
                &client(Duration::from_secs(5)),
                &[refused_url(), working.uri()],
                3,
                Duration::from_secs(10),
            )
            .await;

            assert_eq!(result, RoundResult::Reachable);
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "round retried the dead target first: {:?}",
                started.elapsed()
            );
        }

        #[tokio::test]
        async fn round_retries_all_targets_and_reports_each_failure() {
            let first = slow_server(Duration::from_secs(2), 2).await;
            let second = slow_server(Duration::from_secs(2), 2).await;
            let targets = [first.uri(), second.uri()];

            let result = check_round(
                &client(Duration::from_millis(100)),
                &targets,
                1,
                Duration::ZERO,
            )
            .await;

            let RoundResult::Unreachable { failures } = result else {
                panic!("expected Unreachable, got {result:?}");
            };
            let failed_targets: Vec<&str> =
                failures.iter().map(|(target, _)| target.as_str()).collect();
            assert_eq!(failed_targets, [targets[0].as_str(), targets[1].as_str()]);
        }

        #[tokio::test]
        async fn round_succeeds_when_target_recovers_on_retry() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
                .up_to_n_times(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;

            let result = check_round(
                &client(Duration::from_millis(100)),
                &[server.uri()],
                1,
                Duration::ZERO,
            )
            .await;

            assert_eq!(result, RoundResult::Reachable);
        }
    }
}
