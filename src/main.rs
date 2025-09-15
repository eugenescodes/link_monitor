use chrono::Local;
use log::{LevelFilter, error, info};
use serde::Deserialize;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, WriteLogger};
use std::{fs::read_to_string, time::Duration};

// Structure for representing configuration from config.toml
#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    log_file: String,
    check_interval_seconds: u64,
    max_retries: u32,
    failure_threshold: u32,
    ping_target: String,
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
    for target in config
        .ping_target
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
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

fn init_logger(log_file_path: &str) -> Result<(), String> {
    use std::fs::OpenOptions;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .map_err(|e| format!("Failed to open log file '{log_file_path}': {e}"))?;

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Info,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(LevelFilter::Info, Config::default(), log_file),
    ])
    .map_err(|e| format!("Failed to initialize logger: {e}"))?;

    Ok(())
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
    let mut is_online = true; // Initial internet connection state

    // Parse comma-separated list of targets, trim whitespace
    let ping_targets: Vec<String> = config
        .ping_target
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client");

    let max_retries = config.max_retries;
    let retry_delay = Duration::from_secs(2);
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
                let mut last_error = None;
                let mut last_status = None;

                for target in &ping_targets {
                    let mut attempt = 0;
                    let mut success = false;
                    while attempt < max_retries {
                        match client.get(target).send().await {
                            Ok(response) => {
                                if response.status().is_success() {
                                    any_success = true;
                                    success = true;
                                    break;
                                } else {
                                    last_status = Some(response.status());
                                    // Log error only if no other target succeeded
                                    if !any_success {
                                        error!(
                                            "Request to target '{}' returned unsuccessful status: {}",
                                            target,
                                            response.status()
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                // Log error only if no other target succeeded
                                if !any_success {
                                    error!("Request to target '{target}' failed with error: {e}");
                                }
                                last_error = Some(e);
                            }
                        }
                        if !success {
                            tokio::time::sleep(retry_delay).await;
                        }
                        attempt += 1;
                    }
                    if success {
                        break;
                    }
                }

                let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                if any_success {
                    consecutive_failures = 0;
                    info!("Internet appeared at {timestamp}");
                    info!("Internet outage ended at {timestamp}");
                    is_online = true;
                    // Do not log repeated OKs
                } else {
                    consecutive_failures += 1;
                    if consecutive_failures >= failure_threshold && is_online {
                        if let Some(status) = last_status {
                            error!("Internet outage (unsuccessful status {status}): {timestamp}");
                        } else if let Some(e) = last_error {
                            error!("Internet outage: {timestamp}. Error: {e}");
                        } else {
                            error!("Internet outage: {timestamp}. Unknown error.");
                        }
                        is_online = false;
                    }
                }
                // Wait for the interval specified in the configuration
                tokio::time::sleep(Duration::from_secs(config.check_interval_seconds)).await;
            } => {}
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let config = load_config("config.toml")?;
    init_logger(&config.log_file)?;

    info!("Internet monitoring script started.");
    info!("Check target: {}", config.ping_target);
    info!("Check interval: {} seconds.", config.check_interval_seconds);
    info!("Outage log file: {}", config.log_file);

    run_monitor_loop(&config).await?;

    info!("Internet monitoring script stopped.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{AppConfig, run_monitor_loop};
    use std::fs::File;
    use std::io::Write;
    use tokio::runtime::Runtime;
    use tokio::sync::oneshot;

    #[test]
    fn test_config_load() {
        // Create a minimal config.toml for testing
        let config_content = r#"
log_file = "test_log.txt"
check_interval_seconds = 1
max_retries = 2
failure_threshold = 1
ping_target = "https://example.com, https://example.org"
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
        assert_eq!(config.ping_target.split(',').count(), 2);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.failure_threshold, 1);

        // Clean up
        std::fs::remove_file("test_config.toml").ok();
    }

    #[test]
    fn test_load_config_invalid_url() {
        let config_content = r#"
log_file = "test_log.txt"
check_interval_seconds = 1
max_retries = 2
failure_threshold = 1
ping_target = "ftp://invalid-url.com, not-a-url"
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
