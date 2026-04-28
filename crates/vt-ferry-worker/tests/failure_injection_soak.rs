//! Phase 9 exit criterion: the system survives repeated failure
//! injection without leaking or wedging.
//!
//! Drives the real `vt-ferry-worker` binary (mock backend) through
//! `N` connection cycles, each one ending in a different abnormal
//! teardown — clean drop mid-stream, oversized payload, partial
//! header, no payload after announcing one — and asserts that:
//!
//!  * The worker process stays alive across the whole soak
//!    (`try_wait()` never reports an exit).
//!  * After each abnormal cycle, a fresh connection still
//!    completes a HELLO exchange — i.e. the per-connection
//!    `Backend::reset_from_env` ran and the accept loop is healthy.
//!  * After the soak, `CREATE_SESSION` returns the *same* base
//!    session_id namespace as before the soak (mock starts at
//!    1000 each time it's reset). Proves the worker hasn't
//!    accumulated state across the failure events.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bytemuck::Zeroable;
use vt_ferry_protocol::{
    CreateSessionPayload, CreateSessionReply, HelloPayload, HelloReply, MessageHeader, OP_CREATE_SESSION,
    OP_HELLO, VTF_TRANSPORT_VERSION, STATUS_OK,
};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is .../<workspace>/crates/vt-ferry-worker; the
    // workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn build_debug_worker() -> PathBuf {
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

struct WorkerGuard {
    child: Child,
    socket_path: PathBuf,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn spawn_worker(worker: &PathBuf, socket_path: &PathBuf) -> WorkerGuard {
    let _ = std::fs::remove_file(socket_path);
    let child = Command::new(worker)
        .arg(socket_path)
        .env("VT_FERRY_HOST_WORKER_BACKEND", "mock")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn host-worker");
    WorkerGuard {
        child,
        socket_path: socket_path.clone(),
    }
}

fn connect(socket_path: &PathBuf) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_err: Option<std::io::Error> = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket_path) {
            Ok(s) => return s,
            Err(e) => last_err = Some(e),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "worker socket {} never became connectable: {:?}",
        socket_path.display(),
        last_err
    );
}

fn assert_worker_running(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(status)) => panic!("worker exited mid-soak: {:?}", status),
        Ok(None) => {}
        Err(e) => panic!("try_wait on worker failed: {e}"),
    }
}

/// Send a normal HELLO and verify it lands. Failure means the worker
/// has stopped accepting requests cleanly.
fn round_trip_hello(stream: &mut UnixStream, request_id: u64) {
    let payload = HelloPayload {
        client_abi_version: VTF_TRANSPORT_VERSION as u32,
        reserved: 0,
        requested_features: 0,
    };
    let mut header = MessageHeader::zeroed();
    header.version = VTF_TRANSPORT_VERSION;
    header.opcode = OP_HELLO;
    header.request_id = request_id;
    header.payload_len = std::mem::size_of::<HelloPayload>() as u32;
    stream
        .write_all(bytemuck::bytes_of(&header))
        .expect("write hello header");
    stream
        .write_all(bytemuck::bytes_of(&payload))
        .expect("write hello payload");
    let mut response_header = [0u8; std::mem::size_of::<MessageHeader>()];
    stream
        .read_exact(&mut response_header)
        .expect("read hello response header");
    let response: MessageHeader = bytemuck::pod_read_unaligned(&response_header);
    assert_eq!(response.status, STATUS_OK, "post-cycle HELLO must succeed");
    assert!(
        response.payload_len as usize >= std::mem::size_of::<HelloReply>(),
        "post-cycle HELLO reply truncated"
    );
    let mut reply_buf = vec![0u8; response.payload_len as usize];
    stream
        .read_exact(&mut reply_buf)
        .expect("read hello reply body");
}

/// Inject a clean-drop failure: write a valid header, drop the stream
/// before the matching payload arrives. Worker must close its inner
/// loop on the next read attempt and circle back to accept.
fn inject_drop_after_header(socket_path: &PathBuf) {
    let mut stream = connect(socket_path);
    let mut header = MessageHeader::zeroed();
    header.version = VTF_TRANSPORT_VERSION;
    header.opcode = OP_HELLO;
    header.request_id = 1;
    header.payload_len = std::mem::size_of::<HelloPayload>() as u32;
    let _ = stream.write_all(bytemuck::bytes_of(&header));
    // drop without writing the payload
}

