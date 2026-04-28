//! Spawn vt-ferry-host-worker with the IOSurface Mach ports already
//! `mach_ports_register`'d (so the child inherits them via
//! mach_ports_lookup) and a Unix socket path for the protocol stream.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::pool::WorkerIOSurfacePoolSpec;

const ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON: &str = "VT_FERRY_IOSURFACE_POOL_SPECS_JSON";

pub struct HostWorkerHandle {
    child: Child,
}

impl HostWorkerHandle {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Non-blocking poll for worker exit. Returns Ok(Some(status)) once the
    /// worker has exited, Ok(None) while it's still running, or Err on poll
    /// failure.
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
}

impl HostWorkerHandle {
    pub fn spawn_socket(
        worker_path: &Path,
        worker_specs: &[WorkerIOSurfacePoolSpec],
        socket_path: &Path,
    ) -> Result<Self> {
        validate_worker_path(worker_path)?;

        let mut command = Command::new(worker_path); // nosemgrep
        command.arg(socket_path);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());

        if let Some(log_path) = std::env::var_os("VT_FERRY_HOST_WORKER_STDERR_LOG") {
            let stderr = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .with_context(|| format!("open stderr log {:?}", log_path))?;
            command.stderr(Stdio::from(stderr));
        } else {
            command.stderr(Stdio::null());
        }

        let backend =
            std::env::var("VT_FERRY_HOST_WORKER_BACKEND").unwrap_or_else(|_| "vt-real".to_string());
        command.env("VT_FERRY_HOST_WORKER_BACKEND", &backend);

        if !worker_specs.is_empty() {
            let json = serde_json::to_string(worker_specs)
                .context("encode worker iosurface pool specs")?;
            command.env(ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON, json);
        } else {
            command.env_remove(ENV_VT_FERRY_IOSURFACE_POOL_SPECS_JSON);
        }

        let child = command
            .spawn()
            .with_context(|| format!("spawn host-worker {}", worker_path.display()))?;

        Ok(Self { child })
    }
}

impl Drop for HostWorkerHandle {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn validate_worker_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("worker path must be absolute (got {})", path.display());
    }
    let meta =
        std::fs::metadata(path).with_context(|| format!("stat worker path {}", path.display()))?;
    if !meta.is_file() {
        anyhow::bail!("worker path {} is not a regular file", path.display());
    }
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o111 == 0 {
        anyhow::bail!(
            "worker path {} is not executable (mode {:o})",
            path.display(),
            meta.permissions().mode()
        );
    }
    Ok(())
}
