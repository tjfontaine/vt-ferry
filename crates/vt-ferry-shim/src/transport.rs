use bytemuck::Zeroable;
use lazy_static::lazy_static;
use vt_ferry_protocol::*;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const VSOCK_PORT_DEFAULT: u32 = 6600;
const TCP_HOST_DEFAULT: &str = "127.0.0.1";
const TCP_PORT_DEFAULT: u16 = 6600;
const WRITE_BUFFER_CHUNK_BYTES: usize = 1024 * 1024;
/// Default per-request socket timeout for guest → worker round trips.
/// Override with VT_FERRY_TRANSPORT_TIMEOUT_MS=<ms>; 0 disables the timeout
/// (matches the previous unbounded behavior). 60s is well above the slowest
/// observed control-plane operation (DRAIN with a backed-up encode queue),
/// so a real timeout means the worker is hung or gone, not slow.
const TRANSPORT_TIMEOUT_MS_DEFAULT: u64 = 60_000;

/// How often the heartbeat thread issues `OP_PING` to the worker. Override
/// with `VT_FERRY_HEARTBEAT_INTERVAL_MS=<ms>`; 0 disables the heartbeat
/// thread entirely. The default is conservative — it's not in the hot
/// path of any encode call, just catches a worker that hung while the
/// guest sat idle.
const HEARTBEAT_INTERVAL_MS_DEFAULT: u64 = 10_000;

lazy_static! {
    static ref TRANSPORT_BACKEND: Mutex<Option<StreamBackend>> = Mutex::new(None);
    static ref DEFERRED_OUTPUT_RELEASES: Mutex<Vec<u64>> = Mutex::new(Vec::new());
}

/// Set the first time a backend is poisoned by a transport-level error.
/// Once set, `ensure_backend` refuses to redial — every subsequent call
/// short-circuits to `STATUS_INTERNAL_FAILURE`.
///
/// Why not silently reconnect? The protocol is stateful: sessions,
/// buffer-pool slots, allocated buffers, and in-flight encodes all live
/// on the worker. A reconnect would land on either a fresh worker (state
/// gone) or, worse, a respawned worker that happened to bind the same
/// socket path (state gone *and* the guest doesn't know it). Either way,
/// the guest's handles are stale, and a "successful" follow-up call
/// would silently produce garbage. Failing terminally is the safer
/// default — callers tear down and the next `is_enabled()`-gated retry
/// happens at a layer that knows to rebuild the world.
static TRANSPORT_KILLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn stream_trace_enabled() -> bool {
    std::env::var("VT_FERRY_STREAM_TRACE")
        .ok()
        .filter(|value| value != "0")
        .is_some()
}

