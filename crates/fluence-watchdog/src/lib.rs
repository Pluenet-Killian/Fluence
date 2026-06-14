// SPDX-License-Identifier: Apache-2.0

//! Process watchdog for the desktop app (PLAN 7.1): spawn the hub, watch it, and
//! restart it within a bounded delay (< 2 s) if it ever exits — autostart and
//! self-healing, so a crash is invisible to the user. The Tauri shell runs this
//! in the background and points its webview at the hub's local API (D-2.1: the
//! UI talks to the hub only over the network, even when embedded).
//!
//! Deliberately a plain process supervisor — *not* the hub's IPC supervisor
//! (which handshakes worker children over a pipe). The app does not speak the
//! worker protocol to the hub; it only keeps the hub process alive, with an
//! exponential backoff that caps below the 2 s restart budget so a crash-loop
//! cannot busy-spin yet a one-off crash recovers fast.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Poll granularity while a child runs / while backing off.
const POLL: Duration = Duration::from_millis(20);

/// How to launch and supervise the child.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// Program to run (the hub binary).
    pub program: PathBuf,
    /// Arguments.
    pub args: Vec<String>,
    /// Extra environment (e.g. `FLUENCE_WEB_DIR`).
    pub env: Vec<(String, String)>,
    /// Shortest delay before a restart — a healthy crash recovers about this
    /// fast.
    pub restart_floor: Duration,
    /// Hard cap on the restart delay (PLAN 7.1: < 2 s), bounding a crash loop.
    pub restart_cap: Duration,
    /// A child that runs at least this long is deemed healthy: its later exit
    /// resets the backoff to the floor (a one-off crash, not a crash loop).
    pub stable_after: Duration,
}

impl WatchdogConfig {
    /// A config for `program` with sane restart bounds (floor 200 ms, cap 2 s).
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            restart_floor: Duration::from_millis(200),
            restart_cap: Duration::from_secs(2),
            stable_after: Duration::from_secs(10),
        }
    }

    /// Appends a launch argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets an environment variable for the child.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// A running watchdog. Dropping it (or calling [`Watchdog::shutdown`]) stops the
/// child and the supervising thread.
#[derive(Debug)]
pub struct Watchdog {
    shutdown: Arc<AtomicBool>,
    restarts: Arc<AtomicU32>,
    handle: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Spawns the child and the supervising thread.
    ///
    /// # Panics
    ///
    /// Panics if the OS refuses to spawn the watchdog thread — an unrecoverable
    /// resource-exhaustion condition at process start.
    #[must_use]
    pub fn spawn(config: WatchdogConfig) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let restarts = Arc::new(AtomicU32::new(0));
        let handle = {
            let shutdown = shutdown.clone();
            let restarts = restarts.clone();
            std::thread::Builder::new()
                .name("fluence-watchdog".into())
                .spawn(move || run(&config, &shutdown, &restarts))
                .expect("spawning the watchdog thread never fails")
        };
        Self {
            shutdown,
            restarts,
            handle: Some(handle),
        }
    }

    /// Number of restarts since spawn (the initial start is not a restart).
    #[must_use]
    pub fn restart_count(&self) -> u32 {
        self.restarts.load(Ordering::SeqCst)
    }

    /// Signals stop, kills the child, and joins the supervising thread.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The supervising loop: (re)spawn the child, wait, back off, until shutdown.
fn run(config: &WatchdogConfig, shutdown: &AtomicBool, restarts: &AtomicU32) {
    let mut backoff = config.restart_floor;
    let mut first = true;
    while !shutdown.load(Ordering::SeqCst) {
        if !first {
            restarts.fetch_add(1, Ordering::SeqCst);
            if !sleep_unless_shutdown(backoff, shutdown) {
                break;
            }
            backoff = (backoff * 2).min(config.restart_cap);
        }
        first = false;

        let started = Instant::now();
        let mut child = match spawn_child(config) {
            Ok(child) => child,
            Err(error) => {
                // Spawn failed (binary missing, transient resource limit): do
                // not panic — back off and retry on the next iteration.
                tracing::warn!(%error, program = %config.program.display(), "watchdog: spawn failed");
                continue;
            }
        };

        wait_for_exit_or_shutdown(&mut child, shutdown);
        if shutdown.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        // The child exited on its own. If it had been up long enough, treat it
        // as a one-off and reset the backoff so the next crash recovers fast.
        if started.elapsed() >= config.stable_after {
            backoff = config.restart_floor;
        }
    }
}

fn spawn_child(config: &WatchdogConfig) -> std::io::Result<Child> {
    let mut command = Command::new(&config.program);
    command.args(&config.args);
    for (key, value) in &config.env {
        command.env(key, value);
    }
    command.spawn()
}

/// Polls the child until it exits or shutdown is requested.
fn wait_for_exit_or_shutdown(child: &mut Child, shutdown: &AtomicBool) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        match child.try_wait() {
            Ok(Some(_status)) => return, // exited on its own
            Ok(None) => std::thread::sleep(POLL),
            Err(_) => return, // cannot wait — treat as exited, respawn
        }
    }
}

/// Sleeps `dur` in small slices, returning `false` early if shutdown is
/// requested during the wait.
fn sleep_unless_shutdown(dur: Duration, shutdown: &AtomicBool) -> bool {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        if shutdown.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(POLL);
    }
    !shutdown.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child that exits immediately (cross-platform), with tight restart
    /// bounds so the test runs fast.
    fn quick_exit() -> WatchdogConfig {
        let mut config = if cfg!(windows) {
            WatchdogConfig::new("cmd").arg("/C").arg("exit")
        } else {
            WatchdogConfig::new("sh").arg("-c").arg("exit 0")
        };
        config.restart_floor = Duration::from_millis(20);
        config.restart_cap = Duration::from_millis(80);
        config
    }

    fn wait_until(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + deadline;
        while Instant::now() < end {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        done()
    }

    #[test]
    fn a_crashing_child_is_restarted_repeatedly() {
        let watchdog = Watchdog::spawn(quick_exit());
        assert!(
            wait_until(Duration::from_secs(5), || watchdog.restart_count() >= 3),
            "an immediately-exiting child must be restarted (got {})",
            watchdog.restart_count()
        );
        watchdog.shutdown();
    }

    #[test]
    fn shutdown_stops_the_supervisor_without_hanging() {
        let watchdog = Watchdog::spawn(quick_exit());
        assert!(wait_until(Duration::from_secs(5), || watchdog
            .restart_count()
            >= 1));
        let before = watchdog.restart_count();
        // `shutdown` joins the supervising thread; the test completing at all
        // proves the loop observed the signal and exited (no hang).
        watchdog.shutdown();
        assert!(before >= 1);
    }

    #[test]
    fn a_missing_binary_does_not_panic() {
        let mut config = WatchdogConfig::new("fluence-no-such-binary-xyz");
        config.restart_floor = Duration::from_millis(20);
        config.restart_cap = Duration::from_millis(80);
        let watchdog = Watchdog::spawn(config);
        // Spawn keeps failing; the watchdog backs off and retries, never panics.
        assert!(wait_until(Duration::from_secs(3), || watchdog
            .restart_count()
            >= 1));
        watchdog.shutdown();
    }
}
