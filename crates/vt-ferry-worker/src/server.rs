use crate::backend::{Backend, BackendFactory};
use crate::probes;
use bytemuck::Zeroable;
use vt_ferry_protocol::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// The VMM bridge can forward multi-megabyte WRITE_BUFFER and READ_OUTPUT
// payloads for 1080p reference workloads, so the worker-side transport buffer
// must match that ceiling.
pub const VTF_TRANSPORT_STORAGE_CAPACITY: usize = 2 * 1024 * 1024;

fn worker_trace_enabled() -> bool {
    std::env::var("VT_FERRY_HOST_WORKER_TRACE")
        .map(|value| value != "0")
        .unwrap_or(false)
}

/// Drive one accepted connection until the client disconnects.
/// Owns its own `Box<dyn Backend>` produced by the factory at
/// connection-accept time, so two concurrent connections see two
/// fully isolated backend states (separate session id namespace,
/// separate buffer pools, separate output queues).
///
/// Held shared resources (e.g. launcher-registered IOSurfaces in
/// the vt-real factory) are reachable through the backend's own
/// `Arc`-wrapped fields; this fn doesn't see them.
fn handle_connection(mut client_stream: UnixStream, mut backend: Box<dyn Backend>, trace: bool) {
    if let Err(err) = client_stream.set_nonblocking(false) {
        if trace {
            eprintln!("worker-stream failed to set blocking mode: {err}");
        }
        return;
    }
    // Each accepted connection gets a fresh world — sessions,
    // buffer pools, allocated buffers, and output queues from
    // any previous client of THIS factory are absent because
    // `backend` is freshly created. The reset call is a
    // belt-and-braces no-op that costs nothing and lets the
    // factory's `create()` skip env probing if it wants.
    backend.reset_from_env();
    // Per-connection response buffer. Allocated once for the
    // connection's lifetime; reused across every dispatch (the
    // wire only sends `&res_payload_buf[..output_len]` so stale
    // bytes never leak across requests).
    let mut res_payload_buf = vec![0u8; VTF_TRANSPORT_STORAGE_CAPACITY];

    loop {
        let mut req_header_buf = [0u8; std::mem::size_of::<MessageHeader>()];
        if let Err(err) = client_stream.read_exact(&mut req_header_buf) {
            if trace {
                eprintln!("worker-stream read request header failed: {err}");
            }
            break;
        }

        let req_header: MessageHeader = bytemuck::pod_read_unaligned(&req_header_buf);
        if trace {
            eprintln!(
                "worker-stream request opcode={} request_id={} payload_len={}",
                req_header.opcode, req_header.request_id, req_header.payload_len
            );
        }

        if req_header.payload_len as usize > VTF_TRANSPORT_STORAGE_CAPACITY {
            if trace {
                eprintln!(
                    "worker-stream closing oversized request opcode={} payload_len={}",
                    req_header.opcode, req_header.payload_len
                );
            }
            break;
        }

        let mut req_payload_buf = vec![0u8; req_header.payload_len as usize];
        if req_header.payload_len > 0 {
            if let Err(err) = client_stream.read_exact(&mut req_payload_buf) {
                if trace {
                    eprintln!(
                        "worker-stream read request payload failed opcode={} request_id={}: {err}",
                        req_header.opcode, req_header.request_id
                    );
                }
                break;
            }
        }

        let mut res_header = MessageHeader::zeroed();

        probes::vt_ferry_probe_mailbox_request_begin(
            req_header.opcode as u64,
            req_header.request_id,
            req_header.payload_len as u64,
        );
        let output_len = match backend.dispatch(
            &req_header,
            &req_payload_buf,
            &mut res_header,
            &mut res_payload_buf,
        ) {
            Ok(len) => len,
            Err(_) => {
                if res_header.status == 0 {
                    res_header.status = vt_ferry_protocol::STATUS_UNSUPPORTED_OPCODE;
                }
                if trace {
                    eprintln!(
                        "worker-stream dispatch error opcode={} status={}",
                        req_header.opcode, res_header.status
                    );
                }
                0
            }
        };

        res_header.payload_len = output_len as u32;
        res_header.version = 1; // VTF_TRANSPORT_VERSION
        res_header.opcode = req_header.opcode;
        res_header.request_id = req_header.request_id;
        if trace {
            eprintln!(
                "worker-stream response opcode={} request_id={} status={} payload_len={}",
                res_header.opcode, res_header.request_id, res_header.status, output_len
            );
        }
        probes::vt_ferry_probe_mailbox_request_end(
            req_header.opcode as u64,
            req_header.request_id,
            res_header.status as u64,
            output_len as u64,
        );

        let res_header_bytes = bytemuck::bytes_of(&res_header);
        if let Err(err) = client_stream.write_all(res_header_bytes) {
            if trace {
                eprintln!(
                    "worker-stream write response header failed opcode={} request_id={}: {err}",
                    req_header.opcode, req_header.request_id
                );
            }
            break;
        }

        if output_len > 0 {
            if let Err(err) = client_stream.write_all(&res_payload_buf[..output_len]) {
                if trace {
                    eprintln!(
                        "worker-stream write response payload failed opcode={} request_id={}: {err}",
                        req_header.opcode, req_header.request_id
                    );
                }
                break;
            }
        }
    }
}

/// Listen on `socket_path` and serve one freshly-created backend
/// per accepted connection. Connections run on dedicated threads
/// so two concurrent guest processes can transcode through the
/// same broker without serializing at the worker.
///
/// Cross-connection sharing is limited to whatever the factory
/// itself wraps in `Arc` — for the vt-real factory, that's the
/// `IOSurfacePoolDirectory` of launcher-registered surfaces. Each
/// concurrent guest competes for those entries via take_matching;
/// if the launcher registered N surfaces of one shape, N concurrent
/// guests can use that shape; an N+1th guest sees its
/// `OP_CREATE_BUFFER_POOL` reject with `STATUS_UNSUPPORTED_CODEC_OR_FORMAT`
/// (existing fallback semantic).
pub fn run_server(
    socket_path: &str,
    factory: Arc<dyn BackendFactory>,
    running: Arc<AtomicBool>,
) -> std::io::Result<()> {
    // Unlink the old socket if it exists
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;

    let trace = worker_trace_enabled();

    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((client_stream, _)) => {
                let backend = factory.create();
                std::thread::spawn(move || {
                    handle_connection(client_stream, backend, trace);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(_e) => {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/vt-ferry-server.log")
                    .unwrap();
                use std::io::Write;
                writeln!(file, "accept error").unwrap();
                break;
            }
        }
    }

    let _ = std::fs::remove_file(socket_path);
    Ok(())
}