fn transport_timeout_ms() -> u64 {
    std::env::var("VT_FERRY_TRANSPORT_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(TRANSPORT_TIMEOUT_MS_DEFAULT)
}

fn heartbeat_interval_ms() -> u64 {
    std::env::var("VT_FERRY_HEARTBEAT_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(HEARTBEAT_INTERVAL_MS_DEFAULT)
}

/// Apply SO_RCVTIMEO and SO_SNDTIMEO to a connected socket fd. A zero
/// timeout clears the option (unlimited).
fn apply_socket_timeouts(fd: libc::c_int, timeout_ms: u64) -> std::io::Result<()> {
    let tv = libc::timeval {
        tv_sec: (timeout_ms / 1000) as libc::time_t,
        tv_usec: ((timeout_ms % 1000) * 1000) as libc::suseconds_t,
    };
    for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        let rc = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                option,
                &tv as *const libc::timeval as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

struct StreamBackend {
    stream: File,
    request_id: u64,
    /// Set to true when a socket-level write/read error fires. A poisoned
    /// backend never sends another request — `send_request_bytes` clears it
    /// from `TRANSPORT_BACKEND` and returns -12902, so the guest fails fast
    /// instead of dragging out repeated I/O against a half-dead worker.
    poisoned: bool,
    /// Timestamp of the most recent successful round trip (request issued
    /// + response read). Used by the heartbeat thread to skip the
    /// `OP_PING` probe when the connection has been busy.
    last_activity: Instant,
}

#[derive(Debug, PartialEq)]
pub enum StreamSelection {
    Tcp { host: String, port: u16 },
    Vsock { port: u32 },
}

impl StreamBackend {
    fn connect_tcp(host: &str, port: u16) -> Option<Self> {
        let stream = connect_tcp_host(host, port).ok()?;
        Self::wrap("tcp", stream, &format!("{host}:{port}"))
    }

    fn connect_vsock(port: u32) -> Option<Self> {
        let stream = connect_vsock_host(port).ok()?;
        Self::wrap("vsock", stream, &format!("port={port}"))
    }

    fn wrap(kind: &str, stream: File, descriptor: &str) -> Option<Self> {
        let timeout_ms = transport_timeout_ms();
        if timeout_ms > 0 {
            if let Err(err) = apply_socket_timeouts(stream.as_raw_fd(), timeout_ms) {
                if stream_trace_enabled() {
                    eprintln!("vt-ferry-stream apply_socket_timeouts {kind} {descriptor}: {err}");
                }
                return None;
            }
        }
        if stream_trace_enabled() {
            eprintln!(
                "vt-ferry-stream connected {kind} {descriptor} timeout_ms={timeout_ms}"
            );
        }
        Some(Self {
            stream,
            request_id: 1,
            poisoned: false,
            last_activity: Instant::now(),
        })
    }

    #[cfg(test)]
    fn from_test_file(stream: File) -> Self {
        Self {
            stream,
            request_id: 1,
            poisoned: false,
            last_activity: Instant::now(),
        }
    }

    fn send_request_dynamic(&mut self, opcode: u16, payload: &[u8]) -> Result<Vec<u8>, i32> {
        let trace = stream_trace_enabled();
        let request_id = self.request_id;
        self.request_id += 1;

        let mut header = MessageHeader::zeroed();
        header.version = VTF_TRANSPORT_VERSION;
        header.opcode = opcode;
        header.request_id = request_id;
        header.payload_len = payload.len() as u32;

        if trace {
            eprintln!(
                "vt-ferry-stream request opcode={opcode} request_id={request_id} payload_len={}",
                payload.len()
            );
        }

        self.stream
            .write_all(bytemuck::bytes_of(&header))
            .map_err(|err| {
                eprintln!(
                    "vt-ferry-stream: worker connection lost on write-header opcode={opcode} request_id={request_id}: {err}"
                );
                self.poisoned = true;
                STATUS_INTERNAL_FAILURE as i32
            })?;
        if !payload.is_empty() {
            self.stream.write_all(payload).map_err(|err| {
                eprintln!(
                    "vt-ferry-stream: worker connection lost on write-payload opcode={opcode} request_id={request_id}: {err}"
                );
                self.poisoned = true;
                STATUS_INTERNAL_FAILURE as i32
            })?;
        }

        let mut response_header = [0u8; std::mem::size_of::<MessageHeader>()];
        self.stream.read_exact(&mut response_header).map_err(|err| {
            eprintln!(
                "vt-ferry-stream: worker connection lost on read-header opcode={opcode} request_id={request_id}: {err}"
            );
            self.poisoned = true;
            STATUS_INTERNAL_FAILURE as i32
        })?;
        let response: MessageHeader = bytemuck::pod_read_unaligned(&response_header);
        if trace {
            eprintln!(
                "vt-ferry-stream response opcode={} request_id={} status={} payload_len={}",
                response.opcode, response.request_id, response.status, response.payload_len
            );
        }
        let mut response_payload = vec![0u8; response.payload_len as usize];
        if response.payload_len != 0 {
            self.stream
                .read_exact(&mut response_payload)
                .map_err(|err| {
                    eprintln!(
                        "vt-ferry-stream: worker connection lost on read-payload opcode={opcode} request_id={request_id}: {err}"
                    );
                    self.poisoned = true;
                    STATUS_INTERNAL_FAILURE as i32
                })?;
        }
        // Treat a clean status response as evidence the worker is alive,
        // even when the application-level status is non-OK — the round
        // trip itself is what we care about for heartbeat purposes.
        self.last_activity = Instant::now();
        if response.status != STATUS_OK {
            return Err(response.status as i32);
        }
        Ok(response_payload)
    }
}

fn connect_tcp_host(host: &str, port: u16) -> std::io::Result<File> {
    use std::os::fd::{FromRawFd, IntoRawFd};

    let stream = std::net::TcpStream::connect((host, port))?;
    stream.set_nodelay(true)?;
    Ok(unsafe { File::from_raw_fd(stream.into_raw_fd()) })
}

#[cfg(target_os = "linux")]
fn connect_vsock_host(port: u32) -> std::io::Result<File> {
    use std::mem;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const AF_VSOCK: libc::c_int = 40;
    const HOST_CID: u32 = 2;

    #[repr(C)]
    struct SockAddrVm {
        svm_family: libc::sa_family_t,
        svm_reserved1: u16,
        svm_port: u32,
        svm_cid: u32,
        svm_zero: [u8; 4],
    }

    unsafe {
        let fd = libc::socket(AF_VSOCK, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let addr = SockAddrVm {
            svm_family: AF_VSOCK as libc::sa_family_t,
            svm_reserved1: 0,
            svm_port: port,
            svm_cid: HOST_CID,
            svm_zero: [0; 4],
        };
        if libc::connect(
            fd.as_raw_fd(),
            &addr as *const SockAddrVm as *const libc::sockaddr,
            mem::size_of::<SockAddrVm>() as libc::socklen_t,
        ) < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(File::from(fd))
    }
}

#[cfg(not(target_os = "linux"))]
fn connect_vsock_host(_port: u32) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "AF_VSOCK transport is Linux-only",
    ))
}

pub fn is_enabled() -> bool {
    stream_selection_from_env().is_some()
}

fn ensure_backend(backend: &mut Option<StreamBackend>) {
    if backend.is_some() {
        return;
    }
    if TRANSPORT_KILLED.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    *backend = open_backend();
    if backend.is_some() {
        ensure_heartbeat_thread();
    }
}

/// Spawn the heartbeat probe thread the first time we successfully
/// connect a backend. Honors `VT_FERRY_HEARTBEAT_INTERVAL_MS=0` to disable.
fn ensure_heartbeat_thread() {
    static SPAWNED: OnceLock<()> = OnceLock::new();
    SPAWNED.get_or_init(|| {
        let interval_ms = heartbeat_interval_ms();
        if interval_ms == 0 {
            if stream_trace_enabled() {
                eprintln!("vt-ferry-stream: heartbeat disabled");
            }
            return;
        }
        let interval = Duration::from_millis(interval_ms);
        // The probe is a regular request, so we deliberately set the
        // staleness threshold to half the interval — that way a
        // moderately busy connection (one round-trip per interval) skips
        // the probe entirely while a truly idle connection still gets
        // pinged within roughly `interval` of going quiet.
        let staleness = interval / 2;
        std::thread::Builder::new()
            .name("vt-ferry-heartbeat".into())
            .spawn(move || heartbeat_loop(interval, staleness))
            .ok();
    });
}

fn heartbeat_loop(interval: Duration, staleness: Duration) {
    let trace = stream_trace_enabled();
    loop {
        std::thread::sleep(interval);
        let mut guard = match TRANSPORT_BACKEND.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let needs_probe = match guard.as_ref() {
            Some(backend) => {
                !backend.poisoned && backend.last_activity.elapsed() >= staleness
            }
            None => false,
        };
        if !needs_probe {
            continue;
        }
        // Issue OP_PING. The result is mostly cosmetic; what matters is
        // whether the round trip completes — failure flips
        // `backend.poisoned` via send_request_dynamic, and the next
        // send_request_bytes call drops the dead backend.
        if let Some(backend) = guard.as_mut() {
            if trace {
                eprintln!("vt-ferry-stream: heartbeat probe");
            }
            let _ = backend.send_request_dynamic(OP_PING, &[]);
            let poisoned = backend.poisoned;
            if poisoned {
                *guard = None;
                TRANSPORT_KILLED.store(true, std::sync::atomic::Ordering::Release);
                if trace {
                    eprintln!("vt-ferry-stream: heartbeat detected dead worker; backend dropped");
                }
            }
        }
    }
}

fn open_backend() -> Option<StreamBackend> {
    match stream_selection_from_env()? {
        StreamSelection::Tcp { host, port } => StreamBackend::connect_tcp(&host, port),
        StreamSelection::Vsock { port } => StreamBackend::connect_vsock(port),
    }
}

fn stream_selection_from_env() -> Option<StreamSelection> {
    match std::env::var("VT_FERRY_TRANSPORT").ok().as_deref() {
        Some("tcp") => return Some(tcp_endpoint_from_env()),
        Some("vsock") => {
            return Some(StreamSelection::Vsock {
                port: vsock_port_from_env().unwrap_or(VSOCK_PORT_DEFAULT),
            });
        }
        _ => {}
    }

    if std::env::var("VT_FERRY_TCP_PORT").is_ok() {
        return Some(tcp_endpoint_from_env());
    }

    vsock_port_from_env().map(|port| StreamSelection::Vsock { port })
}

fn tcp_endpoint_from_env() -> StreamSelection {
    let host = std::env::var("VT_FERRY_TCP_HOST").unwrap_or_else(|_| TCP_HOST_DEFAULT.to_string());
    let port = std::env::var("VT_FERRY_TCP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(TCP_PORT_DEFAULT);
    StreamSelection::Tcp { host, port }
}

fn vsock_port_from_env() -> Option<u32> {
    if let Ok(port) = std::env::var("VT_FERRY_VSOCK_PORT") {
        return port.parse().ok();
    }
    None
}

fn send_request_bytes(opcode: u16, payload: &[u8]) -> Result<Vec<u8>, i32> {
    let mut guard = TRANSPORT_BACKEND.lock().unwrap();
    ensure_backend(&mut guard);
    let result = match guard.as_mut() {
        Some(backend) => backend.send_request_dynamic(opcode, payload),
        None => return Err(-12902),
    };
    // A socket I/O error sets `poisoned`. Drop the backend, then mark
    // the transport killed so subsequent calls don't redial — see the
    // `TRANSPORT_KILLED` doc comment for why reconnecting is unsafe.
    if guard.as_ref().map(|b| b.poisoned).unwrap_or(false) {
        *guard = None;
        TRANSPORT_KILLED.store(true, std::sync::atomic::Ordering::Release);
    }
    result
}

pub fn send_request<P: bytemuck::Pod, R: bytemuck::Pod>(
    opcode: u16,
    payload: &P,
) -> Result<R, i32> {
    let reply_payload_bytes = send_request_bytes(opcode, bytemuck::bytes_of(payload))?;
    if reply_payload_bytes.len() < std::mem::size_of::<R>() {
        return Err(-12902);
    }
    Ok(bytemuck::pod_read_unaligned(
        &reply_payload_bytes[..std::mem::size_of::<R>()],
    ))
}

pub fn send_request_dynamic<P: bytemuck::Pod>(opcode: u16, payload: &P) -> Result<Vec<u8>, i32> {
    send_request_bytes(opcode, bytemuck::bytes_of(payload))
}

pub fn create_session(payload: &CreateSessionPayload) -> Result<u64, i32> {
    let reply: CreateSessionReply = send_request(OP_CREATE_SESSION, payload)?;
    Ok(reply.session_id)
}

pub fn destroy_session(session_id: u64) -> Result<(), i32> {
    if drop_deferred_output_releases_on_destroy_enabled() {
        clear_deferred_output_releases();
    } else {
        flush_deferred_output_releases()?;
    }
    let payload = DestroySessionPayload { session_id };
    let _ = send_request_dynamic(OP_DESTROY_SESSION, &payload)?;
    Ok(())
}

/// Tell the worker to drop a `VtRealPool` it created earlier and
/// release any IOSurface directory entry that pool claimed.
/// Called from the guest's `CVPixelBufferPool` finalizer when the
/// caller `CFRelease`s the pool. Idempotent on the worker side —
/// destroy on an unknown pool_id is treated as success — so we
/// don't need to track double-release on the guest.
pub fn destroy_buffer_pool(pool_id: u64) -> Result<(), i32> {
    let payload = DestroyBufferPoolPayload { pool_id };
    let _ = send_request_dynamic(OP_DESTROY_BUFFER_POOL, &payload)?;
    Ok(())
}

pub fn create_buffer_pool(payload: &CreateBufferPoolPayload) -> Result<CreateBufferPoolReply, i32> {
    let reply_payload_bytes =
        send_request_bytes(OP_CREATE_BUFFER_POOL, bytemuck::bytes_of(payload))?;
    if reply_payload_bytes.len() >= std::mem::size_of::<CreateBufferPoolReply>() {
        return Ok(bytemuck::pod_read_unaligned(
            &reply_payload_bytes[..std::mem::size_of::<CreateBufferPoolReply>()],
        ));
    }
    if reply_payload_bytes.len() >= std::mem::size_of::<CreateBufferPoolCompactReply>() {
        let compact: CreateBufferPoolCompactReply = bytemuck::pod_read_unaligned(
            &reply_payload_bytes[..std::mem::size_of::<CreateBufferPoolCompactReply>()],
        );
        if compact.format != CREATE_BUFFER_POOL_COMPACT_REPLY_FORMAT {
            return Err(STATUS_INTERNAL_FAILURE as i32);
        }
        let mut reply = CreateBufferPoolReply::zeroed();
        reply.pool_id = compact.pool_id;
        reply.width = compact.width;
        reply.height = compact.height;
        reply.pixel_format = compact.pixel_format;
        reply.slot_count = compact.slot_count;
        reply.buffer_region_size = compact.buffer_region_size;
        reply.host_backing_kind = compact.host_backing_kind;
        reply.layout = compact.layout;
        reply.buffer_leases = compact.buffer_leases;
        return Ok(reply);
    }
    Err(STATUS_INTERNAL_FAILURE as i32)
}

pub fn alloc_buffer(payload: &AllocBufferPayload) -> Result<AllocBufferReply, i32> {
    send_request(OP_ALLOC_BUFFER, payload)
}

pub fn encode_frame(payload: &EncodeFramePayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_ENCODE_FRAME, payload)?;
    Ok(())
}

pub fn encode_frame_batch(payload: &EncodeFrameBatchPayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_ENCODE_FRAME_BATCH, payload)?;
    Ok(())
}

