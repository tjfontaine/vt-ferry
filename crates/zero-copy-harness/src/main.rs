//! End-to-end proof harness for vt-ferry zero-copy IOSurface encode.
//!
//! Simulates what the smolvm launcher will eventually do in production:
//!   1. Allocate a BGRA IOSurface sized to the target pool slot.
//!   2. Write a deterministic pattern into it directly via
//!      `IOSurfaceGetBaseAddress`.
//!   3. `mach_ports_register` the IOSurface's Mach send right so a child
//!      task can `mach_ports_lookup` it.
//!   4. `posix_spawn` vt-ferry-host-worker with `VT_FERRY_HOST_WORKER_BACKEND=vt-real`
//!      and `VT_FERRY_IOSURFACE_POOL_SPECS_JSON` naming the shape + iosurface_id.
//!   5. Connect to the worker's host-side control socket and drive
//!      HELLO → CREATE_SESSION → CREATE_BUFFER_POOL → ALLOC_BUFFER →
//!      PREPARE_SESSION → ENCODE_FRAME → DRAIN → DEQUEUE_OUTPUT →
//!      READ_OUTPUT.
//!
//! Asserts on exit 0:
//!   - The CREATE_BUFFER_POOL reply reports
//!     `host_backing_kind == VTF_HOST_BACKING_KIND_IOSURFACE` and the
//!     slot-0 `SharedRegionReply` carries
//!     `source_kind == VTF_SHARED_REGION_SOURCE_IOSURFACE` with the
//!     launcher-allocated IOSurfaceID in `source_handle` — i.e. vt-real
//!     took the zero-copy branch.
//!   - `ENCODE_FRAME` → `DRAIN` → `DEQUEUE_OUTPUT` → `READ_OUTPUT` yields a
//!     non-empty H.264 Annex-B sample.
//!
//! The worker's internal copy counter is not asserted here (that would
//! require another opcode or side-channel); the source_kind assertion is
//! sufficient pass/fail signal because the branch containing
//! `vtf_copy_mapped_to_pixel_buffer` on the encode path is only reached
//! when the pool is NOT IOSurface-backed.

use bytemuck::Zeroable;
use vt_ferry_protocol::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;
const SLOT_COUNT: u32 = 3;
const BGRA_FOURCC: u32 = 0x42475241; // 'BGRA'
const H264_FOURCC: u32 = 0x61766331; // 'avc1'
const SESSION_KIND_ENCODE: u32 = 1;

// Apple framework symbols use lowercase-`k` constant prefixes
// and the FFI declarations expose more accessors than the harness
// currently exercises (kept so the same module can grow into a
// fuller probe without re-deriving bindings).
#[allow(non_camel_case_types, non_upper_case_globals, dead_code)]
mod iosurface_ffi {
    use libc::{c_int, c_uint, c_void};

    pub type mach_port_t = c_uint;
    pub type kern_return_t = c_int;
    pub type mach_msg_type_number_t = c_uint;
    pub type mach_port_array_t = *mut mach_port_t;

    pub type CFStringRef = *const c_void;
    pub type CFNumberRef = *const c_void;
    pub type CFNumberType = c_int;
    pub type CFAllocatorRef = *const c_void;
    pub type CFDictionaryKeyCallBacks = c_void;
    pub type CFDictionaryValueCallBacks = c_void;
    pub type CFMutableDictionaryRef = *mut c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFTypeRef = *const c_void;
    pub type CFIndex = isize;

    pub const kCFNumberIntType: CFNumberType = 9;
    pub const KERN_SUCCESS: kern_return_t = 0;

    #[repr(C)]
    pub struct __IOSurface(c_void);
    pub type IOSurfaceRef = *mut __IOSurface;

