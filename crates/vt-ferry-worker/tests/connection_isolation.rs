//! Integration test for the worker's per-connection state reset.
//!
//! The worker calls `Backend::reset_from_env` at the start of every
//! accepted connection (server.rs). That guarantees a guest crash mid-
//! encode doesn't leave zombie sessions/pools/buffers for the next
//! client. This test exercises that contract end-to-end:
//!
//!  1. Spawn the real `vt-ferry-worker` binary against a Unix socket
//!     with the mock backend.
//!  2. First client: HELLO + CREATE_SESSION, observe `session_id`.
//!  3. Disconnect.
//!  4. Second client: HELLO + CREATE_SESSION, observe `session_id`.
//!  5. Assert the two session IDs are equal — the mock starts
//!     `next_host_id` at 1000 and resets it to 1000 on `reset_from_env`,
//!     so a non-reset worker would return 1001 the second time.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bytemuck::Zeroable;
use vt_ferry_protocol::{
    CreateSessionPayload, CreateSessionReply, HelloPayload, HelloReply, MessageHeader,
    OP_CREATE_SESSION, OP_HELLO, VTF_TRANSPORT_VERSION, STATUS_OK,
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

fn wait_for_socket(socket_path: &PathBuf) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_err: Option<std::io::Error> = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket_path) {
            Ok(s) => return s,
            Err(e) => last_err = Some(e),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "worker socket {} never became connectable: {:?}",
        socket_path.display(),
        last_err
    );
}

fn send_request<P: bytemuck::Pod>(
    stream: &mut UnixStream,
    opcode: u16,
    request_id: u64,
    payload: &P,
) -> (MessageHeader, Vec<u8>) {
    let payload_bytes = bytemuck::bytes_of(payload);
    let mut header = MessageHeader::zeroed();
    header.version = VTF_TRANSPORT_VERSION;
    header.opcode = opcode;
    header.request_id = request_id;
    header.payload_len = payload_bytes.len() as u32;
    stream
        .write_all(bytemuck::bytes_of(&header))
        .expect("write header");
    if !payload_bytes.is_empty() {
        stream.write_all(payload_bytes).expect("write payload");
    }
    let mut response_header = [0u8; std::mem::size_of::<MessageHeader>()];
    stream
        .read_exact(&mut response_header)
        .expect("read response header");
    let response: MessageHeader = bytemuck::pod_read_unaligned(&response_header);
    let mut response_payload = vec![0u8; response.payload_len as usize];
    if response.payload_len != 0 {
        stream
            .read_exact(&mut response_payload)
            .expect("read response payload");
    }
    (response, response_payload)
}

fn create_session(stream: &mut UnixStream, request_id: u64) -> u64 {
    let hello_payload = HelloPayload {
        client_abi_version: VTF_TRANSPORT_VERSION as u32,
        reserved: 0,
        requested_features: 0,
    };
    let (hello_resp, hello_body) = send_request(stream, OP_HELLO, request_id, &hello_payload);
    assert_eq!(hello_resp.status, STATUS_OK, "HELLO must succeed");
    assert!(
        hello_body.len() >= std::mem::size_of::<HelloReply>(),
        "HELLO reply truncated"
    );

    let create_payload = CreateSessionPayload {
        kind: 1, // VTF_SESSION_KIND_ENCODE
        codec: 1,
        width: 128,
        height: 72,
        pixel_format: 0x34323076, // '420v' / NV12
        fps_num: 30,
        fps_den: 1,
        bitrate: 1_000_000,
        gop_size: 30,
    };
    let (create_resp, create_body) =
        send_request(stream, OP_CREATE_SESSION, request_id + 1, &create_payload);
    assert_eq!(
        create_resp.status, STATUS_OK,
        "CREATE_SESSION must succeed"
    );
    let reply: CreateSessionReply =
        bytemuck::pod_read_unaligned(&create_body[..std::mem::size_of::<CreateSessionReply>()]);
    reply.session_id
}

#[test]
fn worker_resets_session_id_namespace_per_connection() {
    let worker = build_debug_worker();
    let socket_path = std::env::temp_dir().join(format!(
        "vt-ferry-conn-isolation-{}.sock",
        std::process::id()
    ));
    let _guard = spawn_worker(&worker, &socket_path);

    let first_session_id = {
        let mut stream = wait_for_socket(&socket_path);
        create_session(&mut stream, 1)
        // stream drops here, simulating a guest disconnect
    };

    // Brief pause so the worker's accept loop has a chance to break out
    // of its inner request loop and circle back through reset_from_env.
    std::thread::sleep(Duration::from_millis(100));

    let second_session_id = {
        let mut stream = wait_for_socket(&socket_path);
        create_session(&mut stream, 1)
    };

    assert_eq!(
        first_session_id, second_session_id,
        "worker must reset session_id namespace between connections; \
         got {first_session_id} then {second_session_id}, which means \
         next_host_id wasn't reset on accept (the previous connection's \
         state leaked into the next)"
    );
}
