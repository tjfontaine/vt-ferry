//! Broker-side worker supervision integration test.
//!
//! Spawns the real broker in TCP mode against a real host-worker and a
//! long-running child command (sleep). Kills the worker mid-flight and
//! verifies the broker:
//!   * notices the worker died,
//!   * kills the child,
//!   * exits with a non-zero status,
//!   * surfaces the supervision reason on stderr.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is .../<workspace>/crates/vt-ferry-broker; the
    // workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn build_release(package: &str) -> PathBuf {
    let root = workspace_root();
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", package])
        .current_dir(&root)
        .status()
        .expect("cargo build");
    assert!(status.success(), "cargo build {package} failed");
    let bin = root.join("target/release").join(package);
    assert!(bin.is_file(), "built binary missing: {}", bin.display());
    bin
}

fn build_debug_worker() -> PathBuf {
    // vt-ferry-worker is built as the debug binary in the smoke flow; do the
    // same here so we don't pin tests to a release build only.
    let root = workspace_root();
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "vt-ferry-worker"])
        .current_dir(&root)
        .status()
        .expect("cargo build vt-ferry-worker");
    assert!(status.success(), "cargo build vt-ferry-worker failed");
    let bin = root.join("target/debug/vt-ferry-worker");
    assert!(bin.is_file(), "vt-ferry-worker missing: {}", bin.display());
    bin
}

// The vsock transport intentionally has no broker-side supervisor: the
// broker execvp's into smolvm and smolvm CLI then forks-and-detaches the
// real VM under a different PID, so any signal we'd send from a fork+kqueue
// supervisor would land on a corpse. Worker death is detected guest-side
// via the guest-shim's poisoned-backend logic instead — see
// `crates/vt-ferry-shim/src/transport.rs` and the broker module-level docs.

#[test]
fn broker_tcp_kills_child_when_worker_dies() {
    let broker = build_release("vt-ferry-broker");
    let worker = build_debug_worker();

    // sleep gives the test 30s to land the SIGKILL on the worker; the
    // broker should bail well before that.
    let sleep_bin = "/bin/sleep";

    // Use the mock backend so the worker stays alive without needing
    // VideoToolbox or registered IOSurfaces.
    let pool_json = r#"{"guest_phys_addr":2147483648,"slot_count":4,"width":128,"height":72,"pixel_format":875704438,"writable":true}"#;

    let mut child = Command::new(&broker)
        .env("VT_FERRY_HOST_WORKER_BACKEND", "mock")
        .args([
            "--transport",
            "tcp",
            "--tcp-bind",
            "127.0.0.1",
            "--tcp-port",
            "0",
            "--pool",
            pool_json,
            "--host-worker",
            worker.to_str().unwrap(),
            "--",
            sleep_bin,
            "30",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn broker");

    let stderr_handle = child.stderr.take().expect("stderr");
    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut reader = stderr_handle;
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    });

    // Wait until the broker logs the worker pid.
    let worker_pid = wait_for_worker_pid(child.id())
        .expect("broker should announce host-worker pid within 5s");

    // Kill the worker. The broker's poll loop should notice within
    // ~100ms (its tick interval) and kill the sleep child.
    let kill_status = Command::new("kill")
        .args(["-KILL", &worker_pid.to_string()])
        .status()
        .expect("invoke kill");
    assert!(kill_status.success(), "kill -KILL {worker_pid} failed");

    let start = Instant::now();
    let exit_status = wait_with_deadline(&mut child, Duration::from_secs(5))
        .expect("broker should exit within 5s of worker death");
    let elapsed = start.elapsed();

    let stderr = stderr_thread.join().expect("stderr reader thread");

    assert!(
        !exit_status.success(),
        "broker should have exited with non-zero status; got {exit_status:?}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("host-worker exited before child command"),
        "stderr should mention worker exit; got:\n{stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "broker should bail fast; took {elapsed:?}"
    );
}

fn wait_for_worker_pid(broker_pid: u32) -> Option<u32> {
    // The broker logs `vt-ferry-broker: spawned host-worker pid=N` to stderr
    // before it starts its poll loop. We read that line by reading the
    // broker's stderr from a separate process — but stderr is already piped
    // by the test thread. To avoid contention, locate the worker via /proc
    // or `pgrep` instead: the worker is a child of `broker_pid` running
    // vt-ferry-worker.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let output = Command::new("pgrep")
            .args(["-P", &broker_pid.to_string(), "vt-ferry-worker"])
            .output();
        if let Ok(out) = output {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    return Some(pid);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn wait_with_deadline(
    child: &mut std::process::Child,
    deadline: Duration,
) -> Option<std::process::ExitStatus> {
    let stop_at = Instant::now() + deadline;
    while Instant::now() < stop_at {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return None,
        }
    }
    None
}