pub fn set_property(payload: &SetPropertyPayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_SET_PROPERTY, payload)?;
    Ok(())
}

pub fn dequeue_output(payload: &DequeueOutputPayload) -> Result<DequeueOutputReply, i32> {
    send_request(OP_DEQUEUE_OUTPUT, payload)
}

pub fn dequeue_output_batch(
    payload: &DequeueOutputBatchPayload,
) -> Result<DequeueOutputBatchReply, i32> {
    send_request(OP_DEQUEUE_OUTPUT_BATCH, payload)
}

pub fn read_output(payload: &ReadOutputPayload) -> Result<Vec<u8>, i32> {
    let bytes = send_request_dynamic(OP_READ_OUTPUT, payload)?;
    if bytes.len() < std::mem::size_of::<ReadOutputReply>() {
        return Err(-12902);
    }
    Ok(bytes)
}

pub fn write_buffer(buffer_id: u64, generation: u64, bytes: &[u8]) -> Result<(), i32> {
    let header_len = std::mem::size_of::<WriteBufferPayload>();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let chunk_len = (bytes.len() - offset).min(WRITE_BUFFER_CHUNK_BYTES);
        let payload = WriteBufferPayload {
            buffer_id,
            generation,
            offset: offset as u32,
            length: chunk_len as u32,
        };
        let mut request = Vec::with_capacity(header_len + chunk_len);
        request.extend_from_slice(bytemuck::bytes_of(&payload));
        request.extend_from_slice(&bytes[offset..offset + chunk_len]);
        let _ = send_request_bytes(OP_WRITE_BUFFER, &request)?;
        offset += chunk_len;
    }
    Ok(())
}