/// Inject a "partial header" failure: write 4 bytes of a header, drop.
fn inject_partial_header(socket_path: &PathBuf) {
    let mut stream = connect(socket_path);
    let _ = stream.write_all(&[0u8; 4]);
}

/// Inject an oversized payload. Worker treats it as a transport-level
/// error and breaks the inner loop without serving the request.
fn inject_oversized_payload(socket_path: &PathBuf) {
    let mut stream = connect(socket_path);
    let mut header = MessageHeader::zeroed();
    header.version = VTF_TRANSPORT_VERSION;
    header.opcode = OP_HELLO;
    header.request_id = 1;
    // VTF_TRANSPORT_STORAGE_CAPACITY in the worker is 2 MiB; advertise
    // 16 MiB so the worker bails before reading any payload.
    header.payload_len = 16 * 1024 * 1024;
    let _ = stream.write_all(bytemuck::bytes_of(&header));
}

/// Inject a clean disconnect right after a successful HELLO. Like
/// connection_isolation but as one of many cycles.
fn inject_clean_disconnect(socket_path: &PathBuf) {
    let mut stream = connect(socket_path);
    round_trip_hello(&mut stream, 1);
    // drop
}

#[test]
fn worker_survives_repeated_failure_injection() {
    const CYCLES_PER_MODE: usize = 5;
    let modes: [fn(&PathBuf); 4] = [
        inject_drop_after_header,
        inject_partial_header,
        inject_oversized_payload,
        inject_clean_disconnect,
    ];

    let worker = build_debug_worker();
    let socket_path = std::env::temp_dir().join(format!(
        "vt-ferry-soak-{}.sock",
        std::process::id()
    ));
    let mut guard = spawn_worker(&worker, &socket_path);

    // Warm up the listener — connect once cleanly so we know the
    // worker has bound the socket and is ready to accept.
    {
        let mut stream = connect(&socket_path);
        round_trip_hello(&mut stream, 1);
    }
    let pre_session_id = create_session_id(&socket_path);

    for cycle in 0..CYCLES_PER_MODE {
        for (mode_idx, mode) in modes.iter().enumerate() {
            mode(&socket_path);
            // Brief breath so the worker can finish unwinding the
            // failed inner loop and recycle through accept.
            std::thread::sleep(Duration::from_millis(20));
            assert_worker_running(&mut guard.child);

            // Health probe: a clean HELLO must still land. If this
            // hangs, something wedged.
            let mut stream = connect(&socket_path);
            round_trip_hello(&mut stream, ((cycle * 100 + mode_idx) + 2) as u64);
        }
    }

    // Final integrity check: the session-id namespace must still be
    // pristine after all this churn — proving every accept ran
    // reset_from_env cleanly.
    let post_session_id = create_session_id(&socket_path);
    assert_eq!(
        pre_session_id, post_session_id,
        "session-id namespace drifted after {} cycles of failure injection ({pre_session_id} → {post_session_id}); \
         that means at least one accept didn't reset the backend",
        CYCLES_PER_MODE * modes.len()
    );

    assert_worker_running(&mut guard.child);
}

fn create_session_id(socket_path: &PathBuf) -> u64 {
    let mut stream = connect(socket_path);
    round_trip_hello(&mut stream, 1);
    let payload = CreateSessionPayload {
        kind: 1,
        codec: 1,
        width: 64,
        height: 36,
        pixel_format: 0x34323076,
        fps_num: 30,
        fps_den: 1,
        bitrate: 250_000,
        gop_size: 30,
    };
    let mut header = MessageHeader::zeroed();
    header.version = VTF_TRANSPORT_VERSION;
    header.opcode = OP_CREATE_SESSION;
    header.request_id = 2;
    header.payload_len = std::mem::size_of::<CreateSessionPayload>() as u32;
    stream
        .write_all(bytemuck::bytes_of(&header))
        .expect("write create-session header");
    stream
        .write_all(bytemuck::bytes_of(&payload))
        .expect("write create-session payload");
    let mut response_header = [0u8; std::mem::size_of::<MessageHeader>()];
    stream
        .read_exact(&mut response_header)
        .expect("read create-session response header");
    let response: MessageHeader = bytemuck::pod_read_unaligned(&response_header);
    assert_eq!(response.status, STATUS_OK);
    let mut body = vec![0u8; response.payload_len as usize];
    stream.read_exact(&mut body).expect("read create-session body");
    let reply: CreateSessionReply =
        bytemuck::pod_read_unaligned(&body[..std::mem::size_of::<CreateSessionReply>()]);
    reply.session_id
}
