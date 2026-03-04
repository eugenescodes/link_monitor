use link_monitor::{init_logger, load_config, run_monitor_loop};
use log::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    // Load config from root directory
    let config = load_config("config.toml")?;

    // Setup logging
    init_logger(&config.log_file, config.log_to_console)?;

    info!("Internet monitoring script started.");
    info!("Check targets: {:?}", config.ping_target);
    info!("Check interval: {} seconds.", config.check_interval_seconds);
    info!("Outage log file: {}", config.log_file);

    // Run the main blocking loop
    run_monitor_loop(&config).await?;

    info!("Internet monitoring script stopped.");
    Ok(())
}
