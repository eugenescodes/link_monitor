use chrono::Local;
use log::{LevelFilter, error, info};
use serde::Deserialize;
use simplelog::{Config, WriteLogger};
use std::{
    fs::{OpenOptions, read_to_string},
    time::Duration,
};

// Structure for representing configuration from config.toml
#[derive(Deserialize, Debug)]
pub struct AppConfig {
    log_file: String,
    check_interval_seconds: u64,
    ping_target: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load configuration from config.toml
    let config_content = read_to_string("config.toml").map_err(|e| {
        format!("Failed to read config.toml: {e}. Make sure the file exists in the project root.")
    })?;
    let config: AppConfig = toml::from_str(&config_content)
        .map_err(|e| format!("Failed to parse config.toml: {e}. Check the file syntax."))?;

    // 2. Initialize logger using the path from configuration
    let log_file_path = &config.log_file;
    let log_file_handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .map_err(|e| format!("Failed to open log file '{log_file_path}': {e}"))?;

    WriteLogger::init(LevelFilter::Info, Config::default(), log_file_handle)?;

    info!("Internet monitoring script started.");
    info!("Check target: {}", config.ping_target);
    info!("Check interval: {} seconds.", config.check_interval_seconds);
    info!("Outage log file: {}", config.log_file);

    let mut is_online = true; // Initial internet connection state

    // Parse comma-separated list of targets, trim whitespace
    let ping_targets: Vec<String> = config
        .ping_target
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    loop {
        let mut any_success = false;
        let mut last_error = None;
        let mut last_status = None;

        for target in &ping_targets {
            match reqwest::get(target).await {
                Ok(response) => {
                    if response.status().is_success() {
                        any_success = true;
                        break;
                    } else {
                        last_status = Some(response.status());
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        if any_success {
            if !is_online {
                info!("Internet appeared at {timestamp}");
                is_online = true;
            }
            // Do not log repeated OKs
        } else if is_online {
            if let Some(status) = last_status {
                error!("Internet outage (unsuccessful status {status}): {timestamp}");
            } else if let Some(e) = last_error {
                error!("Internet outage: {timestamp}. Error: {e}");
            } else {
                error!("Internet outage: {timestamp}. Unknown error.");
            }
            is_online = false;
        }
        // Do not log repeated outages
        // Wait for the interval specified in the configuration
        tokio::time::sleep(Duration::from_secs(config.check_interval_seconds)).await;
    }
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
check_interval_seconds = 1
ping_target = "https://example.com"
"#;
        let mut file = File::create("test_config.toml").expect("Failed to create test config");
        file.write_all(config_content.as_bytes())
            .expect("Failed to write test config");

        // Try to load config using the same logic as main.rs
        let config_str =
            std::fs::read_to_string("test_config.toml").expect("Failed to read test config");
        let config: Result<AppConfig, _> = toml::from_str(&config_str);
        assert!(config.is_ok(), "Config should parse correctly");

        // Clean up
        std::fs::remove_file("test_config.toml").ok();
    }
}
