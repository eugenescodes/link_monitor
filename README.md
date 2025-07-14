# Link Monitor

A simple application to monitor internet connectivity by periodically sending HTTP requests to a target URL and logging outages.

## Features

- Configurable check interval, log file, and ping target via `config.toml`
- Logs outages and recovery events
- Separate outage log entries for unsuccessful HTTP status or request failures

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
ping_target = "https://quad9.net, https://9.9.9.9, https://149.112.112.112"
```

- `log_file`: Path to the log file for outages and events
- `check_interval_seconds`: How often to check connectivity (in seconds)
- `ping_target`: A single URL or a comma-separated list of URLs to check for connectivity. The script considers the internet available if any of the targets respond successfully.

## Usage

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

3. Check the log file (default name: `internet_outages.log` in the project folder) for outage and recovery events.
   Example log file:

   ```text
   17:23:11 [INFO] Internet monitoring script started.
   17:23:11 [INFO] Check target: https://quad9.net
   17:23:11 [INFO] Check interval: 30 seconds.
   17:23:11 [INFO] Outage log file: internet_outages.log
   18:34:12 [INFO] Internet monitoring script started.
   18:34:12 [INFO] Check target: https://quad9.net, https://9.9.9.9, https://149.112.112.112
   18:34:12 [INFO] Check interval: 30 seconds.
   18:34:12 [INFO] Outage log file: internet_outages.log
   19:56:10 [ERROR] Internet unavailable since 2025-07-13 22:56:10. Error: error sending request for url (https://149.112.112.112/)
   Internet outage: 2025-07-13 22:56:10
   19:56:40 [INFO] Internet appeared at 2025-07-13 22:56:40
   ```

## License

This project is licensed under the terms of the GNU General Public License v3.0 license. See the `LICENSE` file for details.
