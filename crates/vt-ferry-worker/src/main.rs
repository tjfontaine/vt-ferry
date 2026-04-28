usdt::dtrace_provider!("vt_ferry_worker.d");

mod backend;
#[cfg(target_os = "macos")]
mod iosurface_bridge;
#[cfg(target_os = "macos")]
mod iosurface_pool_directory;
mod mock;
mod probes;
mod server;
mod vt_real;
mod vt_stub;

use backend::{Backend, BackendFactory};
use std::env;
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Factory for the in-memory mock backend. Stateless — every
/// connection gets a fresh `MockBackend::new()`.
struct MockBackendFactory;
impl BackendFactory for MockBackendFactory {
    fn create(&self) -> Box<dyn Backend> {
        Box::new(mock::MockBackend::new())
    }
}

/// Factory for the no-op stub backend. Same shape as MockBackendFactory.
struct VtStubBackendFactory;
impl BackendFactory for VtStubBackendFactory {
    fn create(&self) -> Box<dyn Backend> {
        Box::new(vt_stub::VtStubBackend::new())
    }
}

/// Factory for the real Apple VT backend. Holds the shared
/// `IOSurfacePoolDirectory` (loaded once at process start from
/// launcher-registered Mach ports) so every per-connection
/// backend instance sees the same set of pre-allocated surfaces.
/// Concurrent guests competing for surfaces of the same shape
/// each lock the inner mutex around `take_matching`; that's the
/// single point of cross-connection contention in the design.
#[cfg(target_os = "macos")]
struct VtRealBackendFactory {
    iosurface_pools: Arc<Mutex<iosurface_pool_directory::IOSurfacePoolDirectory>>,
}

#[cfg(target_os = "macos")]
impl BackendFactory for VtRealBackendFactory {
    fn create(&self) -> Box<dyn Backend> {
        Box::new(
            vt_real::VtRealBackend::new()
                .with_iosurface_pools_shared(self.iosurface_pools.clone()),
        )
    }
}

#[cfg(not(target_os = "macos"))]
struct VtRealBackendFactory;

#[cfg(not(target_os = "macos"))]
impl BackendFactory for VtRealBackendFactory {
    fn create(&self) -> Box<dyn Backend> {
        Box::new(vt_real::VtRealBackend::new())
    }
}

fn select_backend_factory() -> Arc<dyn BackendFactory> {
    if let Ok(val) = env::var("VT_FERRY_HOST_WORKER_BACKEND") {
        if val == "mock" {
            return Arc::new(MockBackendFactory);
        }
        if val == "vt-stub" {
            return Arc::new(VtStubBackendFactory);
        }
        if val == "vt-real" {
            #[cfg(target_os = "macos")]
            {
                // Pick up IOSurfaces the launcher registered via
                // mach_ports_register before posix_spawn, then
                // index them by the shapes declared in
                // VT_FERRY_IOSURFACE_POOL_SPECS_JSON so vt-real
                // can match incoming CREATE_BUFFER_POOL requests
                // to a launcher-provided IOSurface and take the
                // zero-copy path. This loading happens ONCE per
                // worker process; the resulting directory is
                // shared across all per-connection backend
                // instances via Arc<Mutex<...>>.
                let mut registry =
                    iosurface_bridge::IOSurfaceRegistry::load_from_registered_ports();
                let directory =
                    iosurface_pool_directory::IOSurfacePoolDirectory::load(&mut registry);
                if !directory.is_empty() {
                    eprintln!(
                        "vt-real: IOSurfacePoolDirectory loaded with {} entry/entries; \
                         zero-copy encode is available for matching pool shapes",
                        directory.len()
                    );
                }
                return Arc::new(VtRealBackendFactory {
                    iosurface_pools: Arc::new(Mutex::new(directory)),
                });
            }
            #[cfg(not(target_os = "macos"))]
            return Arc::new(VtRealBackendFactory);
        }
    }
    // Default to mock
    Arc::new(MockBackendFactory)
}

fn main() {
    probes::register_dtrace_probes();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} <socket-path>", args[0]);
        std::process::exit(2);
    }

    let factory = select_backend_factory();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    if let Err(e) = server::run_server(&args[1], factory, running) {
        eprintln!("host-worker transport error: {}", e);
        std::process::exit(1);
    }
}