/// Chunked counterpart to `write_buffer`. Reads `total_len` bytes
/// from a worker-side pool slot via repeated OP_READ_BUFFER calls
/// (each capped at `WRITE_BUFFER_CHUNK_BYTES` so the response fits
/// the transport budget). Used by the pool-bound decode path to
/// fetch decoded pixels >720p — the inline `read_decoded_frame`
/// caps at 1.5 MiB / single response, while this routes through
/// the existing chunked READ_BUFFER op.
pub fn read_buffer(
    buffer_id: u64,
    generation: u64,
    total_len: usize,
) -> Result<Vec<u8>, i32> {
    let mut out = Vec::with_capacity(total_len);
    let mut offset = 0usize;
    let header_len = std::mem::size_of::<ReadBufferReply>();
    while offset < total_len {
        let chunk_len = (total_len - offset).min(WRITE_BUFFER_CHUNK_BYTES);
        let payload = ReadBufferPayload {
            buffer_id,
            generation,
            offset: offset as u32,
            length: chunk_len as u32,
        };
        let bytes = send_request_dynamic(OP_READ_BUFFER, &payload)?;
        if bytes.len() < header_len + chunk_len {
            return Err(-12902);
        }
        out.extend_from_slice(&bytes[header_len..header_len + chunk_len]);
        offset += chunk_len;
    }
    Ok(out)
}

