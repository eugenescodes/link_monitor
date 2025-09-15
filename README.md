# Link Monitor

A simple application to monitor internet connectivity by periodically sending HTTP requests to a target URL and logging outages.

## Features

- Configurable check interval, log file, and ping target via `config.toml`
- Logs outages and recovery events
- Separate outage log entries for unsuccessful HTTP status or request failures
- Shutdown on Ctrl+C (SIGINT)

## Requirements

- Rust (install with your package manager or from <https://www.rust-lang.org/tools/install>)
- A `config.toml` file in the project root (see below)

### Quick Start

Clone the repository and enter the directory:

```bash
git clone <repo-url>
cd <folder>
```

## Configuration

Create a `config.toml` file in the project root with the following content:

```toml
log_file = "internet_outages.log"
check_interval_seconds = 30
max_retries = 5
failure_threshold = 3
ping_target = "https://on.quad9.net, https://one.one.one.one/help, https://dns.google"
```

- `log_file`: Path to the log file for outages and events
- `check_interval_seconds`: How often to check connectivity (in seconds)
- `max_retries`: Number of retries per target before considering failure
- `failure_threshold`: Number of consecutive failures before logging an outage
- `ping_target`: A single URL or a comma-separated list of URLs to check for connectivity. The script considers the internet available if any of the targets respond successfully.

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

### Running with Docker

1. Build the Docker image:

   ```bash
   docker build -t link_monitor .
   ```

2. Run the container, mounting your config and log directory as needed:

   ```bash
   docker run -v $(pwd)/config.toml:/etc/link_monitor/config.toml -v $(pwd)/logs:/etc/link_monitor/logs link_monitor
   ```

   Adjust the volume mounts to your config and desired log directory.

### Running with Podman

1. Build the Docker image use Podman:

   ```bash
   podman build -t link_monitor .
   ```

2. Run the container, mounting your config and log directory as needed:

   ```bash
   podman run -v $(pwd)/config.toml:/etc/link_monitor/config.toml:Z -v $(pwd)/logs:/etc/link_monitor/logs:Z link_monitor
   ```

   Adjust the volume mounts to your config and desired log directory.

3. The application logs messages to both the console and the log file specified in your `config.toml` (default: `internet_outages.log`).

4. To stop the application gracefully, press Ctrl+C (SIGINT). The app will handle shutdown cleanly and log the event.

5. Check the log file for detailed outage and recovery events.

## Updating the Container with New Code or Config

If you change source code or configuration files, rebuild the image and rerun the container:

### Using Podman

```bash
podman build -t link_monitor .
podman run -v $(pwd)/config.toml:/etc/link_monitor/config.toml:Z -v $(pwd)/logs:/etc/link_monitor/logs:Z link_monitor
```

### Using Docker

```bash
docker build -t link_monitor .
docker run -v $(pwd)/config.toml:/etc/link_monitor/config.toml -v $(pwd)/logs:/etc/link_monitor/logs link_monitor
```

To remove old images and containers:

### Using Podman

```bash
podman stop <container_id_or_name>
podman rm <container_id_or_name>
podman rmi <image_id_or_name>
podman system prune
```

### Using Docker

```bash
docker stop <container_id_or_name>
docker rm <container_id_or_name>
docker rmi <image_id_or_name>
docker system prune
```

## Cleaning Up Docker and Podman

To remove unused images, containers, and volumes, use the following commands:

- List all containers:

  ```bash
  docker ps -a
  podman ps -a
  ```

- Stop and remove containers:

  ```bash
  docker stop <container_id_or_name>
  docker rm <container_id_or_name>
  podman stop <container_id_or_name>
  podman rm <container_id_or_name>
  ```

- Remove images:

  ```bash
  docker rmi <image_id_or_name>
  podman rmi <image_id_or_name>
  ```

- Remove dangling images and unused volumes:

  ```bash
  docker system prune
  podman system prune
  ```

## Viewing Logs

- To view container logs in real-time:

  ```bash
  docker logs -f <container_id_or_name>
  podman logs -f <container_id_or_name>
  ```

- To view log files on the host machine (assuming you mounted the logs directory):

  ```bash
  tail -f logs/internet_outages.log
  ```

Example log file:

```text
17:23:11 [INFO] Internet monitoring script started.
17:23:11 [INFO] Check target: https://on.quad9.net, https://one.one.one.one/help, https://dns.google
17:23:11 [INFO] Check interval: 30 seconds.
17:23:11 [INFO] Outage log file: internet_outages.log
18:34:12 [INFO] Internet monitoring script started.
18:34:12 [INFO] Check target: https://on.quad9.net, https://one.one.one.one/help, https://dns.google
18:34:12 [INFO] Check interval: 30 seconds.
18:34:12 [INFO] Outage log file: internet_outages.log
19:56:10 [ERROR] Internet unavailable since 2025-07-13 22:56:10. Error: error sending request for url (https://on.quad9.net/)
Internet outage: 2025-07-13 22:56:10
19:56:40 [INFO] Internet appeared at 2025-07-13 22:56:40
```

## License

This project is licensed under the terms of the GNU General Public License v3.0 license. See the `LICENSE` file for details.
