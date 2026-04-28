//! vt-ferry-broker — launcher wrapper that registers host-side VT pool resources,
//! starts `vt-ferry-host-worker`, and then runs the child command, typically
//! `smolvm machine start ...` or `docker run ...`.
//!
//! The supported smolvm transport is protocol-over-vsock. The broker
//! publishes the worker's Unix socket via the generic smolvm vsock-port
//! contract — `SMOLVM_VSOCK_PORT_COUNT=1` plus `SMOLVM_VSOCK_PORT_0=<port>:<path>`
//! — before exec'ing smolvm; libkrun then bridges that vsock port to the
//! Unix socket inside the guest. TCP remains useful for Docker Desktop,
//! where host-CID vsock isn't exposed to containers.
//!
//! IOSurface Mach ports remain strictly a host-side host-worker concern. The
//! registered IOSurfaces are exposed to `vt-real` for matching pool shapes;
//! they are invisible to the guest.

use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum};
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

mod host_worker;
mod pool;
mod tcp_bridge;

const ENV_SMOLVM_VSOCK_PORT_COUNT: &str = "SMOLVM_VSOCK_PORT_COUNT";
const ENV_SMOLVM_VSOCK_PORT_0: &str = "SMOLVM_VSOCK_PORT_0";
const ENV_VT_FERRY_TRANSPORT: &str = "VT_FERRY_TRANSPORT";
const ENV_VT_FERRY_TCP_HOST: &str = "VT_FERRY_TCP_HOST";
const ENV_VT_FERRY_TCP_PORT: &str = "VT_FERRY_TCP_PORT";

const DEFAULT_VSOCK_PORT: u32 = 6600;

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum TransportMode {
    Vsock,
    Tcp,
}

#[derive(Parser)]
#[command(
    name = "vt-ferry-broker",
    about = "Allocate VT IOSurfaces and launch a child command"
)]
struct Args {
    /// Pool spec JSON: `{"guest_phys_addr":N, "slot_count":N, "width":W, "height":H, "pixel_format":F, "writable":bool}`.
    /// Repeat for multiple pools (max 3 due to TASK_PORT_REGISTER_MAX).
    #[arg(long = "pool", value_name = "JSON")]
    pools: Vec<String>,

    /// Path to vt-ferry-host-worker binary.
    #[arg(long = "host-worker", value_name = "PATH")]
    host_worker: PathBuf,

    /// Guest transport to provision: protocol over vsock, or TCP bridge for
    /// Docker Desktop.
    #[arg(long = "transport", value_enum, default_value = "vsock")]
    transport: TransportMode,

    /// Guest AF_VSOCK port for --transport vsock.
    #[arg(long = "vsock-port", default_value_t = DEFAULT_VSOCK_PORT)]
    vsock_port: u32,

    /// Host bind address for --transport tcp.
    #[arg(long = "tcp-bind", default_value = "127.0.0.1")]
    tcp_bind: String,

    /// Host bind port for --transport tcp. Use 0 to allocate a free port.
    #[arg(long = "tcp-port", default_value_t = 0)]
    tcp_port: u16,

    /// Guest/container hostname that should reach the TCP bridge.
    #[arg(long = "tcp-guest-host", default_value = "host.docker.internal")]
    tcp_guest_host: String,

    /// Keep broker-owned helpers alive after the child exits successfully.
    #[arg(long = "hold-after-child")]
    hold_after_child: bool,

    /// Runtime scratch directory. Defaults to a unique tmp dir.
    #[arg(long = "runtime-dir", value_name = "DIR")]
    runtime_dir: Option<PathBuf>,

    /// Command to exec after setup. Use `--` before it.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    child: Vec<String>,
}