pub fn release_output(output_id: u64) -> Result<(), i32> {
    if defer_output_releases_enabled() {
        let should_flush = {
            let mut releases = DEFERRED_OUTPUT_RELEASES.lock().unwrap();
            releases.push(output_id);
            releases.len() >= output_release_batch_size()
        };
        if should_flush {
            flush_deferred_output_releases()?;
        }
        return Ok(());
    }
    release_output_now(output_id)
}

fn release_output_now(output_id: u64) -> Result<(), i32> {
    let payload = ReleaseOutputPayload { output_id };
    let _ = send_request_dynamic(OP_RELEASE_OUTPUT, &payload)?;
    Ok(())
}

// ---------- decode-side transport wrappers ----------
//
// Mirror the encode-side helpers (`encode_frame`, `dequeue_output`,
// `read_output`, etc.) for the decode opcodes that landed in
// Phase 10. The public guest-shim `VTDecompressionSession*`
// entrypoints in `videotoolbox.rs` build on these. Each wrapper
// is a thin protocol shim — no shim-side state machine here.

pub fn set_decode_format(payload: &SetDecodeFormatPayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_SET_DECODE_FORMAT, payload)?;
    Ok(())
}

/// Send `OP_ENQUEUE_ENCODED_FRAME` with the encoded bytes inline
/// in the variable-length tail. The header's `encoded_size` field
/// must equal `encoded_bytes.len()` — caller is responsible for
/// setting it (the header is otherwise opaque to this wrapper).
///
/// For frames that exceed `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`,
/// the caller should use `enqueue_encoded_frame_chunked` instead;
/// this function returns -12902 if the inline cap is exceeded.
pub fn enqueue_encoded_frame(
    payload: &EnqueueEncodedFramePayload,
    encoded_bytes: &[u8],
) -> Result<(), i32> {
    if payload.encoded_size as usize != encoded_bytes.len() {
        eprintln!(
            "guest-shim transport: enqueue_encoded_frame size mismatch — \
             header says {}, slice is {}",
            payload.encoded_size,
            encoded_bytes.len()
        );
        return Err(-12902);
    }
    let header_len = std::mem::size_of::<EnqueueEncodedFramePayload>();
    let mut request = Vec::with_capacity(header_len + encoded_bytes.len());
    request.extend_from_slice(bytemuck::bytes_of(payload));
    request.extend_from_slice(encoded_bytes);
    let _ = send_request_bytes(OP_ENQUEUE_ENCODED_FRAME, &request)?;
    Ok(())
}

/// Chunked counterpart to `enqueue_encoded_frame` for frames
/// larger than `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`. Sends
/// repeated `OP_ENQUEUE_ENCODED_FRAME_CHUNK` ops, each carrying
/// up to `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES` bytes; the
/// worker accumulates and dispatches VT decode on the final
/// chunk. The PTS / duration values come along on the head chunk
/// (chunk_offset == 0); subsequent chunks carry those fields too
/// but the worker ignores them.
///
/// The caller passes a "head" payload with `session_id`, timing,
/// and `flags` set; `chunk_offset`, `chunk_length`,
/// `total_encoded_size`, `is_final_chunk`, and `_reserved` on
/// the input are ignored — this wrapper computes them per-chunk.
pub fn enqueue_encoded_frame_chunked(
    head_payload: &EnqueueEncodedFrameChunkPayload,
    encoded_bytes: &[u8],
) -> Result<(), i32> {
    if encoded_bytes.is_empty() {
        return Err(-12902);
    }
    let total = encoded_bytes.len();
    if total > u32::MAX as usize {
        return Err(-12902);
    }
    let header_len = std::mem::size_of::<EnqueueEncodedFrameChunkPayload>();
    let chunk_size_cap =
        vt_ferry_protocol::VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES as usize;
    let mut offset = 0usize;
    while offset < total {
        let chunk_len = (total - offset).min(chunk_size_cap);
        let is_final = offset + chunk_len == total;
        let mut payload = *head_payload;
        payload.chunk_offset = offset as u32;
        payload.chunk_length = chunk_len as u32;
        payload.total_encoded_size = total as u32;
        payload.is_final_chunk = if is_final { 1 } else { 0 };
        payload._reserved = 0;
        let mut request = Vec::with_capacity(header_len + chunk_len);
        request.extend_from_slice(bytemuck::bytes_of(&payload));
        request.extend_from_slice(&encoded_bytes[offset..offset + chunk_len]);
        let _ = send_request_bytes(OP_ENQUEUE_ENCODED_FRAME_CHUNK, &request)?;
        offset += chunk_len;
    }
    Ok(())
}

pub fn dequeue_decoded_frame(
    payload: &DequeueDecodedFramePayload,
) -> Result<DequeueDecodedFrameReply, i32> {
    send_request(OP_DEQUEUE_DECODED_FRAME, payload)
}

/// Read decoded pixel bytes inline. Returns the raw response
/// bytes — caller slices off the `ReadDecodedFrameReply` header
/// and consumes the trailing pixel data. Mirrors `read_output`'s
/// shape so the encode and decode read paths are interchangeable.
pub fn read_decoded_frame(payload: &ReadDecodedFramePayload) -> Result<Vec<u8>, i32> {
    let bytes = send_request_dynamic(OP_READ_DECODED_FRAME, payload)?;
    if bytes.len() < std::mem::size_of::<ReadDecodedFrameReply>() {
        return Err(-12902);
    }
    Ok(bytes)
}

pub fn release_decoded_frame(payload: &ReleaseDecodedFramePayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_RELEASE_DECODED_FRAME, payload)?;
    Ok(())
}

