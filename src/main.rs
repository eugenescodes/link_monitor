use chrono::Local;
use log::{LevelFilter, error, info};
use serde::Deserialize;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, WriteLogger};
use std::{fs::read_to_string, time::Duration};

// Structure for representing configuration from config.toml
#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    log_file: String,
    log_to_console: bool,
    check_interval_seconds: u64,
    max_retries: u32,
    failure_threshold: u32,
    request_timeout_seconds: u64,
    retry_delay_seconds: u64,
    ping_target: Vec<String>,
}

fn load_config(path: &str) -> Result<AppConfig, String> {
    let config_content = read_to_string(path).map_err(|e| {
        format!(
            "Failed to read {}: {e}. Make sure the file exists in the project root.",
            path
        )
    })?;
    let config: AppConfig = toml::from_str(&config_content)
        .map_err(|e| format!("Failed to parse {}: {e}. Check the file syntax.", path))?;

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

    Ok(config)
}

fn init_logger(log_file_path: &str, log_to_console: bool) -> Result<(), String> {
    use std::fs::OpenOptions;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .map_err(|e| format!("Failed to open log file '{log_file_path}': {e}"))?;

    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![
        WriteLogger::new(LevelFilter::Debug, Config::default(), log_file),
    ];

    if log_to_console {
        loggers.push(TermLogger::new(
            LevelFilter::Debug,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ));
    }

    CombinedLogger::init(loggers)
        .map_err(|e| format!("Failed to initialize logger: {e}"))?;

    Ok(())
}

/// Represents the result of a single target check.
#[derive(Debug)]
enum CheckResult {
    Success,
    HttpError {
        status: reqwest::StatusCode,
        reason: String,
    },
    NetworkError,
}

/// Runs the internet connectivity monitoring loop.
///
/// # Arguments
///
/// * `config` - The application configuration.
///
/// # Returns
///
/// Returns `Ok(())` on graceful shutdown or an error if initialization fails.
async fn run_monitor_loop(
    config: &AppConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// Attempts to send HTTP GET requests to a target URL with retries.
    ///
    /// Returns a `CheckResult` indicating the outcome.
    async fn check_target(
        client: &reqwest::Client,
        target: &str,
        max_retries: u32,
        retry_delay: Duration,
    ) -> CheckResult {
        for _attempt in 0..max_retries {
            match client.get(target).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        return CheckResult::Success;
                    } else {
                        let status = response.status();
                        let reason = status
                            .canonical_reason()
                            .unwrap_or("Unknown reason")
                            .to_string();
                        // Still a failure, but we'll log it and can retry.
                        log::debug!(
                            "Request to target '{}' returned unsuccessful status: {} ({})",
                            target,
                            status,
                            reason
                        );
                    }
                }
                Err(e) => {
                    log::debug!("Request error for target {}: {:?}", target, e);
                    // Sleep before retrying on network error
                    tokio::time::sleep(retry_delay).await;
                }
            }
        }
        // If all retries fail, determine the reason for the final failure.
        // Try one last time to get a specific error.
        match client.get(target).send().await {
            Ok(response) => CheckResult::HttpError {
                status: response.status(),
                reason: response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown reason")
                    .to_string(),
            },
            Err(_) => CheckResult::NetworkError,
        }
    }

    let mut is_online = true; // Initial internet connection state
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.request_timeout_seconds))
        .build()
        .expect("Failed to build HTTP client");
    let max_retries = config.max_retries;
    let retry_delay = Duration::from_secs(config.retry_delay_seconds);
    let mut consecutive_failures = 0;
    let failure_threshold = config.failure_threshold;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received, stopping monitoring loop.");
                break;
            }
            _ = async {
                let mut any_success = false;
                let mut last_error_result: Option<CheckResult> = None;
                let mut last_failed_target: Option<String> = None;

                for target in &config.ping_target {
                    match check_target(&client, target, max_retries, retry_delay).await {
                        CheckResult::Success => {
                            any_success = true;
                            break; // One success is enough to consider online
                        }
                        result => {
                            // It's a failure (HttpError or NetworkError)
                            info!("Check failed for target '{}': {:?}", target, result);
                            last_failed_target = Some(target.clone());
                            last_error_result = Some(result);
                        }
                    }
                }

                let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                if any_success {
                    consecutive_failures = 0;
                    if !is_online {
                        info!("Internet connection restored at {timestamp}");
                        is_online = true;
                    }
                } else {
                    consecutive_failures += 1;

                    if consecutive_failures >= failure_threshold && is_online {
                        let mut error_message = format!("Internet outage detected at {timestamp}.");
                        if let Some(failed_target) = &last_failed_target {
                            error_message.push_str(&format!(" Last failed target: '{}'.", failed_target));
                        }

                        if let Some(error_result) = last_error_result {
                            match error_result {
                                CheckResult::HttpError { status, reason } => {
                                    error_message.push_str(&format!(" Status: {} ({}).", status, reason));
                                }
                                CheckResult::NetworkError => {
                                    error_message.push_str(" Reason: Network or other error.");
                                }
                                CheckResult::Success => { /* This case is not possible here */ }
                            }
                        }

                        error_message.push_str(" Please check network connection/DNS settings.");
                        error!("{}", error_message);
                        is_online = false;
                    }
                }
                tokio::time::sleep(Duration::from_secs(config.check_interval_seconds)).await;
            } => {}
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let config = load_config("config.toml")?;
    init_logger(&config.log_file, config.log_to_console)?;

    info!("Internet monitoring script started.");
    info!("Check targets: {:?}", config.ping_target);
    info!("Check interval: {} seconds.", config.check_interval_seconds);
    info!("Outage log file: {}", config.log_file);

    run_monitor_loop(&config).await?;

    info!("Internet monitoring script stopped.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::AppConfig;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_config_load() {
        // Create a minimal config.toml for testing
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

        // Try to load config using the same logic as main.rs
        let config_str =
            std::fs::read_to_string("test_config.toml").expect("Failed to read test config");
        let config: Result<AppConfig, _> = toml::from_str(&config_str);
        assert!(config.is_ok(), "Config should parse correctly");
        let config = config.unwrap();
        assert_eq!(config.ping_target.len(), 2);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.failure_threshold, 1);

        // Clean up
        std::fs::remove_file("test_config.toml").ok();
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

        let _result = std::fs::read_to_string("test_invalid_config.toml")
            .map_err(|e| format!("Failed to read test config: {e}"))
            .and_then(|content| {
                toml::from_str::<AppConfig>(&content)
                    .map_err(|e| format!("Failed to parse test config: {e}"))
            });

        // The toml parsing itself will succeed, but our load_config function does validation,
        // so we test load_config directly instead.
        let load_result = crate::load_config("test_invalid_config.toml");

        // Clean up
        std::fs::remove_file("test_invalid_config.toml").ok();

        assert!(
            load_result.is_err(),
            "Config loading should fail due to invalid URLs"
        );
    }
}
