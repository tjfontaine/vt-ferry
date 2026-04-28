use anyhow::{Context, Result};
use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct TcpBridgeHandle {
    local_addr: SocketAddr,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn bridge_trace_enabled() -> bool {
    std::env::var("VT_FERRY_TCP_BRIDGE_TRACE")
        .ok()
        .filter(|value| value != "0")
        .is_some()
}

impl TcpBridgeHandle {
    pub fn start(bind_addr: &str, port: u16, worker_socket: &Path) -> Result<Self> {
        let listener = TcpListener::bind((bind_addr, port))
            .with_context(|| format!("bind TCP bridge on {bind_addr}:{port}"))?;
        listener
            .set_nonblocking(true)
            .context("set TCP bridge listener nonblocking")?;
        let local_addr = listener.local_addr().context("read TCP bridge address")?;
        let worker_socket = worker_socket.to_path_buf();
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&running);
        let thread = thread::spawn(move || accept_loop(listener, worker_socket, thread_running));
        Ok(Self {
            local_addr,
            running,
            thread: Some(thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

impl Drop for TcpBridgeHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = TcpStream::connect(self.local_addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_loop(listener: TcpListener, worker_socket: PathBuf, running: Arc<AtomicBool>) {
    let trace = bridge_trace_enabled();
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !running.load(Ordering::SeqCst) {
                    drop(stream);
                    break;
                }
                if trace {
                    eprintln!("vt-ferry TCP bridge accepted peer={peer}");
                }
                let path = worker_socket.clone();
                thread::spawn(move || {
                    if let Err(err) = bridge_connection(stream, &path) {
                        eprintln!("vt-ferry TCP bridge connection error: {err}");
                    }
                });
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(err) => {
                eprintln!("vt-ferry TCP bridge accept error: {err}");
                break;
            }
        }
    }
}

fn bridge_connection(tcp: TcpStream, worker_socket: &Path) -> io::Result<()> {
    let trace = bridge_trace_enabled();
    tcp.set_nonblocking(false)?;
    tcp.set_nodelay(true)?;
    let unix = connect_worker_with_retry(worker_socket, Duration::from_secs(5))?;
    if trace {
        eprintln!(
            "vt-ferry TCP bridge connected worker_socket={}",
            worker_socket.display()
        );
    }

    let mut tcp_read = tcp.try_clone()?;
    let mut tcp_write = tcp;
    let mut unix_read = unix.try_clone()?;
    let mut unix_write = unix;

    let upstream = thread::spawn(move || {
        match io::copy(&mut tcp_read, &mut unix_write) {
            Ok(bytes) if trace => eprintln!("vt-ferry TCP bridge upstream eof bytes={bytes}"),
            Err(err) if trace => eprintln!("vt-ferry TCP bridge upstream error: {err}"),
            _ => {}
        }
        let _ = unix_write.shutdown(Shutdown::Write);
    });

    let downstream = thread::spawn(move || {
        match io::copy(&mut unix_read, &mut tcp_write) {
            Ok(bytes) if trace => eprintln!("vt-ferry TCP bridge downstream eof bytes={bytes}"),
            Err(err) if trace => eprintln!("vt-ferry TCP bridge downstream error: {err}"),
            _ => {}
        }
        let _ = tcp_write.shutdown(Shutdown::Write);
    });

    let _ = upstream.join();
    let _ = downstream.join();
    Ok(())
}

fn connect_worker_with_retry(worker_socket: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let start = Instant::now();
    let mut last_error = None;
    while start.elapsed() < timeout {
        match UnixStream::connect(worker_socket) {
            Ok(stream) => return Ok(stream),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::WouldBlock
                ) =>
            {
                last_error = Some(err);
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "worker not ready")))
}