/// Switch a decode session to the zero-copy chunked output path.
/// After this op succeeds, `dequeue_decoded_frame` returns a
/// reply with `buffer_host_id = output_id` (signaling chunked
/// mode), and the guest fetches pixel bytes via repeated
/// `read_decoded_frame_chunk` calls instead of the single-shot
/// `read_decoded_frame`. Used for >720p frames whose total size
/// exceeds the inline transport budget. The `pool_id` field of
/// `BindDecodeOutputPoolPayload` is vestigial and must be zero.
pub fn bind_decode_output_pool(payload: &BindDecodeOutputPoolPayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_BIND_DECODE_OUTPUT_POOL, payload)?;
    Ok(())
}

/// Chunked counterpart to `read_decoded_frame`. Reads `total_len`
/// bytes from a queued decoded frame's `CVImageBuffer` via
/// repeated `OP_READ_DECODED_FRAME_CHUNK` calls (each capped at
/// `WRITE_BUFFER_CHUNK_BYTES` so the response fits the transport
/// budget). The worker caches the canonical-layout flatten on
/// the first call and slices subsequent chunks out of that cache,
/// so the per-frame cost is one host-side memcpy plus N chunk-
/// sized memcpys regardless of how many calls the guest issues.
pub fn read_decoded_frame_chunk(
    session_id: u64,
    output_id: u64,
    total_len: usize,
) -> Result<Vec<u8>, i32> {
    let mut out = Vec::with_capacity(total_len);
    let mut offset = 0usize;
    let header_len = std::mem::size_of::<ReadDecodedFrameChunkReply>();
    while offset < total_len {
        let chunk_len = (total_len - offset).min(WRITE_BUFFER_CHUNK_BYTES);
        let payload = ReadDecodedFrameChunkPayload {
            session_id,
            output_id,
            offset: offset as u32,
            length: chunk_len as u32,
        };
        let bytes = send_request_dynamic(OP_READ_DECODED_FRAME_CHUNK, &payload)?;
        if bytes.len() < header_len + chunk_len {
            return Err(-12902);
        }
        out.extend_from_slice(&bytes[header_len..header_len + chunk_len]);
        offset += chunk_len;
    }
    Ok(out)
}

pub fn flush_deferred_output_releases() -> Result<(), i32> {
    let releases = {
        let mut guard = DEFERRED_OUTPUT_RELEASES.lock().unwrap();
        if guard.is_empty() {
            return Ok(());
        }
        std::mem::take(&mut *guard)
    };
    for chunk in releases.chunks(VTF_TRANSPORT_MAX_OUTPUT_BATCH) {
        release_output_batch_now(chunk)?;
    }
    Ok(())
}

fn release_output_batch_now(output_ids: &[u64]) -> Result<(), i32> {
    if output_ids.is_empty() {
        return Ok(());
    }
    let mut payload = ReleaseOutputBatchPayload::zeroed();
    payload.output_count = output_ids.len() as u32;
    payload.output_ids[..output_ids.len()].copy_from_slice(output_ids);
    match send_request_dynamic(OP_RELEASE_OUTPUT_BATCH, &payload) {
        Ok(_) => Ok(()),
        Err(status) if status == STATUS_UNSUPPORTED_OPCODE as i32 => {
            for output_id in output_ids {
                release_output_now(*output_id)?;
            }
            Ok(())
        }
        Err(status) => Err(status),
    }
}

fn clear_deferred_output_releases() {
    DEFERRED_OUTPUT_RELEASES.lock().unwrap().clear();
}

fn defer_output_releases_enabled() -> bool {
    std::env::var("VT_FERRY_DEFER_OUTPUT_RELEASES")
        .map(|value| value != "0")
        .unwrap_or(false)
}