fn wait_for_termination_signal() -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let handler_running = Arc::clone(&running);
    ctrlc::set_handler(move || {
        handler_running.store(false, Ordering::SeqCst);
    })
    .context("install termination handler")?;
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.pools.is_empty() {
        anyhow::bail!("at least one --pool is required");
    }
    if args.child.is_empty() {
        anyhow::bail!("missing child command — use `-- <cmd> [args...]` to specify");
    }

    let specs: Vec<pool::PoolSpec> = args
        .pools
        .iter()
        .enumerate()
        .map(|(i, json)| serde_json::from_str(json).with_context(|| format!("parse --pool[{}]", i)))
        .collect::<Result<_>>()?;

    let (entries, worker_specs) = pool::allocate_pools(&specs)?;

    pool::register_ports(&entries)?;

    let runtime_dir = args.runtime_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("vt-ferry-broker-{}", std::process::id()))
    });
    std::fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("create runtime dir {}", runtime_dir.display()))?;

    match args.transport {
        TransportMode::Tcp => {
            let socket_path = runtime_dir.join("w.sock");
            let _ = std::fs::remove_file(&socket_path);
            let mut worker = host_worker::HostWorkerHandle::spawn_socket(
                &args.host_worker,
                &worker_specs,
                &socket_path,
            )?;
            let worker_pid = worker.id();
            eprintln!("vt-ferry-broker: spawned host-worker pid={worker_pid}");
            let bridge =
                tcp_bridge::TcpBridgeHandle::start(&args.tcp_bind, args.tcp_port, &socket_path)?;
            let bridge_port = bridge.local_addr().port();

            let mut command = std::process::Command::new(&args.child[0]);
            command.args(&args.child[1..]);
            command.env_remove(ENV_SMOLVM_VSOCK_PORT_COUNT);
            command.env_remove(ENV_SMOLVM_VSOCK_PORT_0);
            command.env(ENV_VT_FERRY_TRANSPORT, "tcp");
            command.env(ENV_VT_FERRY_TCP_HOST, &args.tcp_guest_host);
            command.env(ENV_VT_FERRY_TCP_PORT, bridge_port.to_string());

            let mut child = command
                .spawn()
                .with_context(|| format!("spawn child command {}", args.child[0]))?;

            // Supervise both the worker and the child. If the worker exits
            // first, surface that — the child has no chance of succeeding
            // through a dead worker. If the child exits first, drop the
            // bridge/worker through the normal cleanup path.
            let (status, worker_exited_first) = loop {
                if let Some(status) = child
                    .try_wait()
                    .with_context(|| format!("poll child {}", args.child[0]))?
                {
                    break (status, false);
                }
                if let Some(worker_status) = worker
                    .try_wait()
                    .context("poll host-worker")?
                {
                    eprintln!(
                        "vt-ferry-broker: host-worker (pid={worker_pid}) exited unexpectedly with {worker_status}; killing child {}",
                        args.child[0]
                    );
                    let _ = child.kill();
                    let status = child
                        .wait()
                        .with_context(|| format!("wait child {}", args.child[0]))?;
                    break (status, true);
                }
                std::thread::sleep(Duration::from_millis(100));
            };

            if worker_exited_first {
                anyhow::bail!(
                    "host-worker exited before child command {} completed",
                    args.child[0]
                );
            }
            if !status.success() {
                anyhow::bail!("child command exited with status {status}");
            }
            if args.hold_after_child {
                wait_for_termination_signal()?;
            }
            drop(bridge);
            drop(worker);
            drop(entries);
            Ok(())
        }
        TransportMode::Vsock => {
            let socket_path = std::env::temp_dir().join(format!(
                "vt-ferry-vsock-{}-{}.sock",
                std::process::id(),
                args.vsock_port
            ));
            let _ = std::fs::remove_file(&socket_path);
            let worker = host_worker::HostWorkerHandle::spawn_socket(
                &args.host_worker,
                &worker_specs,
                &socket_path,
            )?;
            let worker_pid = worker.id();
            eprintln!("vt-ferry-broker: spawned host-worker pid={worker_pid}");

            // Publish the worker socket to smolvm via the generic
            // SMOLVM_VSOCK_PORT_* contract (count + indexed entries of
            // `<port>:<host_unix_socket_path>`). smolvm registers each
            // entry with libkrun as listen=false, so the guest dialing
            // the vsock port lands on this worker's Unix socket.
            // SAFETY: single-threaded, before exec.
            unsafe {
                std::env::set_var(ENV_SMOLVM_VSOCK_PORT_COUNT, "1");
                std::env::set_var(
                    ENV_SMOLVM_VSOCK_PORT_0,
                    format!("{}:{}", args.vsock_port, socket_path.display()),
                );
            }

            // The broker process becomes smolvm via execvp (PID preserved),
            // so we can't run any Rust code post-exec to supervise the
            // host-worker. The previous fork+kqueue supervisor signaled
            // the broker's pre-exec PID — but smolvm CLI calls
            // `manager.detach()` and exits, leaving the live VM at a
            // different PID, so the SIGTERM landed on a corpse. Worker
            // death is now detected guest-side: the guest-shim's stream
            // backend is poisoned on the first I/O failure (see
            // crates/vt-ferry-shim/src/transport.rs), and the run-command
            // path returns fast instead of replaying I/O against a dead
            // worker.
            let _ = worker_pid;
            std::mem::forget(entries);
            std::mem::forget(worker);

            let prog = CString::new(args.child[0].as_bytes())
                .context("child program name contains NUL")?;
            let argv: Vec<CString> = args
                .child
                .iter()
                .map(|s| CString::new(s.as_bytes()).context("argv contains NUL"))
                .collect::<Result<_>>()?;
            let argv_ptrs: Vec<*const libc::c_char> = argv
                .iter()
                .map(|c| c.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect();

            let rc = unsafe { libc::execvp(prog.as_ptr(), argv_ptrs.as_ptr()) };
            let err = std::io::Error::last_os_error();
            Err(anyhow!(
                "execvp({}) failed (rc={}): {}",
                args.child[0],
                rc,
                err
            ))
        }
    }
}
