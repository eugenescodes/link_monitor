
# Link Monitor

A Rust-based internet connectivity monitoring tool that periodically checks specified URLs and logs outages and recoveries.

## Project Purpose

This tool monitors internet connectivity by sending HTTP GET requests to configured target URLs. It logs the status of each target and detects internet outages based on configurable failure thresholds.

## Configuration

The project uses a `config.toml` file to configure its behavior. Key configuration options include:

- `log_file`: Path to the log file where monitoring logs are saved.
- `check_interval_seconds`: Interval in seconds between each round of checks.
- `max_retries`: Number of retry attempts for each target before considering it failed.
- `failure_threshold`: Number of consecutive failed checks across all targets to declare an internet outage.
- `ping_target`: A list of URLs to be monitored.

## Main Components and Workflow

- Loads configuration from `config.toml`.
- Initializes logging to file and console.
- Creates an asynchronous Tokio runtime for concurrent operations.
- Runs a monitoring loop that:
  - Checks each target URL with retries.
  - Logs success or failure for each attempt.
  - Tracks consecutive failures and logs internet outages when thresholds are met.
- Supports graceful shutdown on Ctrl+C (SIGINT).

## Usage

### Running Locally

1. Build the project:

   ```bash
   cargo build --release
   ```

2. Run the monitor:

   ```bash
   cargo run --release
   ```

   Or run the compiled binary directly:

   ```bash
   ./target/release/link_monitor
   ```

### Running with Docker or Podman

Build the container image:

```bash
docker build -t link_monitor .
# or
podman build -t link_monitor .
```

Run the container, mounting your config and log directory as needed:

```bash
docker run -v $(pwd)/config.toml:/etc/link_monitor/config.toml -v $(pwd)/logs:/etc/link_monitor/logs link_monitor
# or
podman run -v $(pwd)/config.toml:/etc/link_monitor/config.toml:Z -v $(pwd)/logs:/etc/link_monitor/logs:Z link_monitor
```

### Stopping the Application

Press Ctrl+C to stop the application gracefully. It will log shutdown events.

### Viewing Logs

- View container logs in real-time:

  ```bash
  docker logs -f <container_id_or_name>
  podman logs -f <container_id_or_name>
  ```

- View log files on the host machine (assuming logs directory is mounted):

  ```bash
  tail -f logs/internet_monitor.log
  ```

## License

This project is licensed under the terms of the GNU General Public License v3.0 license. See the `LICENSE` file for details.