fn output_release_batch_size() -> usize {
    std::env::var("VT_FERRY_OUTPUT_RELEASE_BATCH_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(VTF_TRANSPORT_MAX_OUTPUT_BATCH)
        .clamp(1, VTF_TRANSPORT_MAX_OUTPUT_BATCH)
}

fn drop_deferred_output_releases_on_destroy_enabled() -> bool {
    std::env::var("VT_FERRY_DROP_DEFERRED_OUTPUT_RELEASES_ON_DESTROY")
        .map(|value| value != "0")
        .unwrap_or(false)
}

pub fn recycle_buffer(payload: &RecycleBufferPayload) -> Result<(), i32> {
    let _ = send_request_dynamic(OP_RECYCLE_BUFFER, payload)?;
    Ok(())
}

pub fn drain(session_id: u64) -> Result<DrainReply, i32> {
    let payload = DrainPayload { session_id };
    send_request(OP_DRAIN, &payload)
}

#[cfg(test)]
pub fn reset_backend_for_tests() {
    *TRANSPORT_BACKEND.lock().unwrap() = None;
    TRANSPORT_KILLED.store(false, std::sync::atomic::Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    lazy_static! {
        static ref TEST_ENV_LOCK: Mutex<()> = Mutex::new(());
    }

    fn clear_stream_env() {
        for key in [
            "VT_FERRY_TRANSPORT",
            "VT_FERRY_TCP_HOST",
            "VT_FERRY_TCP_PORT",
            "VT_FERRY_VSOCK_PORT",
            "VT_FERRY_TRANSPORT_TIMEOUT_MS",
            "VT_FERRY_HEARTBEAT_INTERVAL_MS",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn selects_tcp_transport_from_explicit_env() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        std::env::set_var("VT_FERRY_TRANSPORT", "tcp");
        std::env::set_var("VT_FERRY_TCP_HOST", "host.docker.internal");
        std::env::set_var("VT_FERRY_TCP_PORT", "7777");

        match stream_selection_from_env().expect("tcp selection") {
            StreamSelection::Tcp { host, port } => {
                assert_eq!(host, "host.docker.internal");
                assert_eq!(port, 7777);
            }
            StreamSelection::Vsock { .. } => panic!("expected tcp selection"),
        }

        clear_stream_env();
    }

    #[test]
    fn selects_vsock_transport_from_explicit_env() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        std::env::set_var("VT_FERRY_TRANSPORT", "vsock");
        std::env::set_var("VT_FERRY_VSOCK_PORT", "6601");

        match stream_selection_from_env().expect("vsock selection") {
            StreamSelection::Vsock { port } => assert_eq!(port, 6601),
            StreamSelection::Tcp { .. } => panic!("expected vsock selection"),
        }

        clear_stream_env();
    }

    #[test]
    fn selects_tcp_via_implicit_port_env() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        std::env::set_var("VT_FERRY_TCP_PORT", "8888");

        match stream_selection_from_env().expect("implicit tcp") {
            StreamSelection::Tcp { host, port } => {
                assert_eq!(host, TCP_HOST_DEFAULT);
                assert_eq!(port, 8888);
            }
            StreamSelection::Vsock { .. } => panic!("expected tcp"),
        }

        clear_stream_env();
    }

    #[test]
    fn defaults_to_no_selection_without_env() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        assert!(stream_selection_from_env().is_none());
    }

    #[test]
    fn transport_timeout_defaults_when_unset() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        assert_eq!(transport_timeout_ms(), TRANSPORT_TIMEOUT_MS_DEFAULT);
    }

    #[test]
    fn transport_timeout_honors_env_override() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        std::env::set_var("VT_FERRY_TRANSPORT_TIMEOUT_MS", "5000");
        assert_eq!(transport_timeout_ms(), 5000);
        std::env::set_var("VT_FERRY_TRANSPORT_TIMEOUT_MS", "0");
        assert_eq!(transport_timeout_ms(), 0);
        clear_stream_env();
    }

    /// Create an AF_UNIX SOCK_STREAM socket pair. Tests use this to simulate
    /// "guest" and "worker" ends of the transport without going through the
    /// real connect path.
    fn unix_socketpair() -> (File, File) {
        use std::os::fd::{FromRawFd, OwnedFd};
        let mut fds = [0i32; 2];
        let rc = unsafe {
            libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr())
        };
        assert_eq!(rc, 0, "socketpair failed: {}", std::io::Error::last_os_error());
        let a = unsafe { File::from(OwnedFd::from_raw_fd(fds[0])) };
        let b = unsafe { File::from(OwnedFd::from_raw_fd(fds[1])) };
        (a, b)
    }

    #[test]
    fn poisons_when_peer_closes_before_reply() {
        let (guest, worker) = unix_socketpair();
        drop(worker); // simulate worker crash before the request is served
        let mut backend = StreamBackend::from_test_file(guest);
        let result = backend.send_request_dynamic(0xFFFE, b"");
        assert!(result.is_err(), "expected error after peer close");
        assert!(backend.poisoned, "backend should be poisoned after I/O error");
    }

    #[test]
    fn poisons_quickly_when_worker_hangs() {
        use std::time::{Duration, Instant};
        let (guest, worker) = unix_socketpair();
        // Apply a 100ms timeout to the guest end; the worker end stays open
        // but never reads or writes. send_request_dynamic should time out
        // on either the write or the read within ~100ms, set poisoned, and
        // return Err.
        apply_socket_timeouts(guest.as_raw_fd(), 100).expect("apply_socket_timeouts");
        let mut backend = StreamBackend::from_test_file(guest);
        let start = Instant::now();
        let result = backend.send_request_dynamic(0xFFFD, b"");
        let elapsed = start.elapsed();
        assert!(result.is_err(), "expected timeout error from hung worker");
        assert!(backend.poisoned, "backend should be poisoned after timeout");
        assert!(
            elapsed < Duration::from_secs(2),
            "expected fast bail (~100ms), got {elapsed:?}"
        );
        drop(worker);
    }

    #[test]
    fn apply_socket_timeouts_sets_so_rcvtimeo_and_so_sndtimeo() {
        // Use a Unix domain socket pair so the test exercises the real
        // setsockopt path on every platform.
        unsafe {
            let mut fds = [0i32; 2];
            let rc = libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr());
            assert_eq!(rc, 0, "socketpair failed: {}", std::io::Error::last_os_error());

            apply_socket_timeouts(fds[0], 1500).expect("apply_socket_timeouts");

            for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
                let mut tv = libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                };
                let mut len = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
                let rc = libc::getsockopt(
                    fds[0],
                    libc::SOL_SOCKET,
                    option,
                    &mut tv as *mut libc::timeval as *mut libc::c_void,
                    &mut len,
                );
                assert_eq!(rc, 0, "getsockopt failed");
                assert_eq!(tv.tv_sec, 1, "expected 1s, got {}s", tv.tv_sec);
                assert_eq!(tv.tv_usec, 500_000, "expected 500ms, got {}us", tv.tv_usec);
            }

            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }

    #[test]
    fn heartbeat_interval_defaults_when_unset() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        assert_eq!(heartbeat_interval_ms(), HEARTBEAT_INTERVAL_MS_DEFAULT);
    }

    #[test]
    fn heartbeat_interval_honors_env_override() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        std::env::set_var("VT_FERRY_HEARTBEAT_INTERVAL_MS", "2500");
        assert_eq!(heartbeat_interval_ms(), 2500);
        std::env::set_var("VT_FERRY_HEARTBEAT_INTERVAL_MS", "0");
        assert_eq!(heartbeat_interval_ms(), 0);
        clear_stream_env();
    }

    #[test]
    fn last_activity_advances_on_successful_round_trip() {
        // Spawn a tiny mock worker thread that echoes back a status_ok
        // header for each incoming request, then drives one round trip
        // and checks that last_activity moved forward.
        let (guest, worker) = unix_socketpair();
        let worker_thread = std::thread::spawn(move || {
            let mut worker = worker;
            let mut header_buf = [0u8; std::mem::size_of::<MessageHeader>()];
            if worker.read_exact(&mut header_buf).is_err() {
                return;
            }
            let req: MessageHeader = bytemuck::pod_read_unaligned(&header_buf);
            let mut payload = vec![0u8; req.payload_len as usize];
            if req.payload_len != 0 && worker.read_exact(&mut payload).is_err() {
                return;
            }
            let mut reply = MessageHeader::zeroed();
            reply.version = VTF_TRANSPORT_VERSION;
            reply.opcode = req.opcode;
            reply.request_id = req.request_id;
            reply.status = STATUS_OK;
            reply.payload_len = 0;
            let _ = worker.write_all(bytemuck::bytes_of(&reply));
        });

        let mut backend = StreamBackend::from_test_file(guest);
        let before = backend.last_activity;
        // Sleep a hair so the post-call timestamp is provably later even on
        // platforms with a coarse Instant resolution.
        std::thread::sleep(Duration::from_millis(2));
        let result = backend.send_request_dynamic(OP_PING, &[]);
        assert!(result.is_ok(), "OP_PING should succeed against echo worker");
        assert!(
            backend.last_activity > before,
            "last_activity should advance after a successful round trip"
        );
        worker_thread.join().expect("worker thread");
    }

    #[test]
    fn poison_makes_transport_terminally_killed() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        clear_stream_env();
        // Aim the transport at a deliberately wrong vsock port so any
        // post-kill reconnect attempt would actually try (and fail) to
        // open a fresh socket — exercising the gate, not just relying on
        // a None backend.
        std::env::set_var("VT_FERRY_TRANSPORT", "tcp");
        std::env::set_var("VT_FERRY_TCP_HOST", "127.0.0.1");
        std::env::set_var("VT_FERRY_TCP_PORT", "1");
        reset_backend_for_tests();

        // Seed the backend with a half-open Unix socketpair so the first
        // send_request_dynamic call sees ECONNRESET / EPIPE and poisons
        // it the same way a real worker crash would. We bypass
        // ensure_backend since we want a controlled poisoning event.
        let (guest, worker) = unix_socketpair();
        drop(worker);
        {
            let mut guard = TRANSPORT_BACKEND.lock().unwrap();
            *guard = Some(StreamBackend::from_test_file(guest));
        }

        let first = send_request_bytes(OP_PING, &[]);
        assert!(first.is_err(), "first call should fail and poison the backend");
        assert!(
            TRANSPORT_KILLED.load(std::sync::atomic::Ordering::Acquire),
            "TRANSPORT_KILLED should be set after the poison"
        );
        assert!(
            TRANSPORT_BACKEND.lock().unwrap().is_none(),
            "backend should be dropped after the poison"
        );

        // Second call must short-circuit without redialing.
        let second = send_request_bytes(OP_PING, &[]);
        assert_eq!(second, Err(-12902), "subsequent calls must terminally fail");
        assert!(
            TRANSPORT_BACKEND.lock().unwrap().is_none(),
            "ensure_backend must refuse to redial after kill"
        );

        clear_stream_env();
        reset_backend_for_tests();
    }

    #[test]
    fn last_activity_advances_even_on_non_ok_status() {
        // A worker that's alive but returns an application-level error
        // is still alive — heartbeat should treat the round trip as
        // evidence and not probe again.
        let (guest, worker) = unix_socketpair();
        let worker_thread = std::thread::spawn(move || {
            let mut worker = worker;
            let mut header_buf = [0u8; std::mem::size_of::<MessageHeader>()];
            if worker.read_exact(&mut header_buf).is_err() {
                return;
            }
            let req: MessageHeader = bytemuck::pod_read_unaligned(&header_buf);
            let mut reply = MessageHeader::zeroed();
            reply.version = VTF_TRANSPORT_VERSION;
            reply.opcode = req.opcode;
            reply.request_id = req.request_id;
            reply.status = STATUS_INVALID_HANDLE;
            reply.payload_len = 0;
            let _ = worker.write_all(bytemuck::bytes_of(&reply));
        });

        let mut backend = StreamBackend::from_test_file(guest);
        let before = backend.last_activity;
        std::thread::sleep(Duration::from_millis(2));
        let result = backend.send_request_dynamic(OP_PING, &[]);
        assert!(result.is_err(), "should propagate non-OK status");
        assert!(
            backend.last_activity > before,
            "last_activity should advance even when status != STATUS_OK"
        );
        assert!(
            !backend.poisoned,
            "non-OK status is not a transport-level failure"
        );
        worker_thread.join().expect("worker thread");
    }
}
