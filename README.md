# Link Monitor

A Rust-based internet connectivity monitoring tool that periodically checks specified URLs and logs outages and recoveries.

## Project Purpose

This tool monitors internet connectivity by sending HTTP GET requests to configured target URLs. It logs the status of each target and detects internet outages based on configurable failure thresholds.

## Configuration

The project uses a `config.toml` file to configure its behavior. Key configuration options include:

- `log_file`: Path to the log file where monitoring logs are saved.
- `log_to_console`: A boolean (`true` or `false`) to enable or disable logging to the console.
- `check_interval_seconds`: Interval in seconds between each round of checks.
- `max_retries`: Number of retry attempts for each target before considering it failed.
- `failure_threshold`: Number of consecutive failed checks across all targets to declare an internet outage.
- `request_timeout_seconds`: Timeout in seconds for each HTTP request.
- `retry_delay_seconds`: Delay in seconds between retry attempts.
- `ping_target`: A list of URLs to be monitored.

> [!NOTE]
> The config path is currently fixed to `config.toml`, resolved relative to
> the process's working directory -- there's no `--config` flag or env var
> to point it elsewhere. When running via Docker, the working directory is
> `/app`, so the file the binary actually reads is `/app/config.toml`.

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

Run the container, mounting your config and log directory:

```bash
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$(pwd)/config.toml":/app/config.toml:ro \
  -v "$(pwd)/logs":/app/logs \
  link_monitor
```

```bash
# Podman -- add the :Z suffix on SELinux systems (Fedora, RHEL, CentOS)
podman run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$(pwd)/config.toml":/app/config.toml:ro,Z \
  -v "$(pwd)/logs":/app/logs:Z \
  link_monitor
```

> [!NOTE]
> The image ships with a default `config.toml` baked in, so `docker run link_monitor`
> (with no mounts at all) works out of the box. Mounting your own file at
> `/app/config.toml` overrides it -- no rebuild needed to change settings.
>
> Both mounts target `/app/...`, not `/etc/link_monitor/...`: the binary
> resolves `config.toml` and its `log_file` setting relative to its working
> directory, which is `/app` inside the container.
>
> The container runs as a dedicated non-root user. `--user "$(id -u):$(id -g)"`
> makes it run as *you* instead, so it can write to your bind-mounted `logs/`
> directory (owned by your host user) without a permission error. Not required
> on Docker Desktop (macOS/Windows), which handles this automatically.

### Stopping the Application

Press Ctrl+C to stop the application gracefully. It will log shutdown events.

### Viewing Logs

- View container logs in real-time:

  ```bash
  docker logs -f <container_id_or_name>
  podman logs -f <container_id_or_name>
  ```

- View log files on the host machine (assuming the `logs/` directory is mounted):

  ```bash
  tail -f logs/internet_monitor.log
  ```

## License

This project is licensed under the terms of the GNU General Public License v3.0 license. See the `LICENSE` file for details.