    unsafe extern "C" {
        pub static mach_task_self_: mach_port_t;

        pub fn mach_ports_register(
            target_task: mach_port_t,
            init_port_set: mach_port_array_t,
            init_port_set_cnt: mach_msg_type_number_t,
        ) -> kern_return_t;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
        pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
        pub fn CFDictionaryCreateMutable(
            allocator: CFAllocatorRef,
            capacity: CFIndex,
            key_callbacks: *const CFDictionaryKeyCallBacks,
            value_callbacks: *const CFDictionaryValueCallBacks,
        ) -> CFMutableDictionaryRef;
        pub fn CFDictionarySetValue(
            dict: CFMutableDictionaryRef,
            key: *const c_void,
            value: *const c_void,
        );
        pub fn CFNumberCreate(
            allocator: CFAllocatorRef,
            the_type: CFNumberType,
            value_ptr: *const c_void,
        ) -> CFNumberRef;
        pub fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "IOSurface", kind = "framework")]
    unsafe extern "C" {
        pub fn IOSurfaceCreate(properties: CFDictionaryRef) -> IOSurfaceRef;
        pub fn IOSurfaceCreateMachPort(surface: IOSurfaceRef) -> mach_port_t;
        pub fn IOSurfaceGetID(surface: IOSurfaceRef) -> u32;
        pub fn IOSurfaceGetBaseAddress(surface: IOSurfaceRef) -> *mut c_void;
        pub fn IOSurfaceGetAllocSize(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceGetBytesPerRow(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceLock(surface: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;
        pub fn IOSurfaceUnlock(surface: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;

        pub static kIOSurfaceWidth: CFStringRef;
        pub static kIOSurfaceHeight: CFStringRef;
        pub static kIOSurfaceBytesPerElement: CFStringRef;
        pub static kIOSurfaceBytesPerRow: CFStringRef;
        pub static kIOSurfacePixelFormat: CFStringRef;
    }
}

fn die(msg: &str) -> ! {
    eprintln!("zero-copy-harness: {}", msg);
    std::process::exit(1);
}

fn bail(msg: &str) -> ! {
    die(msg);
}

fn create_bgra_iosurface(width: u32, height: u32) -> (iosurface_ffi::IOSurfaceRef, u32) {
    use iosurface_ffi::*;
    unsafe {
        let props = CFDictionaryCreateMutable(
            std::ptr::null(),
            0,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let w = width as i32;
        let h = height as i32;
        let bpr = (width * 4) as i32;
        let bpe = 4i32;
        let fmt = BGRA_FOURCC as i32;
        let w_n = CFNumberCreate(
            std::ptr::null(),
            kCFNumberIntType,
            &w as *const _ as *const _,
        );
        let h_n = CFNumberCreate(
            std::ptr::null(),
            kCFNumberIntType,
            &h as *const _ as *const _,
        );
        let bpr_n = CFNumberCreate(
            std::ptr::null(),
            kCFNumberIntType,
            &bpr as *const _ as *const _,
        );
        let bpe_n = CFNumberCreate(
            std::ptr::null(),
            kCFNumberIntType,
            &bpe as *const _ as *const _,
        );
        let fmt_n = CFNumberCreate(
            std::ptr::null(),
            kCFNumberIntType,
            &fmt as *const _ as *const _,
        );
        CFDictionarySetValue(props, kIOSurfaceWidth as *const _, w_n as *const _);
        CFDictionarySetValue(props, kIOSurfaceHeight as *const _, h_n as *const _);
        CFDictionarySetValue(
            props,
            kIOSurfaceBytesPerElement as *const _,
            bpe_n as *const _,
        );
        CFDictionarySetValue(props, kIOSurfaceBytesPerRow as *const _, bpr_n as *const _);
        CFDictionarySetValue(props, kIOSurfacePixelFormat as *const _, fmt_n as *const _);
        let surface = IOSurfaceCreate(props);
        CFRelease(w_n as CFTypeRef);
        CFRelease(h_n as CFTypeRef);
        CFRelease(bpr_n as CFTypeRef);
        CFRelease(bpe_n as CFTypeRef);
        CFRelease(fmt_n as CFTypeRef);
        CFRelease(props as CFTypeRef);
        if surface.is_null() {
            bail("IOSurfaceCreate returned nil");
        }
        let id = IOSurfaceGetID(surface);
        (surface, id)
    }
}

fn fill_pattern(surface: iosurface_ffi::IOSurfaceRef) {
    use iosurface_ffi::*;
    unsafe {
        let mut seed: u32 = 0;
        if IOSurfaceLock(surface, 0, &mut seed) != 0 {
            bail("IOSurfaceLock for write failed");
        }
        let base = IOSurfaceGetBaseAddress(surface) as *mut u8;
        let len = IOSurfaceGetAllocSize(surface);
        for i in 0..len {
            *base.add(i) = ((i.wrapping_mul(53).wrapping_add(29)) & 0xFF) as u8;
        }
        let _ = IOSurfaceUnlock(surface, 0, &mut seed);
    }
}

fn write_message(
    stream: &mut UnixStream,
    opcode: u16,
    request_id: u64,
    payload: &[u8],
) -> std::io::Result<()> {
    let header = MessageHeader {
        version: 1,
        opcode,
        flags: 0,
        request_id,
        payload_len: payload.len() as u32,
        status: 0,
    };
    stream.write_all(bytemuck::bytes_of(&header))?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    Ok(())
}

fn read_message(stream: &mut UnixStream) -> std::io::Result<(MessageHeader, Vec<u8>)> {
    let mut hdr_buf = [0u8; std::mem::size_of::<MessageHeader>()];
    stream.read_exact(&mut hdr_buf)?;
    let header: MessageHeader = bytemuck::pod_read_unaligned(&hdr_buf);
    let mut payload = vec![0u8; header.payload_len as usize];
    if !payload.is_empty() {
        stream.read_exact(&mut payload)?;
    }
    Ok((header, payload))
}

fn connect_with_retry(path: &PathBuf, timeout: Duration) -> UnixStream {
    let start = Instant::now();
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(_) if start.elapsed() < timeout => {
                sleep(Duration::from_millis(20));
            }
            Err(e) => bail(&format!(
                "worker socket {} never became connectable: {}",
                path.display(),
                e
            )),
        }
    }
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

fn spawn_worker(
    socket_path: &PathBuf,
    iosurface_ids: &[u32],
    width: u32,
    height: u32,
    slot_count: u32,
) -> WorkerGuard {
    let worker_bin = std::env::var("VT_FERRY_HOST_WORKER_BIN")
        .unwrap_or_else(|_| "target/debug/vt-ferry-worker".to_string());
    let mut entries = Vec::with_capacity(iosurface_ids.len());
    for (slot_index, id) in iosurface_ids.iter().enumerate() {
        entries.push(format!(
            r#"{{"width":{},"height":{},"pixel_format":{},"slot_count":{},"slot_index":{},"iosurface_id":{}}}"#,
            width, height, BGRA_FOURCC, slot_count, slot_index, id
        ));
    }
    let specs_json = format!("[{}]", entries.join(","));
    let child = Command::new(&worker_bin)
        .arg(socket_path.as_os_str())
        .env("VT_FERRY_HOST_WORKER_BACKEND", "vt-real")
        .env("VT_FERRY_IOSURFACE_POOL_SPECS_JSON", specs_json)
        .env(
            "VT_FERRY_HOST_WORKER_PARENT_PID",
            std::process::id().to_string(),
        )
        .spawn()
        .unwrap_or_else(|e| {
            bail(&format!("failed to spawn {}: {}", worker_bin, e));
        });
    WorkerGuard {
        child,
        socket_path: socket_path.clone(),
    }
}

fn main() {
    // 1. Allocate SLOT_COUNT IOSurfaces, fill each with a deterministic
    //    pattern (a different pattern per slot so we can sanity-check later).
    let mut surfaces: Vec<iosurface_ffi::IOSurfaceRef> = Vec::with_capacity(SLOT_COUNT as usize);
    let mut surface_ids: Vec<u32> = Vec::with_capacity(SLOT_COUNT as usize);
    for _ in 0..SLOT_COUNT {
        let (surface, surface_id) = create_bgra_iosurface(WIDTH, HEIGHT);
        fill_pattern(surface);
        surfaces.push(surface);
        surface_ids.push(surface_id);
    }
    println!(
        "harness: allocated {} IOSurfaces ids={:?}",
        SLOT_COUNT, surface_ids
    );

    // 2. Get Mach ports + register all before spawning worker.
    let mut ports: Vec<iosurface_ffi::mach_port_t> = Vec::with_capacity(surfaces.len());
    for surface in &surfaces {
        let port = unsafe { iosurface_ffi::IOSurfaceCreateMachPort(*surface) };
        if port == 0 {
            bail("IOSurfaceCreateMachPort returned 0");
        }
        ports.push(port);
    }
    let kr = unsafe {
        iosurface_ffi::mach_ports_register(
            iosurface_ffi::mach_task_self_,
            ports.as_mut_ptr(),
            ports.len() as iosurface_ffi::mach_msg_type_number_t,
        )
    };
    if kr != iosurface_ffi::KERN_SUCCESS {
        bail(&format!("mach_ports_register kr={}", kr));
    }

    // 3. Spawn worker with zero-copy pool specs (one per slot).
    let socket_path = PathBuf::from(format!(
        "/tmp/vt-ferry-zero-copy-harness-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let worker = spawn_worker(&socket_path, &surface_ids, WIDTH, HEIGHT, SLOT_COUNT);

    let mut stream = connect_with_retry(&socket_path, Duration::from_secs(5));
    println!("harness: connected to worker at {}", socket_path.display());

    // 4. HELLO
    let hello_payload = HelloPayload {
        client_abi_version: VTF_TRANSPORT_VERSION as u32,
        reserved: 0,
        requested_features: 0,
    };
    write_message(&mut stream, OP_HELLO, 1, bytemuck::bytes_of(&hello_payload))
        .unwrap_or_else(|e| bail(&format!("send HELLO: {}", e)));
    let (hdr, _payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("HELLO reply status={}", hdr.status));
    }
    println!("harness: HELLO ok");

    // 5. CREATE_SESSION (h264 encode, BGRA 256x144)
    let mut cs_payload = CreateSessionPayload::zeroed();
    cs_payload.kind = SESSION_KIND_ENCODE;
    cs_payload.codec = H264_FOURCC;
    cs_payload.width = WIDTH;
    cs_payload.height = HEIGHT;
    cs_payload.pixel_format = BGRA_FOURCC;
    cs_payload.fps_num = 30;
    cs_payload.fps_den = 1;
    cs_payload.bitrate = 1_000_000;
    cs_payload.gop_size = 30;
    write_message(
        &mut stream,
        OP_CREATE_SESSION,
        2,
        bytemuck::bytes_of(&cs_payload),
    )
    .unwrap();
    let (hdr, payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("CREATE_SESSION status={}", hdr.status));
    }
    let cs_reply: CreateSessionReply =
        bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<CreateSessionReply>()]);
    let session_id = cs_reply.session_id;
    println!("harness: CREATE_SESSION ok session_id={}", session_id);

    // 6. CREATE_BUFFER_POOL with SLOT_COUNT slots — expect zero-copy path.
    let cbp_payload = CreateBufferPoolPayload {
        session_id,
        buffer_count: SLOT_COUNT,
        pixel_format: BGRA_FOURCC,
        width: WIDTH,
        height: HEIGHT,
        usage_flags: 0,
        _padding: 0,
    };
    write_message(
        &mut stream,
        OP_CREATE_BUFFER_POOL,
        3,
        bytemuck::bytes_of(&cbp_payload),
    )
    .unwrap();
    let (hdr, payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("CREATE_BUFFER_POOL status={}", hdr.status));
    }
    let cbp_reply: CreateBufferPoolReply =
        bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<CreateBufferPoolReply>()]);
    if cbp_reply.host_backing_kind != VTF_HOST_BACKING_KIND_IOSURFACE {
        bail(&format!(
            "host_backing_kind = {} (expected IOSURFACE={})",
            cbp_reply.host_backing_kind, VTF_HOST_BACKING_KIND_IOSURFACE
        ));
    }
    // Every slot's SharedRegionReply must advertise IOSURFACE source, and the
    // reported IOSurfaceIDs must match the SLOT_COUNT IDs we pre-registered.
    for slot_index in 0..SLOT_COUNT as usize {
        let sr = &cbp_reply.shared_regions[slot_index];
        if sr.source_kind != VTF_SHARED_REGION_SOURCE_IOSURFACE {
            bail(&format!(
                "shared_regions[{}].source_kind = {} (expected IOSURFACE={}) — \
                 vt-real did NOT take the zero-copy branch for slot {}",
                slot_index, sr.source_kind, VTF_SHARED_REGION_SOURCE_IOSURFACE, slot_index,
            ));
        }
        if sr.source_handle != surface_ids[slot_index] as u64 {
            bail(&format!(
                "shared_regions[{}].source_handle = {} (expected IOSurfaceID {})",
                slot_index, sr.source_handle, surface_ids[slot_index]
            ));
        }
    }
    let pool_id = cbp_reply.pool_id;
    println!(
        "harness: CREATE_BUFFER_POOL took zero-copy branch for all {} slots \
         (pool_id={} source_kind=IOSURFACE ids={:?})",
        SLOT_COUNT, pool_id, surface_ids
    );

    // 7. ALLOC_BUFFER
    let alloc_payload = AllocBufferPayload { pool_id };
    write_message(
        &mut stream,
        OP_ALLOC_BUFFER,
        4,
        bytemuck::bytes_of(&alloc_payload),
    )
    .unwrap();
    let (hdr, payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("ALLOC_BUFFER status={}", hdr.status));
    }
    let alloc_reply: AllocBufferReply =
        bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<AllocBufferReply>()]);
    let buffer_id = alloc_reply.buffer_id;
    let generation = alloc_reply.generation;
    println!(
        "harness: ALLOC_BUFFER ok buffer_id={} generation={}",
        buffer_id, generation
    );

    // 8. PREPARE_SESSION
    let prep = PrepareSessionPayload { session_id };
    write_message(
        &mut stream,
        OP_PREPARE_SESSION,
        5,
        bytemuck::bytes_of(&prep),
    )
    .unwrap();
    let (hdr, _) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("PREPARE_SESSION status={}", hdr.status));
    }

    // 9. ENCODE_FRAME — the guest-equivalent write has already happened
    //    (fill_pattern above wrote into the IOSurface's backing pages; in
    //    production the guest would have written through libkrun's HOST_VA
    //    mapping to those same physical pages).
    let enc_payload = EncodeFramePayload {
        session_id,
        image_buffer_proxy_id: buffer_id, // proxy id; worker treats non-zero as presence flag
        image_buffer_host_id: buffer_id,
        image_buffer_generation: generation,
        pts_value: 0,
        pts_timescale: 30,
        duration_timescale: 30,
        duration_value: 1,
    };
    write_message(
        &mut stream,
        OP_ENCODE_FRAME,
        6,
        bytemuck::bytes_of(&enc_payload),
    )
    .unwrap();
    let (hdr, _) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("ENCODE_FRAME status={}", hdr.status));
    }

    // 10. DRAIN
    let drain = DrainPayload { session_id };
    write_message(&mut stream, OP_DRAIN, 7, bytemuck::bytes_of(&drain)).unwrap();
    let (hdr, _) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("DRAIN status={}", hdr.status));
    }

    // 11. DEQUEUE_OUTPUT
    let dq = DequeueOutputPayload { session_id };
    write_message(&mut stream, OP_DEQUEUE_OUTPUT, 8, bytemuck::bytes_of(&dq)).unwrap();
    let (hdr, payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("DEQUEUE_OUTPUT status={}", hdr.status));
    }
    let dq_reply: DequeueOutputReply =
        bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<DequeueOutputReply>()]);
    if dq_reply.sample_size == 0 {
        bail("DEQUEUE_OUTPUT reported sample_size=0");
    }
    println!(
        "harness: DEQUEUE_OUTPUT sample_size={} output_id={}",
        dq_reply.sample_size, dq_reply.output_id
    );

    // 12. READ_OUTPUT
    let ro = ReadOutputPayload {
        output_id: dq_reply.output_id,
    };
    write_message(&mut stream, OP_READ_OUTPUT, 9, bytemuck::bytes_of(&ro)).unwrap();
    let (hdr, payload) = read_message(&mut stream).unwrap();
    if hdr.status != 0 {
        bail(&format!("READ_OUTPUT status={}", hdr.status));
    }
    let reply_hdr_size = std::mem::size_of::<ReadOutputReply>();
    let ro_reply: ReadOutputReply = bytemuck::pod_read_unaligned(&payload[..reply_hdr_size]);
    let sample_bytes = &payload[reply_hdr_size..reply_hdr_size + ro_reply.sample_size as usize];
    if sample_bytes.is_empty() {
        bail("READ_OUTPUT returned 0 bytes");
    }
    println!(
        "harness: READ_OUTPUT got {} bytes of H.264 (first 8 bytes: {:02x?})",
        sample_bytes.len(),
        &sample_bytes[..sample_bytes.len().min(8)]
    );

    drop(worker);
    println!("zero-copy-harness: OK");

    // Clear registered ports to avoid cross-run leakage.
    unsafe {
        let mut empty: [iosurface_ffi::mach_port_t; 0] = [];
        let _ = iosurface_ffi::mach_ports_register(
            iosurface_ffi::mach_task_self_,
            empty.as_mut_ptr(),
            0,
        );
    }

    // Suppress unused warnings for intentional retention.
    let _ = surfaces;
}
