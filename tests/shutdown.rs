//! End-to-end test of graceful shutdown: runs the real binary and signals it.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A temporary working directory that is removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A URL on a local port that nothing listens on, so no real network is used.
fn refused_url() -> String {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("free port")
        .port();
    format!("http://127.0.0.1:{port}/")
}

fn wait_for_log(log: &Path, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let content = fs::read_to_string(log).unwrap_or_default();
        if content.contains(needle) || Instant::now() >= deadline {
            return content;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    None
}

#[test]
fn sigterm_stops_monitor_gracefully() {
    let dir = TempDir::new("link_monitor_sigterm");
    fs::write(
        dir.0.join("config.toml"),
        format!(
            r#"
log_file = "monitor.log"
log_to_console = false
check_interval_seconds = 1
max_retries = 0
failure_threshold = 1
request_timeout_seconds = 1
retry_delay_seconds = 1
ping_target = ["{}"]
"#,
            refused_url()
        ),
    )
    .expect("write config");
    let log = dir.0.join("monitor.log");

    let mut child = Command::new(env!("CARGO_BIN_EXE_link_monitor"))
        .current_dir(&dir.0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start link_monitor");

    // Once the first round is logged, the signal handlers are installed.
    let content = wait_for_log(&log, "Internet outage detected", Duration::from_secs(10));
    assert!(
        content.contains("Internet outage detected"),
        "monitor did not complete a round; log:\n{content}"
    );

    let kill = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("run kill");
    assert!(kill.success(), "kill -TERM failed");

    let status = wait_for_exit(&mut child, Duration::from_secs(5))
        .expect("monitor did not exit within 5s after SIGTERM");
    assert!(status.success(), "monitor exited with {status}");

    let content = fs::read_to_string(&log).expect("read log");
    assert!(
        content.contains("SIGTERM received") && content.contains("monitoring script stopped"),
        "graceful shutdown not logged; log:\n{content}"
    );

    // Debug output of the app itself is kept, debug noise of dependencies is not.
    assert!(
        content.contains("failed (attempt 1/1)"),
        "debug messages of link_monitor are missing; log:\n{content}"
    );
    assert!(
        !content.contains("reqwest::"),
        "debug messages of dependencies leaked into the log; log:\n{content}"
    );
}
