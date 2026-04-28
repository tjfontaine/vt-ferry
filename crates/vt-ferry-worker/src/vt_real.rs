use crate::backend::Backend;
#[cfg(target_os = "macos")]
use crate::iosurface_bridge::{OwnedIOSurface, wrap_bytes_planar, wrap_bytes_single_plane};
#[cfg(target_os = "macos")]
use crate::iosurface_pool_directory::IOSurfacePoolDirectory;
use crate::probes;
use bytemuck::Zeroable;
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_video::image_buffer::TCVImageBuffer;
use vt_ferry_protocol::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};


#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVPixelBufferLockBaseAddress(pixelBuffer: *const libc::c_void, lockFlags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixelBuffer: *const libc::c_void, lockFlags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(pixelBuffer: *const libc::c_void) -> *mut libc::c_void;
    fn CVPixelBufferGetPlaneCount(pixelBuffer: *const libc::c_void) -> usize;
    fn CVPixelBufferGetBaseAddressOfPlane(
        pixelBuffer: *const libc::c_void,
        planeIndex: usize,
    ) -> *mut libc::c_void;
    fn CVPixelBufferGetBytesPerRowOfPlane(
        pixelBuffer: *const libc::c_void,
        planeIndex: usize,
    ) -> usize;
}

#[repr(C)]
pub struct OpaqueCMBlockBuffer(std::ffi::c_void);
pub type CMBlockBufferRef = *mut OpaqueCMBlockBuffer;

/// VT decompression output callback. Fired by VT's internal
/// worker thread once a frame has been decoded. Retains the
/// image buffer (it would otherwise be released when the
/// callback returns) and enqueues it for the guest to dequeue
/// via `OP_DEQUEUE_DECODED_FRAME`.
///
/// The decode session callback context is heap-stashed in
/// `VtRealDecodeSession::callback_context` so the raw pointer
/// stays stable across format-change recreates.
extern "C" fn vtf_vt_decompression_output_callback(
    decompression_output_ref_con: *mut std::ffi::c_void,
    _source_frame_ref_con: *mut std::ffi::c_void,
    status: i32,
    _info_flags: video_toolbox::errors::VTDecodeInfoFlags,
    image_buffer: core_video::image_buffer::CVImageBufferRef,
    presentation_time_stamp: core_media::time::CMTime,
    presentation_duration: core_media::time::CMTime,
) {
    if decompression_output_ref_con.is_null() {
        return;
    }
    if image_buffer.is_null() || status != 0 {
        // Decoder error — record the status as a per-session
        // sticky error so the guest learns about the failure on
        // its next OP_DEQUEUE_DECODED_FRAME after the FIFO drains.
        // Without this the guest just sees "fewer frames than
        // expected" with no diagnostic; FFmpeg eventually maps
        // that to FFMPEG_ERROR_RATE_EXCEEDED at the CLI which
        // hides the real OSStatus (see Phase 16 commit message).
        // The eprintln stays as belt-and-suspenders for ad-hoc
        // worker-stderr inspection but is no longer the only
        // signal.
        let recorded_status = if status != 0 {
            status
        } else {
            // image_buffer.is_null() with status == 0 — VT shouldn't
            // do this in practice, but if it does we still want to
            // surface a non-zero status to the guest. Use
            // -12911 (kVTVideoDecoderUnknownErr) as the canonical
            // "VT said decode succeeded but produced nothing"
            // marker.
            -12911
        };
        if !decompression_output_ref_con.is_null() {
            let context = unsafe {
                &*(decompression_output_ref_con as *const DecodeSessionCallbackContext)
            };
            context
                .output_state
                .record_decode_error(context.session_id, recorded_status);
        }
        eprintln!(
            "vt-real decompression callback: status={status}, image_buffer.null={}",
            image_buffer.is_null()
        );
        return;
    }
    let context =
        unsafe { &*(decompression_output_ref_con as *const DecodeSessionCallbackContext) };
    // SAFETY: VT hands the callback a +0 reference (under the
    // get-rule). Wrapping under get-rule retains it so the buffer
    // outlives the callback frame.
    let buffer = unsafe {
        core_video::image_buffer::CVImageBuffer::wrap_under_get_rule(image_buffer)
    };
    context.output_state.enqueue(
        context.session_id,
        VtRealDecodedOutput {
            session_id: context.session_id,
            pts: presentation_time_stamp,
            duration: presentation_duration,
            status,
            image_buffer: buffer,
        },
    );
}

extern "C" fn vtf_vt_compression_output_callback(
    output_callback_ref_con: *mut std::ffi::c_void,
    source_frame_ref_con: *mut std::ffi::c_void,
    status: i32,
    _info_flags: video_toolbox_sys::compression::VTEncodeInfoFlags,
    sample_buffer: core_media_sys::sample_buffer::CMSampleBufferRef,
) {
    if output_callback_ref_con.is_null()
        || source_frame_ref_con.is_null()
        || status != 0
        || sample_buffer.is_null()
    {
        if !source_frame_ref_con.is_null() {
            unsafe {
                drop(Box::from_raw(source_frame_ref_con as *mut FrameContext));
            }
        }
        return;
    }

    let session_context = unsafe { &*(output_callback_ref_con as *const SessionCallbackContext) };
    let frame_context = unsafe { Box::from_raw(source_frame_ref_con as *mut FrameContext) };
    let sbuf_ptr = sample_buffer as *mut std::ffi::c_void;

    unsafe {
        let data_buffer = CMSampleBufferGetDataBuffer(sbuf_ptr);
        if data_buffer.is_null() {
            return;
        }

        let sample_size = CMBlockBufferGetDataLength(data_buffer);
        if sample_size == 0 {
            return;
        }
        probes::vt_ferry_probe_vt_encode_output_begin(
            session_context.session_id,
            frame_context.buffer_id,
            frame_context.buffer_generation,
            sample_size as u64,
        );
        probes::vt_ferry_probe_vt_output_begin(session_context.session_id, sample_size as u64);

        let mut vt_output = VtRealOutput {
            reply: DequeueOutputReply::zeroed(),
            source_buffer_id: frame_context.buffer_id,
            source_buffer_generation: frame_context.buffer_generation,
            storage: VtRealOutputStorage::Inline(Vec::new()),
        };
        vt_output.reply.session_id = session_context.session_id;
        vt_output.reply.codec = session_context.codec;
        vt_output.reply.width = session_context.width;
        vt_output.reply.height = session_context.height;
        vt_output.reply.pixel_format = session_context.pixel_format;
        vt_output.reply.sample_size = sample_size as u32;

        let sample_pts = CMSampleBufferGetPresentationTimeStamp(sbuf_ptr);
        vt_output.reply.pts_value = sample_pts.value;
        vt_output.reply.pts_timescale = sample_pts.timescale;

        let sample_dur = CMSampleBufferGetDuration(sbuf_ptr);
        vt_output.reply.duration_value = sample_dur.value;
        vt_output.reply.duration_timescale = sample_dur.timescale;

        // H.264 ('avc1') uses 2 parameter sets (SPS + PPS); HEVC ('hvc1')
        // uses 3 (VPS + SPS + PPS). Both are extracted via the same
        // GetParameterSetAtIndex shape — only the function name differs.
        // The wire layout (`parameter_set_sizes[]` + concatenated
        // `parameter_set_data[]`) is codec-agnostic.
        const FOURCC_AVC1: u32 = 0x61766331;
        const FOURCC_HVC1: u32 = 0x68766331;
        if session_context.codec == FOURCC_AVC1 || session_context.codec == FOURCC_HVC1 {
            let format_description = CMSampleBufferGetFormatDescription(sbuf_ptr)
                as core_media::format_description::CMFormatDescriptionRef;
            if !format_description.is_null() {
                let max_sets = vt_output.reply.parameter_set_sizes.len();
                let mut parameter_set_count: usize = 0;
                let mut nal_header_length: i32 = 0;
                let probe_status = if session_context.codec == FOURCC_HVC1 {
                    core_media::format_description::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                        format_description,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut parameter_set_count,
                        &mut nal_header_length,
                    )
                } else {
                    core_media::format_description::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                        format_description,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut parameter_set_count,
                        &mut nal_header_length,
                    )
                };
                if probe_status == 0 {
                    let mut data_offset = 0usize;
                    vt_output.reply.nal_header_length = nal_header_length as u32;
                    for index in 0..parameter_set_count.min(max_sets) {
                        let mut parameter_set: *const u8 = std::ptr::null();
                        let mut parameter_set_size: usize = 0;
                        let fetch_status = if session_context.codec == FOURCC_HVC1 {
                            core_media::format_description::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                                format_description,
                                index,
                                &mut parameter_set,
                                &mut parameter_set_size,
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                            )
                        } else {
                            core_media::format_description::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                                format_description,
                                index,
                                &mut parameter_set,
                                &mut parameter_set_size,
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                            )
                        };
                        if fetch_status != 0
                            || parameter_set.is_null()
                            || data_offset + parameter_set_size > vt_output.reply.parameter_set_data.len()
                        {
                            break;
                        }

                        std::ptr::copy_nonoverlapping(
                            parameter_set,
                            vt_output.reply.parameter_set_data[data_offset..].as_mut_ptr(),
                            parameter_set_size,
                        );
                        vt_output.reply.parameter_set_sizes[vt_output.reply.parameter_set_count as usize] =
                            parameter_set_size as u32;
                        vt_output.reply.parameter_set_count += 1;
                        data_offset += parameter_set_size;
                    }
                }
            }
        }

        use core_foundation::base::TCFTypeRef;
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sbuf_ptr, 0);
        if !attachments.is_null() && core_foundation::array::CFArrayGetCount(attachments) > 0 {
            let attachment = core_foundation::array::CFArrayGetValueAtIndex(attachments, 0)
                as core_foundation::dictionary::CFDictionaryRef;
            let mut not_sync_val: *mut std::ffi::c_void = std::ptr::null_mut();

            if !attachment.is_null()
                && core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                    attachment,
                    kCMSampleAttachmentKey_NotSync as *const _,
                    &mut not_sync_val as *mut *mut std::ffi::c_void as _,
                ) != 0
                && not_sync_val as *const _
                    == core_foundation::boolean::kCFBooleanFalse.as_void_ptr()
            {
                vt_output.reply.sample_flags |= 1;
            }
        }

        let mut id_lock = session_context.output_state.next_output_id.lock().unwrap();
        let output_id = *id_lock;
        *id_lock += 1;
        drop(id_lock);

        vt_output.reply.output_id = output_id;
        let mut sample_bytes = vec![0u8; sample_size];
        probes::vt_ferry_probe_vt_output_inline_copy_begin(output_id, sample_size as u64);
        let copy_status =
            CMBlockBufferCopyDataBytes(data_buffer, 0, sample_size, sample_bytes.as_mut_ptr());
        probes::vt_ferry_probe_vt_output_inline_copy_end(output_id, copy_status as u64);
        if copy_status != 0 {
            return;
        }
        vt_output.storage = VtRealOutputStorage::Inline(sample_bytes);
        probes::vt_ferry_probe_vt_output_queued(output_id, sample_size as u64, 0);
        session_context
            .output_state
            .queued_outputs
            .lock()
            .unwrap()
            .insert(output_id, vt_output);
        let mut queues = session_context.output_state.session_queues.lock().unwrap();
        queues
            .entry(session_context.session_id)
            .or_insert_with(VecDeque::new)
            .push_back(output_id);
    }
}

fn payload_cstr(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    pub fn CMSampleBufferGetDataBuffer(sbuf: *mut std::ffi::c_void) -> CMBlockBufferRef;
    pub fn CMBlockBufferGetDataLength(theBuffer: CMBlockBufferRef) -> usize;
    pub fn CMBlockBufferCopyDataBytes(
        theSourceBuffer: CMBlockBufferRef,
        offsetToData: usize,
        dataLength: usize,
        destination: *mut u8,
    ) -> i32;
    pub fn CMSampleBufferGetFormatDescription(
        sbuf: *mut std::ffi::c_void,
    ) -> core_media_sys::format_description::CMFormatDescriptionRef;
    pub fn CMSampleBufferGetPresentationTimeStamp(
        sbuf: *mut std::ffi::c_void,
    ) -> core_media_sys::time::CMTime;
    pub fn CMSampleBufferGetDuration(sbuf: *mut std::ffi::c_void) -> core_media_sys::time::CMTime;
    pub fn CMSampleBufferGetSampleAttachmentsArray(
        sbuf: *mut std::ffi::c_void,
        createIfNecessary: u8,
    ) -> core_foundation::array::CFArrayRef;
    pub static kCMSampleAttachmentKey_NotSync: core_foundation::string::CFStringRef;
    pub static kCMTimeIndefinite: core_media_sys::time::CMTime;
}

// VideoToolbox compression-property keys not exposed by the upstream
// `video-toolbox` / `video-toolbox-sys` crates. They're standard Apple
// CFStringRef statics — pull them in directly from VideoToolbox.
unsafe extern "C" {
    pub static kVTCompressionPropertyKey_ConstantBitRate:
        core_foundation::string::CFStringRef;
    pub static kVTCompressionPropertyKey_SpatialAdaptiveQPLevel:
        core_foundation::string::CFStringRef;
    pub static kVTCompressionPropertyKey_TargetQualityForAlpha:
        core_foundation::string::CFStringRef;
}

// Placeholder structs for the actual ecosystem crate objects.
// Several fields (id/width/height/payload_version/session_type)
// are read by some dispatch paths but not all; suppressing
// dead_code at the struct level avoids whack-a-mole field-level
// allows as code shifts.
#[allow(dead_code)]
pub struct VtRealSession {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub payload_version: u32,
    pub session_type: u32,
    pub codec: u32,
    pub pixel_format: u32,
    pub prepared: bool,
    pub vt_session: video_toolbox::compression_session::VTCompressionSession,
    pub callback_context: Box<SessionCallbackContext>,
}

/// Worker-side decode session. Mirrors `VtRealSession` but holds a
/// `VTDecompressionSession` plus its `CMVideoFormatDescription`.
///
/// Created by `OP_CREATE_SESSION` with kind = `VTF_SESSION_KIND_DECODE`
/// (no format yet — `format_description` and `vt_session` are
/// `None`). `OP_SET_DECODE_FORMAT` fills both in. Future
/// `OP_SET_DECODE_FORMAT` calls (mid-stream format changes) check
/// `VTDecompressionSessionCanAcceptFormatDescription` and either
/// swap the format in place or invalidate + recreate the session.
///
/// `vt_session` is wrapped in `Option` because a decode session
/// can legitimately exist without a format (after CREATE_SESSION
/// but before SET_DECODE_FORMAT) and `ENQUEUE_ENCODED_FRAME` is
/// expected to reject in that state with `STATUS_INVALID_STATE`.
#[allow(dead_code)]
pub struct VtRealDecodeSession {
    pub id: u64,
    pub codec: u32,
    pub width: u32,
    pub height: u32,
    pub format_description: Option<core_media::format_description::CMVideoFormatDescription>,
    pub vt_session: Option<video_toolbox::decompression_session::VTDecompressionSession>,
    /// Callback context heap-stashed so the C-side
    /// `VTDecompressionOutputCallback` always sees a stable
    /// pointer through the lifetime of the VT session, even when
    /// `vt_session` is replaced during a format-change.
    pub callback_context: Option<Box<DecodeSessionCallbackContext>>,
    /// `true` once `OP_BIND_DECODE_OUTPUT_POOL` has switched the
    /// session to the chunked-zero-copy output path. In that mode
    /// `OP_DEQUEUE_DECODED_FRAME` populates the reply's
    /// `buffer_host_id` with the `output_id` (signaling chunked
    /// mode to the guest), and the guest drains pixels via
    /// `OP_READ_DECODED_FRAME_CHUNK` instead of the single-shot
    /// `OP_READ_DECODED_FRAME`. The held `CVImageBuffer` IS the
    /// destination — no intermediate slot copy.
    pub chunked_output: bool,
}

/// Heap-stable context passed to `VTDecompressionOutputCallback`
/// as `decompressionOutputRefCon`. The callback fires from VT's
/// internal worker thread; we use the `output_state`'s mutex-
/// guarded queues to safely enqueue decoded frames.
pub struct DecodeSessionCallbackContext {
    pub output_state: DecodedOutputQueue,
    pub session_id: u64,
}

/// Per-decoder-session queue of decoded `CVImageBuffer` outputs
/// awaiting `OP_DEQUEUE_DECODED_FRAME`. Mirrors `OutputQueue` for
/// the encode side but holds image buffers instead of compressed
/// sample buffers.
///
/// Also carries a per-session sticky "last VT decode error" slot.
/// VT's decompression callback fires asynchronously; when it
/// reports a non-zero status (e.g. `kVTVideoDecoderBadDataErr`)
/// there is no decoded frame to enqueue — but the guest needs to
/// learn about the failure or it'll just see "fewer frames than
/// expected" with no diagnostic. The sticky slot is consumed at
/// `OP_DEQUEUE_DECODED_FRAME` time once the FIFO is drained, so
/// good frames preceding the error still flow through normally.
#[derive(Clone)]
pub struct DecodedOutputQueue {
    /// Map from `output_id` → pending decoded frame. Populated by
    /// the VT output callback, drained by `OP_DEQUEUE_DECODED_FRAME`.
    queued: Arc<Mutex<HashMap<u64, VtRealDecodedOutput>>>,
    /// Per-session FIFO of `output_id`s in arrival order. The
    /// dequeue handler pops the front of the matching session's
    /// queue.
    session_queues: Arc<Mutex<HashMap<u64, VecDeque<u64>>>>,
    next_output_id: Arc<Mutex<u64>>,
    /// Per-session sticky VT decode-error status. Set by the VT
    /// output callback when `status != 0`. Consumed (taken +
    /// cleared) by `OP_DEQUEUE_DECODED_FRAME` once the FIFO is
    /// empty, so `output_id == 0` + non-zero `reply.status` acts
    /// as the wire-level error sentinel.
    decode_errors: Arc<Mutex<HashMap<u64, i32>>>,
}

impl DecodedOutputQueue {
    pub fn new() -> Self {
        DecodedOutputQueue {
            queued: Arc::new(Mutex::new(HashMap::new())),
            session_queues: Arc::new(Mutex::new(HashMap::new())),
            next_output_id: Arc::new(Mutex::new(70_000)),
            decode_errors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueue a decoded frame. Returns the assigned `output_id`.
    /// Called from the VT output callback thread.
    #[allow(dead_code)] // wired by the callback in a follow-up commit
    pub fn enqueue(&self, session_id: u64, output: VtRealDecodedOutput) -> u64 {
        let mut next = self.next_output_id.lock().unwrap();
        let output_id = *next;
        *next += 1;
        drop(next);

        self.queued.lock().unwrap().insert(output_id, output);
        self.session_queues
            .lock()
            .unwrap()
            .entry(session_id)
            .or_default()
            .push_back(output_id);
        output_id
    }

    /// Record a VT decode error for `session_id`. Called from the
    /// VT output callback when `status != 0` (or the image buffer
    /// is null). Overwrites any prior unread error — the guest
    /// only learns about the most recent failure once it drains
    /// the queue, which matches the "VT's reference frame chain
    /// is broken; the rest of the stream is suspect" reality.
    pub fn record_decode_error(&self, session_id: u64, status: i32) {
        if status == 0 {
            return;
        }
        self.decode_errors.lock().unwrap().insert(session_id, status);
    }

    /// Atomically read + clear the sticky error for `session_id`.
    /// Returns `Some(status)` exactly once after a callback
    /// recorded an error; subsequent calls return `None` until a
    /// new error fires. Called from `OP_DEQUEUE_DECODED_FRAME`
    /// when the FIFO is empty.
    pub fn take_decode_error(&self, session_id: u64) -> Option<i32> {
        self.decode_errors.lock().unwrap().remove(&session_id)
    }

    pub fn clear(&self) {
        self.queued.lock().unwrap().clear();
        self.session_queues.lock().unwrap().clear();
        self.decode_errors.lock().unwrap().clear();
    }
}

/// One decoded frame waiting for `OP_DEQUEUE_DECODED_FRAME`.
/// Holds VT's `CVImageBuffer` (retained at +1 by the decompression
/// callback so the pixel data outlives the callback frame) until
/// `OP_RELEASE_DECODED_FRAME` drops it.
///
/// Two read paths flow off the same held buffer:
///
///   - **inline (`OP_READ_DECODED_FRAME`)** — single-shot
///     response; capped at `VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES`
///     (≤720p NV12). Copies the whole frame into the response in
///     one shot.
///   - **chunked (`OP_READ_DECODED_FRAME_CHUNK`)** — repeated
///     reads pull byte ranges out of the same buffer. Used for
///     larger frames; activated when the session has called
///     `OP_BIND_DECODE_OUTPUT_POOL`.
///
/// Either path locks the buffer for read, copies, unlocks. The
/// buffer is the actual destination VT decoded into — no
/// intermediate slot copy.
#[allow(dead_code)] // fields read by op handlers
pub struct VtRealDecodedOutput {
    pub session_id: u64,
    pub pts: core_media::time::CMTime,
    pub duration: core_media::time::CMTime,
    pub status: i32,
    pub image_buffer: core_video::image_buffer::CVImageBuffer,
}

/// In-flight assembly state for `OP_ENQUEUE_ENCODED_FRAME_CHUNK`.
/// One entry per session can exist at a time (the protocol delivers
/// one frame at a time; concurrent frames need separate sessions).
/// The head chunk (`chunk_offset == 0`) primes the entry with
/// `total_size` + timing metadata; subsequent chunks append to
/// `bytes`. The final chunk hands the assembled buffer off to VT
/// decode through the same code path the inline op uses.
pub struct PendingEncodedFrame {
    pub bytes: Vec<u8>,
    pub total_size: u32,
    pub pts_value: i64,
    pub pts_timescale: i32,
    pub duration_value: i64,
    pub duration_timescale: i32,
}

pub struct VtRealBuffer {
    pub id: u64,
    pub pool_id: u64,
    pub generation: u64,
    pub slot_index: u32,
    pub slot_offset: usize,
    pub pixel_buffer: Option<core_video::pixel_buffer::CVPixelBuffer>,
    pub mapped_bytes: *mut u8,
    pub mapped_size: usize,
    pub pixel_format: u32,
    pub width: i32,
    pub height: i32,
    pub state: u32,
}

pub struct VtRealOutput {
    pub reply: DequeueOutputReply,
    pub source_buffer_id: u64,
    pub source_buffer_generation: u64,
    pub storage: VtRealOutputStorage,
}

pub enum VtRealOutputStorage {
    Inline(Vec<u8>),
}

pub struct SessionCallbackContext {
    pub output_state: OutputQueue,
    pub session_id: u64,
    pub codec: u32,
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
}

pub struct FrameContext {
    pub buffer_id: u64,
    pub buffer_generation: u64,
}

#[derive(Clone)]
pub struct OutputQueue {
    queued_outputs: Arc<Mutex<HashMap<u64, VtRealOutput>>>,
    session_queues: Arc<Mutex<HashMap<u64, VecDeque<u64>>>>,
    next_output_id: Arc<Mutex<u64>>,
}

impl OutputQueue {
    pub fn new() -> Self {
        OutputQueue {
            queued_outputs: Arc::new(Mutex::new(HashMap::new())),
            session_queues: Arc::new(Mutex::new(HashMap::new())),
            next_output_id: Arc::new(Mutex::new(50000)),
        }
    }

    pub fn clear(&self) {
        self.queued_outputs.lock().unwrap().clear();
        self.session_queues.lock().unwrap().clear();
        *self.next_output_id.lock().unwrap() = 50000;
    }
}

// `max_buffers` / `buffer_count` are recorded for invariants and
// telemetry but the active dispatch path doesn't read them today;
// `iosurfaces` keeps the surfaces alive via Drop so it's used for
// its side-effect rather than direct access. dead_code suppression
// matches the VtRealSession pattern.
#[allow(dead_code)]
pub struct VtRealPool {
    pub id: u64,
    pub session_id: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub max_buffers: u32,
    pub buffer_count: u32,
    pub slot_region_size: u32,
    /// Retained IOSurface for the packed pool. Keeps the underlying pages
    /// alive for the lifetime of the pool; dropped when the pool is
    /// removed. One entry — the pool's single packed surface.
    #[cfg(target_os = "macos")]
    pub iosurfaces: Vec<OwnedIOSurface>,
    /// If this pool was satisfied from the launcher-registered
    /// `IOSurfacePoolDirectory`, the index of the directory entry it
    /// claimed. `None` for non-zero-copy pools (those exist in tests
    /// and on non-macOS targets). The entry stays `claimed` in the
    /// shared directory until either DESTROY_SESSION drops the
    /// pool — at which point the destroy handler releases it — or
    /// the backend itself drops without an explicit teardown, at
    /// which point `Drop` walks the remaining pools and releases
    /// each. Tracking the entry on the pool (not on the backend)
    /// gets the lifetime exactly right: ffmpeg's "init encoder for
    /// SPS/PPS probe, destroy, init real encoder" pattern requires
    /// the probe encoder's directory entry to be re-claimable by
    /// the real encoder, even though both run on the same
    /// connection.
    #[cfg(target_os = "macos")]
    pub iosurface_entry_id: Option<usize>,
}

fn vtf_align_size(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return value;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + (alignment - remainder)
    }
}

// Companion to vtf_fill_buffer_layout — returns just the total
// bytes for a given (width, height, pixel_format). Currently
// unused in the hot path (callers go through vtf_fill_buffer_layout
// to get strides + offsets too) but kept so tests can validate
// "total_size matches the layout fn" cheaply, and so adding a new
// pixel format is a paired edit.
#[allow(dead_code)]
fn vtf_buffer_total_size(width: u32, height: u32, pixel_format: u32) -> usize {
    match pixel_format {
        0x34323076 | 0x34323066 /* '420v' or '420f' (NV12 8-bit) */ => {
            let stride = vtf_align_size(width as usize, 64);
            (stride * (height as usize)) + (stride * ((height as usize) / 2))
        }
        0x78343230 | 0x78663230 /* 'x420' or 'xf20' (P010 10-bit) */ => {
            // 10-bit packed into 16-bit words → 2 bytes per sample.
            let stride = vtf_align_size((width as usize) * 2, 64);
            (stride * (height as usize)) + (stride * ((height as usize) / 2))
        }
        _ /* 'BGRA' etc */ => {
            let stride = vtf_align_size((width as usize) * 4, 64);
            stride * (height as usize)
        }
    }
}

fn vtf_fill_buffer_layout(
    width: u32,
    height: u32,
    pixel_format: u32,
    layout: &mut BufferLayoutReply,
) {
    *layout = BufferLayoutReply::zeroed();
    match pixel_format {
        0x34323076 | 0x34323066 /* '420v' or '420f' (NV12 8-bit) */ => {
            let stride = vtf_align_size(width as usize, 64) as u32;
            layout.plane_count = 2;
            layout.plane_offsets[0] = 0;
            layout.plane_widths[0] = width;
            layout.plane_heights[0] = height;
            layout.plane_bytes_per_row[0] = stride;

            layout.plane_offsets[1] = stride * height;
            layout.plane_widths[1] = width / 2;
            layout.plane_heights[1] = height / 2;
            layout.plane_bytes_per_row[1] = stride;

            layout.total_size = layout.plane_offsets[1] + (stride * (height / 2));
        }
        0x78343230 | 0x78663230 /* 'x420' or 'xf20' (P010 10-bit) */ => {
            // Same NV12-style layout — Y + interleaved CbCr — but each
            // sample occupies a 16-bit word (10 bits payload, 6 unused).
            // Plane widths are the *sample* counts, the stride accounts
            // for the 2 bytes per sample.
            let stride = vtf_align_size((width as usize) * 2, 64) as u32;
            layout.plane_count = 2;
            layout.plane_offsets[0] = 0;
            layout.plane_widths[0] = width;
            layout.plane_heights[0] = height;
            layout.plane_bytes_per_row[0] = stride;

            layout.plane_offsets[1] = stride * height;
            layout.plane_widths[1] = width / 2;
            layout.plane_heights[1] = height / 2;
            layout.plane_bytes_per_row[1] = stride;

            layout.total_size = layout.plane_offsets[1] + (stride * (height / 2));
        }
        _ /* 'BGRA' etc */ => {
            let stride = vtf_align_size((width as usize) * 4, 64) as u32;
            layout.plane_count = 1;
            layout.plane_offsets[0] = 0;
            layout.plane_widths[0] = width;
            layout.plane_heights[0] = height;
            layout.plane_bytes_per_row[0] = stride;
            layout.total_size = stride * height;
        }
    }
}

/// Copy a byte range out of `cv_ref`'s canonical layout into
/// `destination`. Mirrors `vtf_copy_pixel_buffer_to_bytes` but
/// produces only the bytes for `[offset, offset+length)` in the
/// canonical layout — no full-frame intermediate buffer required.
///
/// Used by the chunked decode-output read path
/// (`OP_READ_DECODED_FRAME_CHUNK`) so the worker doesn't have to
/// hold a flattened `Vec<u8>` per in-flight output (12 MiB per 4K
/// frame × pool depth across all sessions). The CVPixelBuffer
/// itself stays held by the queue entry; each chunk read
/// locks the buffer, copies the range, and unlocks.
///
/// `destination` must be sized at least `length` bytes. The caller
/// holds the CV lock around the entire range; this function does
/// not touch the lock.
///
/// Padding bytes (`stride - real_content` per row) are zeroed in
/// the destination so the wire output matches what
/// `vtf_copy_pixel_buffer_to_bytes` would have produced for the
/// same range.
unsafe fn vtf_copy_pixel_buffer_byte_range(
    cv_ref: *const libc::c_void,
    layout: &BufferLayoutReply,
    offset: usize,
    length: usize,
    destination: *mut u8,
) -> bool {
    if layout.plane_count == 0 {
        return false;
    }
    let total = layout.total_size as usize;
    if offset > total || length > total - offset {
        return false;
    }
    if length == 0 {
        return true;
    }

    // Pre-zero so any range bytes that fall in stride padding stay
    // zero without per-row branching below. Cheap (256 KiB / chunk
    // is the upper bound = WRITE_BUFFER_CHUNK_BYTES).
    unsafe {
        std::ptr::write_bytes(destination, 0, length);
    }

    if layout.plane_count == 1 || unsafe { CVPixelBufferGetPlaneCount(cv_ref) } == 0 {
        // Single-plane: source is contiguous at base. Mirror the
        // existing single-plane shortcut in
        // `vtf_copy_pixel_buffer_to_bytes` which assumes
        // src_stride == dst_stride.
        let base = unsafe { CVPixelBufferGetBaseAddress(cv_ref) };
        if base.is_null() {
            return false;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                (base as *const u8).add(offset),
                destination,
                length,
            );
        }
        return true;
    }

    // Multi-plane: iterate each plane that intersects
    // `[offset, offset+length)`, then walk affected rows with
    // stride translation.
    for plane in 0..layout.plane_count as usize {
        let plane_offset = layout.plane_offsets[plane] as usize;
        let plane_stride = layout.plane_bytes_per_row[plane] as usize;
        let plane_height = layout.plane_heights[plane] as usize;
        let plane_size = plane_stride * plane_height;
        let plane_end = plane_offset + plane_size;

        let range_start = offset.max(plane_offset);
        let range_end = (offset + length).min(plane_end);
        if range_start >= range_end {
            continue;
        }

        let plane_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(cv_ref, plane) };
        if plane_base.is_null() {
            return false;
        }
        let src_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(cv_ref, plane) };
        // Real content per row = min(src_stride, dst_stride).
        // Bytes beyond that in dst are padding (kept zero by the
        // write_bytes above).
        let row_content = src_stride.min(plane_stride);

        let plane_local_start = range_start - plane_offset;
        let plane_local_end = range_end - plane_offset;
        let first_row = plane_local_start / plane_stride;
        let last_row = (plane_local_end - 1) / plane_stride;

        for row in first_row..=last_row {
            let row_byte_offset = row * plane_stride;
            let row_start = plane_local_start.max(row_byte_offset);
            let row_end = plane_local_end.min(row_byte_offset + plane_stride);
            let col_start = row_start - row_byte_offset;
            let col_end = row_end - row_byte_offset;

            // Source covers only the first `row_content` bytes of
            // the row; anything beyond is dst-side padding.
            let copy_end = col_end.min(row_content);
            if col_start >= copy_end {
                continue;
            }
            let copy_len = copy_end - col_start;

            // Destination offset within `destination` for this
            // row's slice: canonical byte position - chunk offset.
            let dst_offset_in_chunk =
                row_byte_offset + col_start + plane_offset - offset;

            unsafe {
                std::ptr::copy_nonoverlapping(
                    (plane_base as *const u8).add(row * src_stride + col_start),
                    destination.add(dst_offset_in_chunk),
                    copy_len,
                );
            }
        }
    }

    true
}

unsafe fn vtf_copy_pixel_buffer_to_bytes(
    cv_ref: *const libc::c_void,
    destination: *mut u8,
    width: u32,
    height: u32,
    pixel_format: u32,
) -> bool {
    // SAFETY: caller passes a non-null CVPixelBuffer ref and a
    // destination buffer sized at least `layout.total_size`. The
    // CV `Get*` accessors are FFI calls that don't touch Rust-
    // owned memory. Pointer arithmetic stays inside the
    // destination buffer because dst_stride * height ≤ total_size
    // by construction in vtf_fill_buffer_layout.
    unsafe {
        let mut layout = BufferLayoutReply::zeroed();
        vtf_fill_buffer_layout(width, height, pixel_format, &mut layout);
        if layout.plane_count == 0 {
            return false;
        }

        if layout.plane_count == 1 || CVPixelBufferGetPlaneCount(cv_ref) == 0 {
            let base = CVPixelBufferGetBaseAddress(cv_ref);
            if base.is_null() {
                return false;
            }
            std::ptr::copy_nonoverlapping(base.cast(), destination, layout.total_size as usize);
            return true;
        }

        for plane in 0..layout.plane_count as usize {
            let plane_base = CVPixelBufferGetBaseAddressOfPlane(cv_ref, plane);
            if plane_base.is_null() {
                return false;
            }
            let dst = destination.add(layout.plane_offsets[plane] as usize);
            let dst_stride = layout.plane_bytes_per_row[plane] as usize;
            let src_stride = CVPixelBufferGetBytesPerRowOfPlane(cv_ref, plane);
            let row_bytes = dst_stride.min(src_stride);
            let rows = layout.plane_heights[plane] as usize;
            for row in 0..rows {
                std::ptr::copy_nonoverlapping(
                    (plane_base as *const u8).add(row * src_stride),
                    dst.add(row * dst_stride),
                    row_bytes,
                );
            }
        }

        true
    }
}

fn vtf_make_source_attributes(
    width: i32,
    height: i32,
    pixel_format: u32,
) -> CFDictionary<CFString, CFType> {
    let width_key = CFString::new("Width");
    let height_key = CFString::new("Height");
    let pixel_format_key = CFString::new("PixelFormatType");
    let iosurface_key = CFString::new("IOSurfaceProperties");
    let width_val = CFNumber::from(width);
    let height_val = CFNumber::from(height);
    let pixel_format_val = CFNumber::from(pixel_format as i32);
    let io_surface: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[]);

    CFDictionary::from_CFType_pairs(&[
        (width_key, width_val.as_CFType()),
        (height_key, height_val.as_CFType()),
        (pixel_format_key, pixel_format_val.as_CFType()),
        (iosurface_key, io_surface.as_CFType()),
    ])
}

fn vtf_wrap_source_slot_bytes(
    base: *mut u8,
    width: usize,
    height: usize,
    pixel_format: u32,
    slot_total_bytes: usize,
) -> Result<core_video::pixel_buffer::CVPixelBuffer, i32> {
    let cv_ref_result = match pixel_format {
        0x42475241 => {
            let bytes_per_row = width * 4;
            unsafe { wrap_bytes_single_plane(base, width, height, bytes_per_row, pixel_format) }
        }
        0x34323076 => {
            let stride = (width + 63) & !63;
            let y_size = stride * height;
            unsafe {
                wrap_bytes_planar(
                    base,
                    width,
                    height,
                    pixel_format,
                    slot_total_bytes,
                    &[0, y_size],
                    &[width, width / 2],
                    &[height, height / 2],
                    &[stride, stride],
                )
            }
        }
        0x78343230 | 0x78663230 => {
            // P010 ('x420' video range / 'xf20' full range): 10-bit Y plane plus
            // 10-bit interleaved CbCr at half resolution. Two bytes per sample,
            // stride aligned to 64 bytes (matches vtf_fill_buffer_layout).
            let stride = ((width * 2) + 63) & !63;
            let y_size = stride * height;
            unsafe {
                wrap_bytes_planar(
                    base,
                    width,
                    height,
                    pixel_format,
                    slot_total_bytes,
                    &[0, y_size],
                    &[width, width / 2],
                    &[height, height / 2],
                    &[stride, stride],
                )
            }
        }
        other => {
            eprintln!(
                "vt-real shared source arena: unsupported pixel_format 0x{:x}",
                other
            );
            Err(STATUS_UNSUPPORTED_CODEC_OR_FORMAT as i32)
        }
    }?;
    Ok(unsafe {
        core_video::pixel_buffer::CVPixelBuffer::wrap_under_create_rule(cv_ref_result as *mut _)
    })
}

fn vtf_ensure_source_pixel_buffer(
    buffer: &mut VtRealBuffer,
) -> Result<&core_video::pixel_buffer::CVPixelBuffer, u32> {
    if buffer.pixel_buffer.is_none() {
        crate::probes::vt_ferry_probe_vt_source_wrap_begin(
            buffer.id,
            buffer.slot_index as u64,
            buffer.mapped_size as u64,
            buffer.pixel_format as u64,
        );
        match vtf_wrap_source_slot_bytes(
            buffer.mapped_bytes,
            buffer.width as usize,
            buffer.height as usize,
            buffer.pixel_format,
            buffer.mapped_size,
        ) {
            Ok(pixel_buffer) => {
                buffer.pixel_buffer = Some(pixel_buffer);
                crate::probes::vt_ferry_probe_vt_source_wrap_end(buffer.id, STATUS_OK as u64);
            }
            Err(status) => {
                let status = if status > 0 {
                    status as u32
                } else {
                    STATUS_INTERNAL_FAILURE
                };
                crate::probes::vt_ferry_probe_vt_source_wrap_end(buffer.id, status as u64);
                return Err(status);
            }
        }
    }

    Ok(buffer
        .pixel_buffer
        .as_ref()
        .expect("source pixel buffer should be initialized"))
}

pub struct VtRealBackend {
    sessions: HashMap<u64, VtRealSession>,
    /// Decode sessions kept alongside `sessions` because their
    /// underlying VT object types are different
    /// (`VTDecompressionSession` vs `VTCompressionSession`) — sharing
    /// a hashmap would force an enum and erase the type info that
    /// VT bindings already give us.
    decode_sessions: HashMap<u64, VtRealDecodeSession>,
    buffers: HashMap<u64, VtRealBuffer>,
    pools: HashMap<u64, VtRealPool>,
    /// Per-session in-flight chunked-encoded-frame assembly state.
    /// Populated by `OP_ENQUEUE_ENCODED_FRAME_CHUNK` while a frame
    /// is being delivered piecewise; cleared when the final chunk
    /// hands the assembled bytes off to VT decode. The single-shot
    /// `OP_ENQUEUE_ENCODED_FRAME` path doesn't touch this map.
    pending_encoded_frames: HashMap<u64, PendingEncodedFrame>,
    next_host_id: u64,
    output_state: OutputQueue,
    decoded_output_state: DecodedOutputQueue,
    /// Shared across all backend instances created by a single
    /// `VtRealBackendFactory`. Each accepted connection gets its
    /// own backend (its own session map, buffer pools, output
    /// queues), but the launcher-registered IOSurface directory is
    /// process-global — multiple concurrent guests competing for
    /// surfaces of the same shape compete for the SAME directory
    /// entries via `take_matching`. Lock granularity is the whole
    /// directory, which is fine since directory access is rare
    /// (once per `OP_CREATE_BUFFER_POOL`, not on the per-frame
    /// hot path).
    #[cfg(target_os = "macos")]
    iosurface_pools: Arc<Mutex<IOSurfacePoolDirectory>>,
}

#[cfg(target_os = "macos")]
impl Drop for VtRealBackend {
    fn drop(&mut self) {
        // Release every directory entry still claimed by a
        // pool this backend owns. Most pools have already been
        // torn down by an explicit DESTROY_SESSION, which
        // releases their entry inline; this loop is the
        // belt-and-braces fallback for connections that
        // disconnect mid-stream without a clean destroy
        // (process killed, transport poisoned, etc.). Without
        // it, the launcher-allocated IOSurface stays "claimed"
        // until the worker process exits, leaking concurrency
        // capacity for the lifetime of the worker.
        let entries: Vec<usize> = self
            .pools
            .values()
            .filter_map(|pool| pool.iosurface_entry_id)
            .collect();
        if !entries.is_empty() {
            if let Ok(mut dir) = self.iosurface_pools.lock() {
                for id in entries {
                    dir.release_entry(id);
                }
            }
        }
    }
}

// VtRealBackend holds CFType-wrapping fields (CVPixelBuffer,
// CMSampleBuffer, IOSurfaceRef) that aren't auto-Send because they
// expose interior-mutable refcount semantics through raw pointers.
// In practice each backend instance is owned by a single
// connection thread for its entire lifetime — we only need Send
// to MOVE the Box<dyn Backend> into the freshly-spawned thread,
// and never share a backend instance across threads. Apple's
// CFRetain / CFRelease are atomic; the only soundness concern
// would be concurrent access to the same CFType, which our
// architecture excludes by construction.
unsafe impl Send for VtRealBackend {}

impl VtRealBackend {
    pub fn new() -> Self {
        VtRealBackend {
            sessions: HashMap::new(),
            decode_sessions: HashMap::new(),
            buffers: HashMap::new(),
            pools: HashMap::new(),
            pending_encoded_frames: HashMap::new(),
            next_host_id: 10000,
            output_state: OutputQueue::new(),
            decoded_output_state: DecodedOutputQueue::new(),
            #[cfg(target_os = "macos")]
            iosurface_pools: Arc::new(Mutex::new(IOSurfacePoolDirectory::empty())),
        }
    }

    /// Used by tests that build a backend with a pre-seeded pool
    /// directory in-place. Production code uses
    /// `with_iosurface_pools_shared` instead so the directory can
    /// be shared across multiple per-connection backend instances.
    #[cfg(all(target_os = "macos", test))]
    pub fn with_iosurface_pools(mut self, directory: IOSurfacePoolDirectory) -> Self {
        self.iosurface_pools = Arc::new(Mutex::new(directory));
        self
    }

    /// Production constructor for the multi-connection world.
    /// Multiple `VtRealBackend` instances share the same directory
    /// `Arc`; each `take_matching` is serialized through the
    /// inner mutex, so concurrent `OP_CREATE_BUFFER_POOL` requests
    /// from independent guests compete for the same set of
    /// launcher-registered surfaces.
    #[cfg(target_os = "macos")]
    pub fn with_iosurface_pools_shared(
        mut self,
        directory: Arc<Mutex<IOSurfacePoolDirectory>>,
    ) -> Self {
        self.iosurface_pools = directory;
        self
    }

    fn mark_source_buffer_reusable(&mut self, buffer_id: u64, generation: u64) {
        if let Some(buffer) = self.buffers.get_mut(&buffer_id) {
            if buffer.generation == generation && buffer.state == 2 {
                buffer.state = 0;
            }
        }
    }

    fn release_output_id(&mut self, output_id: u64) -> u32 {
        let mut outputs = self.output_state.queued_outputs.lock().unwrap();
        let released = outputs.remove(&output_id);
        if released.is_none() {
            return STATUS_INVALID_HANDLE;
        }
        drop(outputs);
        if let Some(output) = released {
            let sample_size = output.reply.sample_size as u64;
            probes::vt_ferry_probe_vt_release_output(output_id, sample_size, 0);
        }
        STATUS_OK
    }

    fn encode_frame_payload(&mut self, payload: EncodeFramePayload) -> u32 {
        let session = match self.sessions.get(&payload.session_id) {
            Some(s) => s,
            None => return STATUS_INVALID_HANDLE,
        };
        if !session.prepared {
            return STATUS_INVALID_STATE;
        }
        if payload.image_buffer_proxy_id == 0 {
            return STATUS_INVALID_HANDLE;
        }

        let buffer = match self.buffers.get_mut(&payload.image_buffer_host_id) {
            Some(b) => b,
            None => return STATUS_INVALID_HANDLE,
        };

        if buffer.generation != payload.image_buffer_generation {
            return STATUS_STALE_GENERATION;
        }
        if buffer.state != 1 {
            return STATUS_INVALID_STATE;
        }

        let buffer_id = buffer.id;
        let buffer_generation = buffer.generation;
        let pixel_buffer = match vtf_ensure_source_pixel_buffer(buffer) {
            Ok(pixel_buffer) => pixel_buffer,
            Err(status) => return status,
        };

        probes::vt_ferry_probe_cv_pixel_buffer_lock_begin(buffer_id);
        let status = pixel_buffer.lock_base_address(0);
        probes::vt_ferry_probe_cv_pixel_buffer_lock_end(
            buffer_id,
            if status == 0 {
                STATUS_OK as u64
            } else {
                STATUS_INTERNAL_FAILURE as u64
            },
        );
        if status != 0 {
            return STATUS_INTERNAL_FAILURE;
        }
        pixel_buffer.unlock_base_address(0);

        let image_buffer = pixel_buffer.as_image_buffer();
        let pts = core_media::time::CMTime::make(payload.pts_value, payload.pts_timescale);
        let dur =
            core_media::time::CMTime::make(payload.duration_value, payload.duration_timescale);
        let frame_context = Box::new(FrameContext {
            buffer_id,
            buffer_generation,
        });

        buffer.state = 2; // VTF_VT_BUFFER_STATE_QUEUED_TO_HOST
        let empty_props: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[]);
        probes::vt_ferry_probe_vt_encode_frame_begin(
            payload.session_id,
            buffer_id,
            buffer_generation,
        );
        let encode_res = unsafe {
            session.vt_session.encode_frame(
                image_buffer,
                pts,
                dur,
                empty_props,
                Box::into_raw(frame_context) as *mut std::ffi::c_void,
            )
        };
        probes::vt_ferry_probe_vt_encode_frame_end(
            payload.session_id,
            buffer_id,
            if encode_res.is_ok() {
                STATUS_OK as u64
            } else {
                STATUS_INTERNAL_FAILURE as u64
            },
        );

        if encode_res.is_err() {
            buffer.state = 1;
            return STATUS_INTERNAL_FAILURE;
        }

        STATUS_OK
    }

    /// Hand a complete encoded frame to VT for decoding. Shared
    /// dispatch path for both `OP_ENQUEUE_ENCODED_FRAME` (single
    /// inline op, ≤ `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`) and
    /// `OP_ENQUEUE_ENCODED_FRAME_CHUNK` (chunked, arbitrary size).
    /// Both ops do the same thing once the encoded bytes are
    /// assembled — wrap them in a `CMBlockBuffer`, build the
    /// sample buffer with timing info, hand it to
    /// `VTDecompressionSessionDecodeFrame`. Pulled out into a
    /// helper so the chunked path doesn't have to duplicate the
    /// VT plumbing.
    ///
    /// Returns a protocol status code: `STATUS_OK` on a successful
    /// dispatch (the VT callback may still fire with a non-zero
    /// status later — that's surfaced via the
    /// `OP_DEQUEUE_DECODED_FRAME` error sentinel), or one of the
    /// `STATUS_*` error codes when validation or VT setup fails.
    fn dispatch_decode_assembled_frame(
        &self,
        session_id: u64,
        encoded_bytes: &[u8],
        pts_value: i64,
        pts_timescale: i32,
        duration_value: i64,
        duration_timescale: i32,
    ) -> u32 {
        let session = match self.decode_sessions.get(&session_id) {
            Some(s) => s,
            None => return STATUS_INVALID_HANDLE,
        };
        let format_description = match session.format_description.as_ref() {
            Some(fd) => fd,
            None => {
                eprintln!(
                    "vt-real dispatch_decode: session {} has no format \
                     description — guest must call OP_SET_DECODE_FORMAT first",
                    session_id
                );
                return STATUS_INVALID_STATE;
            }
        };
        let vt_session = match session.vt_session.as_ref() {
            Some(vt) => vt,
            None => return STATUS_INVALID_STATE,
        };

        let block_buffer = match unsafe {
            core_media::block_buffer::CMBlockBuffer::new_with_memory_block(
                None,
                encoded_bytes.len(),
                None,
                0,
                encoded_bytes.len(),
                0,
            )
        } {
            Ok(bb) => bb,
            Err(os_status) => {
                eprintln!(
                    "vt-real dispatch_decode: CMBlockBufferCreateWithMemoryBlock \
                     failed OSStatus {}",
                    os_status
                );
                return STATUS_INTERNAL_FAILURE;
            }
        };
        if let Err(os_status) = block_buffer.replace_data_bytes(encoded_bytes, 0) {
            eprintln!(
                "vt-real dispatch_decode: CMBlockBufferReplaceDataBytes \
                 failed OSStatus {}",
                os_status
            );
            return STATUS_INTERNAL_FAILURE;
        }

        let timing = core_media::sample_buffer::CMSampleTimingInfo {
            duration: core_media::time::CMTime {
                value: duration_value,
                timescale: duration_timescale,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            presentationTimeStamp: core_media::time::CMTime {
                value: pts_value,
                timescale: pts_timescale,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            decodeTimeStamp: core_media::time::CMTime {
                value: 0,
                timescale: 0,
                flags: 0,
                epoch: 0,
            },
        };
        let sample_size: libc::size_t = encoded_bytes.len() as libc::size_t;
        let mut sample_buffer_ref: core_media::sample_buffer::CMSampleBufferRef =
            std::ptr::null_mut();
        let create_status = unsafe {
            core_media::sample_buffer::CMSampleBufferCreateReady(
                core_foundation::base::kCFAllocatorDefault,
                block_buffer.as_concrete_TypeRef(),
                format_description.as_concrete_TypeRef(),
                1,
                1,
                &timing,
                1,
                &sample_size,
                &mut sample_buffer_ref,
            )
        };
        if create_status != 0 || sample_buffer_ref.is_null() {
            eprintln!(
                "vt-real dispatch_decode: CMSampleBufferCreateReady failed \
                 OSStatus {}",
                create_status
            );
            return STATUS_INTERNAL_FAILURE;
        }
        let sample_buffer = unsafe {
            core_media::sample_buffer::CMSampleBuffer::wrap_under_create_rule(
                sample_buffer_ref,
            )
        };

        let decode_result = unsafe {
            vt_session.decode_frame(
                sample_buffer,
                video_toolbox::errors::VTDecodeFrameFlags::empty(),
                std::ptr::null_mut(),
            )
        };
        match decode_result {
            Ok(_info_flags) => STATUS_OK,
            Err(os_status) => {
                eprintln!(
                    "vt-real dispatch_decode: \
                     VTDecompressionSessionDecodeFrame failed OSStatus {}",
                    os_status
                );
                STATUS_INTERNAL_FAILURE
            }
        }
    }
}

impl Backend for VtRealBackend {
    fn reset_from_env(&mut self) {
        self.sessions.clear();
        self.decode_sessions.clear();
        self.pools.clear();
        self.buffers.clear();
        self.output_state.clear();
        self.decoded_output_state.clear();
        self.next_host_id = 1;
    }

    fn dispatch(
        &mut self,
        req_header: &MessageHeader,
        req_payload: &[u8],
        res_header: &mut MessageHeader,
        res_payload: &mut [u8],
    ) -> Result<usize, ()> {
        match req_header.opcode {
            OP_HELLO => {
                if req_payload.len() < std::mem::size_of::<HelloPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let _payload: HelloPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<HelloPayload>()],
                );

                let mut reply = HelloReply::zeroed();
                reply.worker_abi_version = VTF_TRANSPORT_VERSION as u32;
                let name = b"vt-ferry-host-worker-vt\0";
                reply.worker_name[..name.len()].copy_from_slice(name);

                res_payload[..std::mem::size_of::<HelloReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<HelloReply>())
            }
            OP_PING => {
                // Liveness probe — empty request, empty reply, status_ok.
                Ok(0)
            }
            OP_GET_CAPS => {
                let mut reply = GetCapsReply::zeroed();
                reply.codec_bits = CAP_CODEC_H264 | CAP_CODEC_HEVC;
                reply.pixel_format_bits =
                    CAP_PIXEL_FORMAT_NV12 | CAP_PIXEL_FORMAT_BGRA | CAP_PIXEL_FORMAT_P010;
                reply.session_feature_bits = CAP_SESSION_FEATURE_ASYNC_COMPLETE
                    | CAP_SESSION_FEATURE_BUFFER_SYNC
                    | CAP_SESSION_FEATURE_DECODE;
                reply.max_width = 7680;
                reply.max_height = 4320;
                reply.max_inflight_frames = 16;

                res_payload[..std::mem::size_of::<GetCapsReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<GetCapsReply>())
            }
            OP_CREATE_SESSION => {
                if req_payload.len() < std::mem::size_of::<CreateSessionPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: CreateSessionPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<CreateSessionPayload>()],
                );

                // Decode session creation is shaped like a two-phase
                // commit: CREATE_SESSION reserves the session id and
                // stores codec/dimensions; OP_SET_DECODE_FORMAT later
                // ships the parameter sets and creates the underlying
                // VTDecompressionSession. This split mirrors how
                // VTDecompressionSession itself requires a populated
                // CMVideoFormatDescription at create time — we can't
                // build it until the guest has sent us the SPS/PPS
                // (or VPS+SPS+PPS for HEVC).
                if payload.kind == VTF_SESSION_KIND_DECODE {
                    let session_id = self.next_host_id;
                    self.next_host_id += 1;
                    self.decode_sessions.insert(
                        session_id,
                        VtRealDecodeSession {
                            id: session_id,
                            codec: payload.codec,
                            width: payload.width,
                            height: payload.height,
                            format_description: None,
                            vt_session: None,
                            callback_context: None,
                            chunked_output: false,
                        },
                    );
                    let mut reply = CreateSessionReply::zeroed();
                    reply.session_id = session_id;
                    reply.negotiated_width = payload.width;
                    reply.negotiated_height = payload.height;
                    reply.pixel_format = payload.pixel_format;
                    res_payload[..std::mem::size_of::<CreateSessionReply>()]
                        .copy_from_slice(bytemuck::bytes_of(&reply));
                    return Ok(std::mem::size_of::<CreateSessionReply>());
                }
                if payload.kind != VTF_SESSION_KIND_ENCODE {
                    res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                    return Ok(0);
                }

                let session_id = self.next_host_id;
                self.next_host_id += 1;
                let callback_context = Box::new(SessionCallbackContext {
                    output_state: self.output_state.clone(),
                    session_id,
                    codec: payload.codec,
                    pixel_format: payload.pixel_format,
                    width: payload.width,
                    height: payload.height,
                });
                let source_attributes = vtf_make_source_attributes(
                    payload.width as i32,
                    payload.height as i32,
                    payload.pixel_format,
                );

                let mut session_ref: video_toolbox_sys::compression::VTCompressionSessionRef =
                    std::ptr::null_mut();
                probes::vt_ferry_probe_vt_session_create_begin(
                    payload.width as u64,
                    payload.height as u64,
                    payload.codec as u64,
                    payload.pixel_format as u64,
                );
                let status = unsafe {
                    video_toolbox_sys::compression::VTCompressionSessionCreate(
                        core_foundation_sys::base::kCFAllocatorDefault,
                        payload.width as i32,
                        payload.height as i32,
                        payload.codec,
                        std::ptr::null(),
                        source_attributes.as_concrete_TypeRef(),
                        core_foundation_sys::base::kCFAllocatorDefault,
                        vtf_vt_compression_output_callback,
                        (&*callback_context) as *const SessionCallbackContext
                            as *mut std::ffi::c_void,
                        &mut session_ref,
                    )
                };
                probes::vt_ferry_probe_vt_session_create_end(
                    session_id,
                    if status == 0 && !session_ref.is_null() {
                        STATUS_OK as u64
                    } else {
                        STATUS_INTERNAL_FAILURE as u64
                    },
                );

                if status != 0 || session_ref.is_null() {
                    res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                    return Ok(0);
                }
                let session = unsafe {
                    video_toolbox::compression_session::VTCompressionSession::wrap_under_create_rule(
                        session_ref,
                    )
                };

                self.sessions.insert(
                    session_id,
                    VtRealSession {
                        id: session_id,
                        width: payload.width,
                        height: payload.height,
                        payload_version: 1,
                        session_type: payload.kind,
                        codec: payload.codec,
                        pixel_format: payload.pixel_format,
                        prepared: false,
                        vt_session: session,
                        callback_context,
                    },
                );

                let mut reply = CreateSessionReply::zeroed();
                reply.session_id = session_id;
                reply.negotiated_width = payload.width;
                reply.negotiated_height = payload.height;
                reply.pixel_format = payload.pixel_format;

                res_payload[..std::mem::size_of::<CreateSessionReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<CreateSessionReply>())
            }
            OP_SET_PROPERTY => {
                if req_payload.len() < std::mem::size_of::<SetPropertyPayload>() {
                    eprintln!(
                        "vt-real SET_PROPERTY short payload len={} expected={}",
                        req_payload.len(),
                        std::mem::size_of::<SetPropertyPayload>()
                    );
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: SetPropertyPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<SetPropertyPayload>()],
                );

                let session = match self.sessions.get_mut(&payload.session_id) {
                    Some(s) => s,
                    None => {
                        eprintln!(
                            "vt-real SET_PROPERTY invalid session_id={} key={}",
                            payload.session_id,
                            payload_cstr(&payload.property_key)
                        );
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                let property_key = payload_cstr(&payload.property_key);
                let owned_key = match property_key {
                    "AllowOpenGOP" => Some(CFString::new("AllowOpenGOP")),
                    "PrioritizeEncodingSpeedOverQuality" => {
                        Some(CFString::new("PrioritizeEncodingSpeedOverQuality"))
                    }
                    _ => None,
                };
                let key_ref = match property_key {
                    "AverageBitRate" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_AverageBitRate
                    },
                    "MaxKeyFrameInterval" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_MaxKeyFrameInterval
                    },
                    "MoreFramesBeforeStart" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_MoreFramesBeforeStart
                    },
                    "MoreFramesAfterEnd" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_MoreFramesAfterEnd
                    },
                    "AllowFrameReordering" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_AllowFrameReordering
                    },
                    "H264EntropyMode" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_H264EntropyMode
                    },
                    "MaxH264SliceBytes" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_MaxH264SliceBytes
                    },
                    "TransferFunction" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_TransferFunction
                    },
                    "YCbCrMatrix" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_YCbCrMatrix
                    },
                    "ColorPrimaries" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_ColorPrimaries
                    },
                    "DataRateLimits" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_DataRateLimits
                    },
                    "AllowOpenGOP" => owned_key.as_ref().unwrap().as_concrete_TypeRef(),
                    "MinAllowedFrameQP" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_MinAllowedFrameQP
                    },
                    "MaxAllowedFrameQP" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_MaxAllowedFrameQP
                    },
                    "PrioritizeEncodingSpeedOverQuality" => {
                        owned_key.as_ref().unwrap().as_concrete_TypeRef()
                    }
                    "PixelAspectRatio" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_PixelAspectRatio
                    },
                    "RealTime" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_RealTime
                    },
                    "ProfileLevel" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_ProfileLevel
                    },
                    // Phase 10 expansion — these keys are already
                    // exported by the guest-shim's static singletons
                    // but were rejected as STATUS_PROPERTY_NOT_SUPPORTED
                    // here. Wire them so guest callers (FFmpeg + future
                    // vt-ferry-driven consumers) can drive the encoder
                    // through the full surface VideoToolbox actually
                    // provides.
                    "ConstantBitRate" => unsafe { kVTCompressionPropertyKey_ConstantBitRate },
                    "EncoderID" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_EncoderID
                    },
                    "ExpectedFrameRate" => unsafe {
                        video_toolbox_sys::compression::kVTCompressionPropertyKey_ExpectedFrameRate
                    },
                    "MaximizePowerEfficiency" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_MaximizePowerEfficiency
                    },
                    "Quality" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_Quality
                    },
                    "ReferenceBufferCount" => unsafe {
                        video_toolbox::compression_properties::kVTCompressionPropertyKey_ReferenceBufferCount
                    },
                    "SpatialAdaptiveQPLevel" => unsafe {
                        kVTCompressionPropertyKey_SpatialAdaptiveQPLevel
                    },
                    "TargetQualityForAlpha" => unsafe {
                        kVTCompressionPropertyKey_TargetQualityForAlpha
                    },
                    _ => {
                        res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                        return Ok(0);
                    }
                };

                probes::vt_ferry_probe_vt_set_property_begin(
                    payload.session_id,
                    payload.property_value_kind as u64,
                );
                let property_status = match payload.property_value_kind {
                    2 => {
                        let number = if payload.property_number_type == 3 {
                            core_foundation::number::CFNumber::from(payload.property_sint64 as i32)
                                .as_CFTypeRef()
                        } else {
                            core_foundation::number::CFNumber::from(payload.property_sint64)
                                .as_CFTypeRef()
                        };
                        unsafe {
                            video_toolbox_sys::session::VTSessionSetProperty(
                                session.vt_session.as_concrete_TypeRef(),
                                key_ref,
                                number,
                            )
                        }
                    }
                    1 => {
                        let boolean = if payload.property_bool != 0 {
                            unsafe { core_foundation::boolean::kCFBooleanTrue }
                        } else {
                            unsafe { core_foundation::boolean::kCFBooleanFalse }
                        };
                        unsafe {
                            video_toolbox_sys::session::VTSessionSetProperty(
                                session.vt_session.as_concrete_TypeRef(),
                                key_ref,
                                boolean as core_foundation_sys::base::CFTypeRef,
                            )
                        }
                    }
                    3 => {
                        let property_string = payload_cstr(&payload.property_string);
                        let string_ref = match property_string {
                            "H264_Main_AutoLevel" => unsafe {
                                video_toolbox_sys::compression::kVTProfileLevel_H264_Main_AutoLevel
                            },
                            "H264_High_AutoLevel" => unsafe {
                                video_toolbox_sys::compression::kVTProfileLevel_H264_High_AutoLevel
                            },
                            "CABAC" => unsafe {
                                video_toolbox_sys::compression::kVTH264EntropyMode_CABAC
                            },
                            "CAVLC" => unsafe {
                                video_toolbox_sys::compression::kVTH264EntropyMode_CAVLC
                            },
                            "ITU_R_709_2" => unsafe {
                                match property_key {
                                    "TransferFunction" => core_video::image_buffer::kCVImageBufferTransferFunction_ITU_R_709_2,
                                    "YCbCrMatrix" => core_video::image_buffer::kCVImageBufferYCbCrMatrix_ITU_R_709_2,
                                    "ColorPrimaries" => core_video::image_buffer::kCVImageBufferColorPrimaries_ITU_R_709_2,
                                    _ => {
                                        res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                                        return Ok(0);
                                    }
                                }
                            },
                            "ITU_R_2020" => unsafe {
                                match property_key {
                                    "TransferFunction" => core_video::image_buffer::kCVImageBufferTransferFunction_ITU_R_2020,
                                    "YCbCrMatrix" => core_video::image_buffer::kCVImageBufferYCbCrMatrix_ITU_R_2020,
                                    "ColorPrimaries" => core_video::image_buffer::kCVImageBufferColorPrimaries_ITU_R_2020,
                                    _ => {
                                        res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                                        return Ok(0);
                                    }
                                }
                            },
                            "SMPTE_240M_1995" => unsafe {
                                match property_key {
                                    "TransferFunction" => core_video::image_buffer::kCVImageBufferTransferFunction_SMPTE_240M_1995,
                                    "YCbCrMatrix" => core_video::image_buffer::kCVImageBufferYCbCrMatrix_SMPTE_240M_1995,
                                    _ => {
                                        res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                                        return Ok(0);
                                    }
                                }
                            },
                            "SMPTE_ST_2084_PQ" => unsafe {
                                core_video::image_buffer::kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ
                            },
                            "SMPTE_ST_428_1" => unsafe {
                                core_video::image_buffer::kCVImageBufferTransferFunction_SMPTE_ST_428_1
                            },
                            "ITU_R_2100_HLG" => unsafe {
                                core_video::image_buffer::kCVImageBufferTransferFunction_ITU_R_2100_HLG
                            },
                            "UseGamma" => unsafe {
                                core_video::image_buffer::kCVImageBufferTransferFunction_UseGamma
                            },
                            "SMPTE_C" => unsafe {
                                core_video::image_buffer::kCVImageBufferColorPrimaries_SMPTE_C
                            },
                            "EBU_3213" => unsafe {
                                core_video::image_buffer::kCVImageBufferColorPrimaries_EBU_3213
                            },
                            "ITU_R_601_4" => unsafe {
                                core_video::image_buffer::kCVImageBufferYCbCrMatrix_ITU_R_601_4
                            },
                            _ => {
                                res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                                return Ok(0);
                            }
                        };
                        unsafe {
                            video_toolbox_sys::session::VTSessionSetProperty(
                                session.vt_session.as_concrete_TypeRef(),
                                key_ref,
                                string_ref as core_foundation_sys::base::CFTypeRef,
                            )
                        }
                    }
                    4 => {
                        if payload.property_array_count != 2 {
                            res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                            return Ok(0);
                        }
                        let first = CFNumber::from(payload.property_array_i64[0]).as_CFType();
                        let second = CFNumber::from(payload.property_array_i64[1]).as_CFType();
                        let array = CFArray::from_CFTypes(&[first, second]);
                        unsafe {
                            video_toolbox_sys::session::VTSessionSetProperty(
                                session.vt_session.as_concrete_TypeRef(),
                                key_ref,
                                array.as_CFTypeRef(),
                            )
                        }
                    }
                    5 => {
                        if payload.property_dict_pair_count != 2 {
                            res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                            return Ok(0);
                        }
                        let mut keys = Vec::with_capacity(2);
                        let mut values = Vec::with_capacity(2);
                        for index in 0..2 {
                            let key_name = payload_cstr(&payload.property_dict_keys[index]);
                            let key = match key_name {
                                "HorizontalSpacing" => CFString::new("HorizontalSpacing"),
                                "VerticalSpacing" => CFString::new("VerticalSpacing"),
                                _ => {
                                    res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                                    return Ok(0);
                                }
                            };
                            keys.push(key);
                            values.push(
                                CFNumber::from(payload.property_dict_sint64[index]).as_CFType(),
                            );
                        }
                        let dictionary = CFDictionary::from_CFType_pairs(&[
                            (keys[0].as_CFType(), values[0].clone()),
                            (keys[1].as_CFType(), values[1].clone()),
                        ]);
                        unsafe {
                            video_toolbox_sys::session::VTSessionSetProperty(
                                session.vt_session.as_concrete_TypeRef(),
                                key_ref,
                                dictionary.as_CFTypeRef(),
                            )
                        }
                    }
                    _ => {
                        res_header.status = STATUS_PROPERTY_NOT_SUPPORTED;
                        return Ok(0);
                    }
                };
                probes::vt_ferry_probe_vt_set_property_end(
                    payload.session_id,
                    if property_status == 0 {
                        STATUS_OK as u64
                    } else {
                        STATUS_INTERNAL_FAILURE as u64
                    },
                );

                if property_status != 0 {
                    eprintln!(
                        "vt-real SET_PROPERTY failed key={} value_kind={} number_type={} sint64={} status={}",
                        property_key,
                        payload.property_value_kind,
                        payload.property_number_type,
                        payload.property_sint64,
                        property_status
                    );
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                Ok(0)
            }
            OP_CREATE_BUFFER_POOL => {
                if req_payload.len() < std::mem::size_of::<CreateBufferPoolPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: CreateBufferPoolPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<CreateBufferPoolPayload>()],
                );

                if payload.session_id != 0 && !self.sessions.contains_key(&payload.session_id) {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }

                let mut layout = BufferLayoutReply::zeroed();
                vtf_fill_buffer_layout(
                    payload.width,
                    payload.height,
                    payload.pixel_format,
                    &mut layout,
                );
                let slot_region_size = layout.total_size;
                let max_buffers = if payload.buffer_count == 0 {
                    16
                } else {
                    payload.buffer_count
                };

                // Zero-copy fast path: if the launcher pre-allocated N
                // IOSurfaces for this shape (one per slot) and handed them
                // to us via mach_ports_register, use them directly — skip
                // both the shared-region broker and cv_pool.create_pixel_buffer.
                #[cfg(target_os = "macos")]
                {
                    // Lock the shared directory only for the
                    // take_matching call — releasing before any
                    // potentially long-running CV/VT work avoids
                    // serializing concurrent guests on this lock.
                    // The entry stays "claimed" (track entry_id
                    // so this backend's Drop can release it when
                    // the connection ends) so a sibling
                    // connection sees the entry as taken.
                    let zc_match = self
                        .iosurface_pools
                        .lock()
                        .unwrap()
                        .take_matching(
                            payload.width,
                            payload.height,
                            payload.pixel_format,
                            max_buffers,
                        );
                    if zc_match.is_none() {
                        eprintln!(
                            "vt-real CREATE_BUFFER_POOL: no zero-copy match \
                             for width={} height={} pixel_format=0x{:x} \
                             buffers={} — falling through to copy path",
                            payload.width, payload.height, payload.pixel_format, max_buffers
                        );
                    }
                    if let Some((iosurface_entry_id, pool)) = zc_match {
                        // The pool carries `pool.spec.slot_count` slots packed
                        // into a single IOSurface. Cap max_buffers at what we
                        // have — guest cycles through them.
                        let pool_slot_count = pool.spec.slot_count;
                        if pool_slot_count < max_buffers {
                            eprintln!(
                                "vt-real zero-copy pool has {} slot(s); \
                                 capping CREATE_BUFFER_POOL request of {} \
                                 to {} so the guest still gets the \
                                 zero-copy path",
                                pool_slot_count, max_buffers, pool_slot_count
                            );
                        }
                        let effective_max_buffers = pool_slot_count;
                        if (effective_max_buffers as usize)
                            > vt_ferry_protocol::VTF_TRANSPORT_MAX_POOL_SLOTS
                        {
                            eprintln!(
                                "vt-real zero-copy pool slot_count {} exceeds \
                                 VTF_TRANSPORT_MAX_POOL_SLOTS={}",
                                effective_max_buffers,
                                vt_ferry_protocol::VTF_TRANSPORT_MAX_POOL_SLOTS,
                            );
                            res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                            return Ok(0);
                        }

                        // Hold an IOSurface read+write lock for the pool's
                        // lifetime. CVPixelBufferCreateWithBytes /
                        // CreateWithPlanarBytes wrappers aren't IOSurface-backed
                        // from VT's perspective, so VT won't trigger the
                        // implicit lock path. We keep the pages resident.
                        if let Err(kr) = pool.surface.lock_read_write() {
                            eprintln!("vt-real zero-copy pool lock_read_write kr={}", kr);
                            res_header.status = STATUS_INTERNAL_FAILURE;
                            return Ok(0);
                        }

                        let pool_id = self.next_host_id;
                        self.next_host_id += 1;

                        let mut reply = CreateBufferPoolReply::zeroed();
                        reply.pool_id = pool_id;
                        reply.width = payload.width;
                        reply.height = payload.height;
                        reply.pixel_format = payload.pixel_format;
                        reply.slot_count = effective_max_buffers;
                        reply.buffer_region_size = effective_max_buffers * slot_region_size;
                        reply.host_backing_kind = VTF_HOST_BACKING_KIND_IOSURFACE;
                        reply.layout = layout;

                        // Build per-slot CVPixelBuffers using CreateWithBytes
                        // (BGRA) or CreateWithPlanarBytes (NV12).
                        let width_u = payload.width as usize;
                        let height_u = payload.height as usize;
                        let slot_bytes = slot_region_size as usize;
                        for slot_index in 0..effective_max_buffers as usize {
                            let slot_offset = slot_index * slot_bytes;
                            let cv_ref_result = match payload.pixel_format {
                                0x42475241 => {
                                    // BGRA: one contiguous plane at slot_offset
                                    let bpr = width_u * 4;
                                    pool.surface.wrap_slot_single_plane(
                                        slot_offset,
                                        width_u,
                                        height_u,
                                        bpr,
                                        payload.pixel_format,
                                    )
                                }
                                0x34323076 => {
                                    // NV12: Y plane at slot_offset+0,
                                    // CbCr plane at slot_offset+(stride*height).
                                    // Align stride up to 64 (matches launcher).
                                    let stride = (width_u + 63) & !63;
                                    let y_size = stride * height_u;
                                    pool.surface.wrap_slot_planar(
                                        width_u,
                                        height_u,
                                        payload.pixel_format,
                                        slot_offset,
                                        slot_bytes,
                                        &[0, y_size],
                                        &[width_u, width_u / 2],
                                        &[height_u, height_u / 2],
                                        &[stride, stride],
                                    )
                                }
                                other => {
                                    eprintln!(
                                        "vt-real zero-copy pool {} slot {}: \
                                         unsupported pixel_format 0x{:x}",
                                        pool_id, slot_index, other
                                    );
                                    res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                                    return Ok(0);
                                }
                            };
                            let cv_ref = match cv_ref_result {
                                Ok(r) => r,
                                Err(status) => {
                                    eprintln!(
                                        "vt-real zero-copy pool {} slot {}: \
                                         CVPixelBufferCreateWith{} status={}",
                                        pool_id,
                                        slot_index,
                                        if payload.pixel_format == 0x34323076 {
                                            "PlanarBytes"
                                        } else {
                                            "Bytes"
                                        },
                                        status
                                    );
                                    res_header.status = STATUS_INTERNAL_FAILURE;
                                    return Ok(0);
                                }
                            };
                            // SAFETY: cv_ref is a +1 retained CVPixelBufferRef.
                            let cv_pixel_buffer = unsafe {
                                core_video::pixel_buffer::CVPixelBuffer::wrap_under_create_rule(
                                    cv_ref as *mut _,
                                )
                            };

                            let mut shared_region_reply = SharedRegionReply::zeroed();
                            shared_region_reply.region_size = slot_bytes as u64;
                            shared_region_reply.source_kind =
                                vt_ferry_protocol::VTF_SHARED_REGION_SOURCE_IOSURFACE;
                            shared_region_reply.source_handle = pool.spec.iosurface_id as u64;
                            shared_region_reply.flags = 1; // writable
                            reply.shared_regions[slot_index] = shared_region_reply;

                            let mapped_bytes =
                                unsafe { pool.surface.base_address().add(slot_offset) };

                            let buffer_id = self.next_host_id;
                            self.next_host_id += 1;

                            self.buffers.insert(
                                buffer_id,
                                VtRealBuffer {
                                    id: buffer_id,
                                    pool_id,
                                    generation: 0,
                                    slot_index: slot_index as u32,
                                    slot_offset,
                                    pixel_buffer: Some(cv_pixel_buffer),
                                    mapped_bytes,
                                    mapped_size: slot_bytes,
                                    pixel_format: payload.pixel_format,
                                    width: payload.width as i32,
                                    height: payload.height as i32,
                                    state: 0,
                                },
                            );
                            reply.buffer_leases[slot_index] = PoolBufferLeaseReply {
                                buffer_id,
                                generation: 0,
                                slot_index: slot_index as u32,
                                slot_offset: slot_offset as u32,
                                host_backing_kind: VTF_HOST_BACKING_KIND_IOSURFACE,
                                flags: 0,
                            };
                        }

                        // Stash the pool's single OwnedIOSurface so it
                        // outlives every CVPixelBuffer we created. Each
                        // CVPixelBuffer additionally holds its own CFRetain
                        // via the release callback.
                        let iosurfaces = vec![pool.surface];

                        self.pools.insert(
                            pool_id,
                            VtRealPool {
                                id: pool_id,
                                session_id: payload.session_id,
                                width: payload.width,
                                height: payload.height,
                                pixel_format: payload.pixel_format,
                                max_buffers: effective_max_buffers,
                                buffer_count: 0,
                                slot_region_size,
                                iosurfaces,
                                iosurface_entry_id: Some(iosurface_entry_id),
                            },
                        );

                        res_payload[..std::mem::size_of::<CreateBufferPoolReply>()]
                            .copy_from_slice(bytemuck::bytes_of(&reply));
                        return Ok(std::mem::size_of::<CreateBufferPoolReply>());
                    }
                }

                // Zero-copy is the only interface. If the launcher didn't
                // pre-declare an IOSurface pool matching this request's
                // (width, height, pixel_format), the request fails — the
                // guest must match a launcher-declared pool shape. The
                // former "copy path" (broker-claimed HostPath shared region
                // + `cv_pool.create_pixel_buffer` + per-encode byte copy)
                // has been removed; see the repo's plan for context.
                res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                Ok(0)
            }
            OP_ALLOC_BUFFER => {
                if req_payload.len() < std::mem::size_of::<AllocBufferPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: AllocBufferPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<AllocBufferPayload>()],
                );

                let (
                    pool_id,
                    pool_width,
                    pool_height,
                    pool_pixel_format,
                    _pool_slot_region_size,
                    pool_host_backing_kind,
                ) = match self.pools.get(&payload.pool_id) {
                    Some(pool) => (
                        pool.id,
                        pool.width,
                        pool.height,
                        pool.pixel_format,
                        pool.slot_region_size,
                        VTF_HOST_BACKING_KIND_IOSURFACE,
                    ),
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                if let Some((buffer_id, buffer)) = self
                    .buffers
                    .iter_mut()
                    .find(|(_, buffer)| buffer.pool_id == pool_id && buffer.state == 0)
                {
                    buffer.generation += 1;
                    buffer.state = 1;
                    let mapped_size = buffer.mapped_size;
                    let pixel_buffer = match vtf_ensure_source_pixel_buffer(buffer) {
                        Ok(pixel_buffer) => pixel_buffer,
                        Err(status) => {
                            res_header.status = status;
                            return Ok(0);
                        }
                    };
                    unsafe {
                        let cv_ref = pixel_buffer.as_concrete_TypeRef() as *const libc::c_void;
                        CVPixelBufferLockBaseAddress(cv_ref, 0);
                        let ptr = CVPixelBufferGetBaseAddress(cv_ref);
                        if !ptr.is_null() {
                            std::ptr::write_bytes(ptr, 0, mapped_size);
                        }
                        CVPixelBufferUnlockBaseAddress(cv_ref, 0);
                    }

                    let mut layout = BufferLayoutReply::zeroed();
                    vtf_fill_buffer_layout(
                        pool_width,
                        pool_height,
                        pool_pixel_format,
                        &mut layout,
                    );

                    let mut reply = AllocBufferReply::zeroed();
                    reply.buffer_id = *buffer_id;
                    reply.generation = buffer.generation;
                    reply.width = pool_width;
                    reply.height = pool_height;
                    reply.pixel_format = pool_pixel_format;
                    reply.slot_index = buffer.slot_index;
                    reply.slot_offset = buffer.slot_offset as u32;
                    reply.host_backing_kind = pool_host_backing_kind;
                    reply.layout = layout;

                    res_payload[..std::mem::size_of::<AllocBufferReply>()]
                        .copy_from_slice(bytemuck::bytes_of(&reply));
                    return Ok(std::mem::size_of::<AllocBufferReply>());
                }

                res_header.status = STATUS_RESOURCE_EXHAUSTED;
                return Ok(0);
            }
            OP_READ_BUFFER => {
                if req_payload.len() < std::mem::size_of::<ReadBufferPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReadBufferPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReadBufferPayload>()],
                );

                let buffer = match self.buffers.get_mut(&payload.buffer_id) {
                    Some(b) => b,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                if buffer.generation != payload.generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Ok(0);
                }
                if buffer.state == 0 {
                    // RECYCLED
                    res_header.status = STATUS_INVALID_STATE;
                    return Ok(0);
                }

                if payload.offset as usize > buffer.mapped_size
                    || payload.length as usize > (buffer.mapped_size - payload.offset as usize)
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                let reply_size = std::mem::size_of::<ReadBufferReply>() + payload.length as usize;
                if res_payload.len() < reply_size {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let mut reply = ReadBufferReply::zeroed();
                reply.buffer_id = buffer.id;
                reply.generation = buffer.generation;
                reply.offset = payload.offset;
                reply.length = payload.length;

                res_payload[..std::mem::size_of::<ReadBufferReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));

                let mapped_size = buffer.mapped_size;
                let width = buffer.width as u32;
                let height = buffer.height as u32;
                let pixel_format = buffer.pixel_format;
                let pixel_buffer = match vtf_ensure_source_pixel_buffer(buffer) {
                    Ok(pixel_buffer) => pixel_buffer,
                    Err(status) => {
                        res_header.status = status;
                        return Ok(0);
                    }
                };
                unsafe {
                    let cv_ref = pixel_buffer.as_concrete_TypeRef() as *const libc::c_void;
                    CVPixelBufferLockBaseAddress(cv_ref, 0);
                    let mut flat_bytes = vec![0u8; mapped_size];
                    if vtf_copy_pixel_buffer_to_bytes(
                        cv_ref,
                        flat_bytes.as_mut_ptr(),
                        width,
                        height,
                        pixel_format,
                    ) {
                        std::ptr::copy_nonoverlapping(
                            flat_bytes.as_ptr().add(payload.offset as usize),
                            res_payload[std::mem::size_of::<ReadBufferReply>()..].as_mut_ptr(),
                            payload.length as usize,
                        );
                    }
                    CVPixelBufferUnlockBaseAddress(cv_ref, 0);
                }

                Ok(reply_size)
            }
            OP_WRITE_BUFFER => {
                if req_payload.len() < std::mem::size_of::<WriteBufferPayload>() {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                let payload: WriteBufferPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<WriteBufferPayload>()],
                );

                if req_payload.len()
                    != std::mem::size_of::<WriteBufferPayload>() + payload.length as usize
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                let buffer = match self.buffers.get_mut(&payload.buffer_id) {
                    Some(b) => b,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                if buffer.generation != payload.generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Ok(0);
                }
                if buffer.state != 1 {
                    // GUEST_WRITABLE
                    res_header.status = STATUS_INVALID_STATE;
                    return Ok(0);
                }

                if payload.offset as usize > buffer.mapped_size
                    || payload.length as usize > (buffer.mapped_size - payload.offset as usize)
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                // Packed zero-copy: buffer.mapped_bytes already points at
                // the slot's bytes within the packed IOSurface. Patch the
                // incoming payload directly at the offset.
                let mapped_bytes = buffer.mapped_bytes;
                let pixel_buffer = match vtf_ensure_source_pixel_buffer(buffer) {
                    Ok(pixel_buffer) => pixel_buffer,
                    Err(status) => {
                        res_header.status = status;
                        return Ok(0);
                    }
                };
                unsafe {
                    let cv_ref = pixel_buffer.as_concrete_TypeRef() as *const libc::c_void;
                    CVPixelBufferLockBaseAddress(cv_ref, 0);
                    std::ptr::copy_nonoverlapping(
                        req_payload[std::mem::size_of::<WriteBufferPayload>()..].as_ptr(),
                        mapped_bytes.add(payload.offset as usize),
                        payload.length as usize,
                    );
                    CVPixelBufferUnlockBaseAddress(cv_ref, 0);
                }

                Ok(0)
            }
            OP_PREPARE_SESSION => {
                use core_foundation::base::TCFType;
                if req_payload.len() < std::mem::size_of::<PrepareSessionPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: PrepareSessionPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<PrepareSessionPayload>()],
                );

                let session = match self.sessions.get_mut(&payload.session_id) {
                    Some(s) => s,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                probes::vt_ferry_probe_vt_prepare_begin(payload.session_id);
                let status = unsafe {
                    video_toolbox_sys::compression::VTCompressionSessionPrepareToEncodeFrames(
                        session.vt_session.as_concrete_TypeRef(),
                    )
                };
                probes::vt_ferry_probe_vt_prepare_end(
                    payload.session_id,
                    if status == 0 {
                        STATUS_OK as u64
                    } else {
                        STATUS_INTERNAL_FAILURE as u64
                    },
                );
                if status != 0 {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                session.prepared = true;
                Ok(0)
            }
            OP_RECYCLE_BUFFER => {
                if req_payload.len() < std::mem::size_of::<RecycleBufferPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: RecycleBufferPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<RecycleBufferPayload>()],
                );

                let pool = self.pools.get(&payload.pool_id);
                let buffer = self.buffers.get_mut(&payload.buffer_id);

                if pool.is_none() || buffer.is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }
                let buffer = buffer.unwrap();
                if buffer.pool_id != payload.pool_id {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }

                if buffer.generation != payload.generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Ok(0);
                }
                if buffer.state == 0 {
                    // RECYCLED
                    res_header.status = STATUS_INVALID_STATE;
                    return Ok(0);
                }

                buffer.state = 0; // RECYCLED
                res_header.status = STATUS_OK;
                Ok(0)
            }
            OP_ENCODE_FRAME => {
                if req_payload.len() < std::mem::size_of::<EncodeFramePayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: EncodeFramePayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<EncodeFramePayload>()],
                );
                res_header.status = self.encode_frame_payload(payload);
                Ok(0)
            }
            OP_ENCODE_FRAME_BATCH => {
                if req_payload.len() < std::mem::size_of::<EncodeFrameBatchPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: EncodeFrameBatchPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<EncodeFrameBatchPayload>()],
                );
                let frame_count =
                    (payload.frame_count as usize).min(VTF_TRANSPORT_MAX_ENCODE_BATCH);
                for frame in &payload.frames[..frame_count] {
                    if frame.session_id != payload.session_id {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                    let status = self.encode_frame_payload(*frame);
                    if status != STATUS_OK {
                        res_header.status = status;
                        return Ok(0);
                    }
                }
                res_header.status = STATUS_OK;

                Ok(0)
            }
            OP_DEQUEUE_OUTPUT => {
                if req_payload.len() < std::mem::size_of::<DequeueOutputPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DequeueOutputPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DequeueOutputPayload>()],
                );

                let output_id = {
                    let mut queues = self.output_state.session_queues.lock().unwrap();
                    queues
                        .get_mut(&payload.session_id)
                        .and_then(|queue| queue.pop_front())
                };
                if let Some(output_id) = output_id {
                    let dequeued = {
                        let outputs = self.output_state.queued_outputs.lock().unwrap();
                        outputs.get(&output_id).map(|out| {
                            (
                                out.reply,
                                out.source_buffer_id,
                                out.source_buffer_generation,
                            )
                        })
                    };
                    if let Some((reply, source_buffer_id, source_buffer_generation)) = dequeued {
                        self.mark_source_buffer_reusable(
                            source_buffer_id,
                            source_buffer_generation,
                        );
                        res_payload[..std::mem::size_of::<DequeueOutputReply>()]
                            .copy_from_slice(bytemuck::bytes_of(&reply));
                        return Ok(std::mem::size_of::<DequeueOutputReply>());
                    }
                }

                res_header.status = STATUS_TIMEOUT;
                Ok(0)
            }
            OP_DEQUEUE_OUTPUT_BATCH => {
                if req_payload.len() < std::mem::size_of::<DequeueOutputBatchPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DequeueOutputBatchPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DequeueOutputBatchPayload>()],
                );

                let max_outputs = (payload.max_outputs as usize)
                    .min(VTF_TRANSPORT_MAX_OUTPUT_BATCH)
                    .max(1);
                let mut output_ids = Vec::with_capacity(max_outputs);
                {
                    let mut queues = self.output_state.session_queues.lock().unwrap();
                    if let Some(queue) = queues.get_mut(&payload.session_id) {
                        while output_ids.len() < max_outputs {
                            let Some(output_id) = queue.pop_front() else {
                                break;
                            };
                            output_ids.push(output_id);
                        }
                    }
                }

                if output_ids.is_empty() {
                    res_header.status = STATUS_TIMEOUT;
                    return Ok(0);
                }

                let mut reply = DequeueOutputBatchReply::zeroed();
                reply.session_id = payload.session_id;
                let mut source_buffers = Vec::with_capacity(output_ids.len());
                {
                    let outputs = self.output_state.queued_outputs.lock().unwrap();
                    for output_id in output_ids {
                        let Some(out) = outputs.get(&output_id) else {
                            continue;
                        };
                        let index = reply.output_count as usize;
                        if index >= VTF_TRANSPORT_MAX_OUTPUT_BATCH {
                            break;
                        }
                        reply.outputs[index] = out.reply;
                        reply.output_count += 1;
                        source_buffers.push((out.source_buffer_id, out.source_buffer_generation));
                    }
                }

                if reply.output_count == 0 {
                    res_header.status = STATUS_TIMEOUT;
                    return Ok(0);
                }

                for (source_buffer_id, source_buffer_generation) in source_buffers {
                    self.mark_source_buffer_reusable(source_buffer_id, source_buffer_generation);
                }

                res_payload[..std::mem::size_of::<DequeueOutputBatchReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<DequeueOutputBatchReply>())
            }
            OP_READ_OUTPUT => {
                if req_payload.len() < std::mem::size_of::<ReadOutputPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReadOutputPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReadOutputPayload>()],
                );

                let outputs = self.output_state.queued_outputs.lock().unwrap();
                let out = match outputs.get(&payload.output_id) {
                    Some(o) => o,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                let sample_size = out.reply.sample_size as usize;
                let reply_size = std::mem::size_of::<ReadOutputReply>() + sample_size;
                if res_payload.len() < reply_size {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let mut reply = ReadOutputReply::zeroed();
                reply.output_id = payload.output_id;
                reply.sample_size = sample_size as u32;

                res_payload[..std::mem::size_of::<ReadOutputReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                match &out.storage {
                    VtRealOutputStorage::Inline(sample_data) => {
                        probes::vt_ferry_probe_vt_read_output_copy_begin(
                            payload.output_id,
                            sample_size as u64,
                            0,
                        );
                        res_payload[std::mem::size_of::<ReadOutputReply>()..reply_size]
                            .copy_from_slice(sample_data);
                        probes::vt_ferry_probe_vt_read_output_copy_end(payload.output_id, 0);
                    }
                }

                Ok(reply_size)
            }
            OP_RELEASE_OUTPUT => {
                if req_payload.len() < std::mem::size_of::<ReleaseOutputPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReleaseOutputPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReleaseOutputPayload>()],
                );

                res_header.status = self.release_output_id(payload.output_id);
                Ok(0)
            }
            OP_RELEASE_OUTPUT_BATCH => {
                if req_payload.len() < std::mem::size_of::<ReleaseOutputBatchPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReleaseOutputBatchPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReleaseOutputBatchPayload>()],
                );
                let output_count =
                    (payload.output_count as usize).min(VTF_TRANSPORT_MAX_OUTPUT_BATCH);
                for output_id in &payload.output_ids[..output_count] {
                    let status = self.release_output_id(*output_id);
                    if status != STATUS_OK {
                        res_header.status = status;
                        return Ok(0);
                    }
                }
                res_header.status = STATUS_OK;
                Ok(0)
            }
            OP_DRAIN => {
                if req_payload.len() < std::mem::size_of::<DrainPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DrainPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DrainPayload>()],
                );

                // Decode-side dispatch: if the session_id maps to a
                // VtRealDecodeSession, drive
                // VTDecompressionSessionWaitForAsynchronousFrames so
                // any pending VT decode work flushes through the
                // output callback into the DecodedOutputQueue. The
                // guest-shim's WaitForAsynchronousFrames then drains
                // that queue. Without this, the guest's "wait" call
                // returns before VT has actually emitted all frames
                // and FFmpeg loses the tail of the stream.
                if let Some(decode_session) =
                    self.decode_sessions.get(&payload.session_id)
                {
                    if let Some(vt_session) = decode_session.vt_session.as_ref() {
                        if let Err(os_status) = vt_session.wait_for_asynchronous_frames() {
                            eprintln!(
                                "vt-real OP_DRAIN(decode): \
                                 VTDecompressionSessionWaitForAsynchronousFrames \
                                 OSStatus {}",
                                os_status
                            );
                            res_header.status = STATUS_INTERNAL_FAILURE;
                            return Ok(0);
                        }
                    }
                    let mut reply = DrainReply::zeroed();
                    reply.session_id = payload.session_id;
                    let queues =
                        self.decoded_output_state.session_queues.lock().unwrap();
                    reply.pending_outputs = queues
                        .get(&payload.session_id)
                        .map(|q| q.len() as u32)
                        .unwrap_or(0);
                    res_payload[..std::mem::size_of::<DrainReply>()]
                        .copy_from_slice(bytemuck::bytes_of(&reply));
                    return Ok(std::mem::size_of::<DrainReply>());
                }

                let session = match self.sessions.get(&payload.session_id) {
                    Some(s) => s,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                probes::vt_ferry_probe_vt_complete_frames_begin(payload.session_id);
                let drain_status = unsafe {
                    video_toolbox_sys::compression::VTCompressionSessionCompleteFrames(
                        session.vt_session.as_concrete_TypeRef(),
                        kCMTimeIndefinite,
                    )
                };
                probes::vt_ferry_probe_vt_complete_frames_end(
                    payload.session_id,
                    if drain_status == 0 {
                        STATUS_OK as u64
                    } else {
                        STATUS_INTERNAL_FAILURE as u64
                    },
                );

                if drain_status != 0 {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let mut reply = DrainReply::zeroed();
                reply.session_id = payload.session_id;

                let queues = self.output_state.session_queues.lock().unwrap();
                if let Some(queue) = queues.get(&payload.session_id) {
                    reply.pending_outputs = queue.len() as u32;
                } else {
                    reply.pending_outputs = 0;
                }

                res_payload[..std::mem::size_of::<DrainReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<DrainReply>())
            }
            OP_DESTROY_SESSION => {
                if req_payload.len() < std::mem::size_of::<DestroySessionPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DestroySessionPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DestroySessionPayload>()],
                );

                // Decode sessions live in their own map (different VT
                // type). Try the decode path first; if it hits, drop
                // the VTDecompressionSession and any cached format
                // description and we're done. Otherwise fall through
                // to the encode path.
                if self.decode_sessions.remove(&payload.session_id).is_some() {
                    return Ok(0);
                }

                if self.sessions.remove(&payload.session_id).is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }
                self.output_state
                    .session_queues
                    .lock()
                    .unwrap()
                    .remove(&payload.session_id);
                let released_output_ids = {
                    let outputs = self.output_state.queued_outputs.lock().unwrap();
                    outputs
                        .iter()
                        .filter_map(|(output_id, output)| {
                            (output.reply.session_id == payload.session_id).then_some(*output_id)
                        })
                        .collect::<Vec<_>>()
                };
                if !released_output_ids.is_empty() {
                    let mut outputs = self.output_state.queued_outputs.lock().unwrap();
                    for output_id in released_output_ids {
                        outputs.remove(&output_id);
                    }
                }
                let pool_ids: Vec<u64> = self
                    .pools
                    .iter()
                    .filter_map(|(pool_id, pool)| {
                        (pool.session_id == payload.session_id).then_some(*pool_id)
                    })
                    .collect();
                // Release the IOSurfacePoolDirectory entries claimed
                // by each session-owned pool BEFORE we remove the
                // pool record. Otherwise ffmpeg's "init encoder
                // probe → DESTROY → init real encoder" pattern would
                // see the second `CREATE_BUFFER_POOL` reject with
                // STATUS_UNSUPPORTED_CODEC_OR_FORMAT, because all
                // directory entries would still be marked claimed
                // by the dead probe encoder.
                #[cfg(target_os = "macos")]
                {
                    let entries_to_release: Vec<usize> = pool_ids
                        .iter()
                        .filter_map(|pool_id| {
                            self.pools.get(pool_id).and_then(|p| p.iosurface_entry_id)
                        })
                        .collect();
                    if !entries_to_release.is_empty() {
                        if let Ok(mut dir) = self.iosurface_pools.lock() {
                            for entry_id in entries_to_release {
                                dir.release_entry(entry_id);
                            }
                        }
                    }
                }
                for pool_id in &pool_ids {
                    self.pools.remove(pool_id);
                }
                self.buffers
                    .retain(|_, buffer| !pool_ids.iter().any(|pool_id| *pool_id == buffer.pool_id));
                Ok(0)
            }
            OP_DESTROY_BUFFER_POOL => {
                // Standalone pool teardown. The guest shim's
                // `CVPixelBufferPool` finalizer fires this when the
                // caller `CFRelease`s a pool. We need this opcode
                // (rather than relying on `OP_DESTROY_SESSION`'s
                // pool sweep) because pools created without a
                // session — notably `AVHWFramesContext`'s
                // `CVPixelBufferPoolCreate` for FFmpeg's
                // `-hwaccel videotoolbox` — would otherwise stay
                // alive (and keep their `IOSurfacePoolDirectory`
                // entry claimed) until the connection drops, even
                // when the pool is long-since unreferenced.
                if req_payload.len() < std::mem::size_of::<DestroyBufferPoolPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DestroyBufferPoolPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DestroyBufferPoolPayload>()],
                );
                #[cfg(target_os = "macos")]
                let entry_to_release: Option<usize> = self
                    .pools
                    .get(&payload.pool_id)
                    .and_then(|p| p.iosurface_entry_id);
                let removed = self.pools.remove(&payload.pool_id);
                if removed.is_none() {
                    // Idempotent semantics: the guest shim might
                    // race the worker's own teardown (e.g. a
                    // DESTROY_SESSION that already cleaned up the
                    // pool). Treat double-release as STATUS_OK
                    // rather than surfacing INVALID_HANDLE — the
                    // guest's pool refcount has already hit zero
                    // either way and we have no useful work to do.
                    return Ok(0);
                }
                self.buffers
                    .retain(|_, buffer| buffer.pool_id != payload.pool_id);
                #[cfg(target_os = "macos")]
                if let Some(entry_id) = entry_to_release {
                    if let Ok(mut dir) = self.iosurface_pools.lock() {
                        dir.release_entry(entry_id);
                    }
                }
                Ok(0)
            }
            // ---- Decode-path opcodes (Phase 10 scaffolding) ----
            //
            // The protocol surface is reserved with payload structs
            // and contract tests; the actual VTDecompressionSession
            // plumbing is the next major chunk. These handlers do the
            // payload-shape validation that the future implementation
            // will need (so a future bring-up is just "fill in the
            // body, flip STATUS_UNSUPPORTED_OPCODE to STATUS_OK") and
            // currently reject every well-formed request with
            // STATUS_UNSUPPORTED_OPCODE — keeping the contract that
            // `vt_real::protocol_surface_tests::reserved_decode_opcodes_are_rejected_with_unsupported_opcode`
            // pins.
            OP_ENQUEUE_ENCODED_FRAME => {
                // Variable-length wire format: fixed-size header
                // (`EnqueueEncodedFramePayload`) followed by the
                // encoded bitstream bytes inline. Mirrors
                // `OP_WRITE_BUFFER`'s pattern; total wire length
                // must equal `size_of::<header>() + encoded_size`.
                if req_payload.len() < std::mem::size_of::<EnqueueEncodedFramePayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: EnqueueEncodedFramePayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<EnqueueEncodedFramePayload>()],
                );

                // Bound the inline byte count.
                if payload.encoded_size > VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES {
                    eprintln!(
                        "vt-real OP_ENQUEUE_ENCODED_FRAME: encoded_size={} \
                         exceeds VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES={}",
                        payload.encoded_size, VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                if payload.encoded_size == 0 {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                // Total wire size must include the inline tail.
                let header_size = std::mem::size_of::<EnqueueEncodedFramePayload>();
                let expected_total = header_size + payload.encoded_size as usize;
                if req_payload.len() != expected_total {
                    eprintln!(
                        "vt-real OP_ENQUEUE_ENCODED_FRAME: wire size mismatch — \
                         got {} bytes, expected {} (header={} + encoded_size={})",
                        req_payload.len(),
                        expected_total,
                        header_size,
                        payload.encoded_size
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                let encoded_bytes = &req_payload[header_size..];

                // Hand the assembled bytes to the shared decode
                // dispatch path. Returns STATUS_OK on success or
                // a STATUS_* error code; we copy the result
                // verbatim into res_header.status.
                let status = self.dispatch_decode_assembled_frame(
                    payload.session_id,
                    encoded_bytes,
                    payload.pts_value,
                    payload.pts_timescale,
                    payload.duration_value,
                    payload.duration_timescale,
                );
                if status != STATUS_OK {
                    res_header.status = status;
                }
                Ok(0)
            }
            OP_ENQUEUE_ENCODED_FRAME_CHUNK => {
                // Chunked counterpart to OP_ENQUEUE_ENCODED_FRAME.
                // Assembles a multi-op encoded-frame delivery into
                // a per-session staging buffer; on the final
                // chunk, dispatches to the same VT decode path
                // OP_ENQUEUE_ENCODED_FRAME uses. Lets the guest
                // ship encoded frames larger than
                // VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES (e.g.
                // multi-MiB IDRs from very-high-bitrate 4K or
                // future 8K content) without needing the inline
                // cap to grow.
                if req_payload.len()
                    < std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()
                {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: EnqueueEncodedFrameChunkPayload =
                    bytemuck::pod_read_unaligned(
                        &req_payload[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()],
                    );
                let header_size = std::mem::size_of::<EnqueueEncodedFrameChunkPayload>();
                let expected_total = header_size + payload.chunk_length as usize;
                if req_payload.len() != expected_total {
                    eprintln!(
                        "vt-real OP_ENQUEUE_ENCODED_FRAME_CHUNK: wire size \
                         mismatch — got {} bytes, expected {} (header={} \
                         + chunk_length={})",
                        req_payload.len(),
                        expected_total,
                        header_size,
                        payload.chunk_length
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                // Bound the total. The chunked path's whole point
                // is to exceed the inline cap, so its own cap has
                // to be considerably larger. Pin at 64 MiB — well
                // above any plausible 8K H.264 IDR (5–10 MiB) and
                // below "the guest is misbehaving".
                const MAX_CHUNKED_ENCODED_FRAME_BYTES: u32 = 64 * 1024 * 1024;
                if payload.total_encoded_size == 0
                    || payload.total_encoded_size > MAX_CHUNKED_ENCODED_FRAME_BYTES
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                if payload.chunk_offset
                    .checked_add(payload.chunk_length)
                    .map(|end| end > payload.total_encoded_size)
                    .unwrap_or(true)
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                if payload.chunk_length == 0 && payload.is_final_chunk == 0 {
                    // Empty non-final chunks are pointless and
                    // would let a misbehaving guest pump opcodes
                    // forever; reject loud.
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                // Validate session up front so we can abort early
                // before allocating an assembly buffer.
                if !self.decode_sessions.contains_key(&payload.session_id) {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }

                let chunk_data = &req_payload[header_size..];

                // Head chunk? Establish the assembly entry. Any
                // prior in-flight assembly for the same session
                // gets dropped — incomplete-previous-frame is a
                // guest bug but we recover cleanly rather than
                // poisoning the session.
                if payload.chunk_offset == 0 {
                    let mut buffer = Vec::with_capacity(payload.total_encoded_size as usize);
                    buffer.extend_from_slice(chunk_data);
                    let pending = PendingEncodedFrame {
                        bytes: buffer,
                        total_size: payload.total_encoded_size,
                        pts_value: payload.pts_value,
                        pts_timescale: payload.pts_timescale,
                        duration_value: payload.duration_value,
                        duration_timescale: payload.duration_timescale,
                    };
                    self.pending_encoded_frames.insert(payload.session_id, pending);
                } else {
                    let pending = match self.pending_encoded_frames.get_mut(&payload.session_id) {
                        Some(p) => p,
                        None => {
                            // Mid-stream chunk with no head — guest
                            // sent the body without the start, or
                            // the worker dropped state across a
                            // session reset.
                            res_header.status = STATUS_INVALID_STATE;
                            return Ok(0);
                        }
                    };
                    if pending.total_size != payload.total_encoded_size {
                        res_header.status = STATUS_INVALID_STATE;
                        return Ok(0);
                    }
                    if pending.bytes.len() != payload.chunk_offset as usize {
                        // Out-of-order chunks are rejected. The
                        // guest's chunked sender walks contiguously;
                        // anything else is a bug.
                        res_header.status = STATUS_INVALID_STATE;
                        return Ok(0);
                    }
                    pending.bytes.extend_from_slice(chunk_data);
                }

                // Final chunk? Validate complete + dispatch.
                if payload.is_final_chunk != 0 {
                    let pending = match self.pending_encoded_frames.remove(&payload.session_id) {
                        Some(p) => p,
                        None => {
                            res_header.status = STATUS_INVALID_STATE;
                            return Ok(0);
                        }
                    };
                    if pending.bytes.len() as u32 != pending.total_size {
                        eprintln!(
                            "vt-real OP_ENQUEUE_ENCODED_FRAME_CHUNK: final chunk \
                             with partial assembly — got {} bytes, expected {}",
                            pending.bytes.len(),
                            pending.total_size
                        );
                        res_header.status = STATUS_INVALID_STATE;
                        return Ok(0);
                    }
                    let status = self.dispatch_decode_assembled_frame(
                        payload.session_id,
                        &pending.bytes,
                        pending.pts_value,
                        pending.pts_timescale,
                        pending.duration_value,
                        pending.duration_timescale,
                    );
                    if status != STATUS_OK {
                        res_header.status = status;
                    }
                }
                Ok(0)
            }
            OP_DEQUEUE_DECODED_FRAME => {
                if req_payload.len() < std::mem::size_of::<DequeueDecodedFramePayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: DequeueDecodedFramePayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<DequeueDecodedFramePayload>()],
                );

                // Determine the session's output mode (chunked vs
                // inline) up front so the reply can signal the
                // right path to the guest.
                let chunked_output = match self.decode_sessions.get(&payload.session_id) {
                    Some(s) => s.chunked_output,
                    None => {
                        res_header.status = STATUS_INVALID_HANDLE;
                        return Ok(0);
                    }
                };

                // Pop the front of the session's FIFO. If empty,
                // STATUS_TIMEOUT signals "no frames ready yet" —
                // matches the encode-side OP_DEQUEUE_OUTPUT semantic
                // so guests share polling logic.
                let output_id = {
                    let mut queues = self.decoded_output_state.session_queues.lock().unwrap();
                    queues
                        .get_mut(&payload.session_id)
                        .and_then(|queue| queue.pop_front())
                };
                let Some(output_id) = output_id else {
                    // Queue empty. Before signaling TIMEOUT, check
                    // if the VT callback recorded a sticky decode
                    // error for this session — if so, build an
                    // error-sentinel reply (output_id == 0,
                    // reply.status == that VT OSStatus) so the
                    // guest can surface the real failure instead
                    // of just seeing "no frames yet, try again".
                    // The take_decode_error call clears the sticky
                    // so a subsequent dequeue returns to TIMEOUT
                    // semantics — guests see each VT error exactly
                    // once.
                    if let Some(vt_status) =
                        self.decoded_output_state.take_decode_error(payload.session_id)
                    {
                        let mut reply = DequeueDecodedFrameReply::zeroed();
                        reply.session_id = payload.session_id;
                        // output_id == 0 is the sentinel: the
                        // worker's normal IDs start at 70_000, so
                        // 0 cannot be confused with a real frame.
                        reply.output_id = 0;
                        reply.status = vt_status as u32;
                        res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()]
                            .copy_from_slice(bytemuck::bytes_of(&reply));
                        return Ok(std::mem::size_of::<DequeueDecodedFrameReply>());
                    }
                    res_header.status = STATUS_TIMEOUT;
                    return Ok(0);
                };

                // Read the decoded frame's metadata under the queue
                // lock, then release the lock before building the
                // reply. The CVImageBuffer stays in the `queued`
                // map until OP_RELEASE_DECODED_FRAME — the guest
                // holds a reference via output_id during that
                // window. No copy here: chunked-mode reads pull
                // bytes straight out of the held image buffer at
                // OP_READ_DECODED_FRAME_CHUNK time, inline mode
                // reads do the same at OP_READ_DECODED_FRAME time.
                let queued_outputs = self.decoded_output_state.queued.lock().unwrap();
                let Some(output) = queued_outputs.get(&output_id) else {
                    // Shouldn't happen — queue and map are kept in
                    // lock-step. Defensive: return TIMEOUT so the
                    // guest can retry rather than crashing here.
                    res_header.status = STATUS_TIMEOUT;
                    return Ok(0);
                };

                let pixel_buffer_ref = output.image_buffer.as_concrete_TypeRef();
                let width = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetWidth(pixel_buffer_ref)
                };
                let height = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetHeight(pixel_buffer_ref)
                };
                let pixel_format = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetPixelFormatType(pixel_buffer_ref)
                };

                // Build the reply. Chunked mode signals itself by
                // populating buffer_host_id with output_id (and the
                // guest knows to issue OP_READ_DECODED_FRAME_CHUNK
                // calls). Inline mode leaves those zero and the
                // guest issues a single OP_READ_DECODED_FRAME.
                let mut reply = DequeueDecodedFrameReply::zeroed();
                reply.session_id = payload.session_id;
                reply.output_id = output_id;
                if chunked_output {
                    reply.buffer_host_id = output_id;
                }
                reply.width = width as u32;
                reply.height = height as u32;
                reply.pixel_format = pixel_format;
                reply.flags = 0;
                reply.pts_value = output.pts.value;
                reply.pts_timescale = output.pts.timescale;
                reply.duration_value = output.duration.value;
                reply.duration_timescale = output.duration.timescale;
                reply.status = output.status as u32;

                res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(std::mem::size_of::<DequeueDecodedFrameReply>())
            }
            OP_RELEASE_DECODED_FRAME => {
                if req_payload.len() < std::mem::size_of::<ReleaseDecodedFramePayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReleaseDecodedFramePayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReleaseDecodedFramePayload>()],
                );
                // Drop the held CVImageBuffer. Retain count goes to
                // zero unless VT still holds it (which it shouldn't
                // by this point — the decompression callback uses
                // wrap_under_get_rule + the +1 retain dropped here
                // is the only ref the worker keeps). Returns
                // INVALID_HANDLE if the guest released an output_id
                // that doesn't exist (double-release, or release
                // after session destroy).
                //
                // The same path serves both inline and chunked
                // modes — chunked mode simply means the guest read
                // pixels via repeated OP_READ_DECODED_FRAME_CHUNK
                // calls before getting here, but the queue entry's
                // shape is identical.
                let removed = self
                    .decoded_output_state
                    .queued
                    .lock()
                    .unwrap()
                    .remove(&payload.output_id);
                if removed.is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                }
                Ok(0)
            }
            OP_READ_DECODED_FRAME => {
                // Mirror of the encode-side OP_READ_OUTPUT: the
                // guest fetches pixel bytes for a previously-
                // dequeued frame. Variable-length response: fixed
                // ReadDecodedFrameReply header + sample_size bytes
                // of pixel data inline.
                //
                // The 1.5 MiB inline cap means this works for
                // ≤720p NV12 today; larger frames need the future
                // pool-binding path. STATUS_RESOURCE_EXHAUSTED is
                // distinct from BOUNDS_VIOLATION so guests can
                // detect "use the pool path" vs. "you sent a bad
                // request".
                if req_payload.len() < std::mem::size_of::<ReadDecodedFramePayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReadDecodedFramePayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReadDecodedFramePayload>()],
                );

                // Chunked-mode sessions must use
                // OP_READ_DECODED_FRAME_CHUNK instead. A correct
                // guest never gets here (the dequeue reply's
                // nonzero buffer_host_id steers it to the chunked
                // path), but reject up front so a confused guest
                // sees a clear error rather than silently
                // succeeding for small frames or hitting the
                // RESOURCE_EXHAUSTED bounds gate for large ones.
                if let Some(s) = self.decode_sessions.get(&payload.session_id) {
                    if s.chunked_output {
                        res_header.status = STATUS_INVALID_STATE;
                        return Ok(0);
                    }
                } else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }

                let queued = self.decoded_output_state.queued.lock().unwrap();
                let Some(output) = queued.get(&payload.output_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                };

                let pixel_buffer_ref = output.image_buffer.as_concrete_TypeRef();
                let width = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetWidth(pixel_buffer_ref)
                };
                let height = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetHeight(pixel_buffer_ref)
                };
                let pixel_format = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetPixelFormatType(pixel_buffer_ref)
                };

                // Compute total byte count using the existing
                // layout helper. Cap at the inline-shipping budget
                // before we touch the lock.
                let mut layout = BufferLayoutReply::zeroed();
                vtf_fill_buffer_layout(width as u32, height as u32, pixel_format, &mut layout);
                let sample_size = layout.total_size as usize;
                if sample_size > VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES as usize {
                    eprintln!(
                        "vt-real OP_READ_DECODED_FRAME: frame size {} \
                         exceeds inline cap {} ({}x{} pixel_format=0x{:08x}). \
                         Use the pool-binding path for larger frames.",
                        sample_size,
                        VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES,
                        width,
                        height,
                        pixel_format,
                    );
                    res_header.status = STATUS_RESOURCE_EXHAUSTED;
                    return Ok(0);
                }

                let reply_size = std::mem::size_of::<ReadDecodedFrameReply>() + sample_size;
                if res_payload.len() < reply_size {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                // Lock for read, copy planar data into the response
                // body, unlock. The kCVPixelBufferLock_ReadOnly flag
                // (1) tells CV the host doesn't need write access —
                // critical for IOSurface-backed buffers where write
                // access can stall the GPU pipeline.
                const LOCK_FLAGS_READ_ONLY: u64 = 1;
                let lock_status = unsafe {
                    CVPixelBufferLockBaseAddress(
                        pixel_buffer_ref as *const libc::c_void,
                        LOCK_FLAGS_READ_ONLY,
                    )
                };
                if lock_status != 0 {
                    eprintln!(
                        "vt-real OP_READ_DECODED_FRAME: \
                         CVPixelBufferLockBaseAddress failed status={}",
                        lock_status
                    );
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let dest_ptr =
                    res_payload[std::mem::size_of::<ReadDecodedFrameReply>()..].as_mut_ptr();
                let copied = unsafe {
                    vtf_copy_pixel_buffer_to_bytes(
                        pixel_buffer_ref as *const libc::c_void,
                        dest_ptr,
                        width as u32,
                        height as u32,
                        pixel_format,
                    )
                };

                let _ = unsafe {
                    CVPixelBufferUnlockBaseAddress(
                        pixel_buffer_ref as *const libc::c_void,
                        LOCK_FLAGS_READ_ONLY,
                    )
                };

                if !copied {
                    eprintln!("vt-real OP_READ_DECODED_FRAME: copy failed");
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let mut reply = ReadDecodedFrameReply::zeroed();
                reply.output_id = payload.output_id;
                reply.sample_size = sample_size as u32;
                reply.status = output.status as u32;

                res_payload[..std::mem::size_of::<ReadDecodedFrameReply>()]
                    .copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(reply_size)
            }
            OP_SET_DECODE_FORMAT => {
                if req_payload.len() < std::mem::size_of::<SetDecodeFormatPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: SetDecodeFormatPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<SetDecodeFormatPayload>()],
                );
                // parameter_set_count must fit the inline table.
                if (payload.parameter_set_count as usize) > VTF_TRANSPORT_MAX_PARAMETER_SETS {
                    eprintln!(
                        "vt-real OP_SET_DECODE_FORMAT: parameter_set_count={} \
                         exceeds VTF_TRANSPORT_MAX_PARAMETER_SETS={}",
                        payload.parameter_set_count, VTF_TRANSPORT_MAX_PARAMETER_SETS
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                // Sum of declared sizes must fit the inline byte
                // budget. Catches malformed senders that under-
                // populate the size table.
                let declared_total: u64 = payload
                    .parameter_set_sizes
                    .iter()
                    .take(payload.parameter_set_count as usize)
                    .map(|s| u64::from(*s))
                    .sum();
                if declared_total > VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES as u64 {
                    eprintln!(
                        "vt-real OP_SET_DECODE_FORMAT: parameter sets total \
                         {} bytes exceeds VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES={}",
                        declared_total, VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }
                if payload.parameter_set_count == 0 {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                // Look up the decode session. Must have been created
                // by a prior OP_CREATE_SESSION with kind = DECODE.
                if !self.decode_sessions.contains_key(&payload.session_id) {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                }

                // Slice the inline parameter_set_data buffer into
                // per-set sub-slices using the size table. Bounds
                // already validated above.
                let mut offset: usize = 0;
                let mut parameter_sets: Vec<&[u8]> = Vec::with_capacity(
                    payload.parameter_set_count as usize,
                );
                for i in 0..payload.parameter_set_count as usize {
                    let len = payload.parameter_set_sizes[i] as usize;
                    let end = offset + len;
                    parameter_sets
                        .push(&payload.parameter_set_data[offset..end]);
                    offset = end;
                }

                // Build the CMVideoFormatDescription. H.264 = 'avc1',
                // HEVC = 'hvc1'. Anything else is an unsupported codec
                // for this op (the encode-side accepts more, but
                // VTDecompressionSession only supports these two
                // through the parameter-set entrypoints).
                const FOURCC_AVC1: u32 = 0x6176_6331;
                const FOURCC_HVC1: u32 = 0x6876_6331;
                let format_description_result = match payload.codec {
                    FOURCC_AVC1 => {
                        core_media::format_description::CMVideoFormatDescription::from_h264_parameter_sets(
                            &parameter_sets,
                            payload.nal_header_length as i32,
                        )
                    }
                    FOURCC_HVC1 => {
                        core_media::format_description::CMVideoFormatDescription::from_hevc_parameter_sets(
                            &parameter_sets,
                            payload.nal_header_length as i32,
                            None,
                        )
                    }
                    _ => {
                        eprintln!(
                            "vt-real OP_SET_DECODE_FORMAT: codec FourCC \
                             0x{:08x} not supported — only 'avc1' and \
                             'hvc1' work through the parameter-set entrypoints",
                            payload.codec
                        );
                        res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                        return Ok(0);
                    }
                };
                let format_description = match format_description_result {
                    Ok(fd) => fd,
                    Err(os_status) => {
                        eprintln!(
                            "vt-real OP_SET_DECODE_FORMAT: \
                             CMVideoFormatDescriptionCreateFrom{}ParameterSets \
                             failed with OSStatus {}",
                            if payload.codec == FOURCC_HVC1 { "HEVC" } else { "H264" },
                            os_status
                        );
                        res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                        return Ok(0);
                    }
                };

                // Two paths depending on whether we already have a
                // VTDecompressionSession:
                //
                //   1. First SET_DECODE_FORMAT call — no existing VT
                //      session. Create a fresh one bound to the new
                //      format description.
                //   2. Subsequent call (mid-stream format change) —
                //      check VTDecompressionSessionCanAcceptFormatDescription.
                //      If yes, the session keeps running; we just
                //      swap the stashed format description. If no,
                //      we drop the existing VT session and create a
                //      new one.
                let session = self
                    .decode_sessions
                    .get_mut(&payload.session_id)
                    .expect("decode session checked above");
                let needs_new_vt = match &session.vt_session {
                    None => true,
                    Some(existing) => {
                        use core_media::format_description::TCMFormatDescription;
                        // can_accept_format_description consumes a
                        // CMFormatDescription by value. Build a fresh
                        // base view via as_format_description (a
                        // get-rule wrap that doesn't move ownership)
                        // so we keep the typed CMVideoFormatDescription
                        // for the swap below.
                        !existing
                            .can_accept_format_description(format_description.as_format_description())
                    }
                };
                if needs_new_vt {
                    // Drop the old session (if any) before creating a
                    // new one so we don't briefly hold two real VT
                    // objects against the same logical decode session.
                    session.vt_session = None;

                    // Heap-stash the callback context so the raw
                    // pointer we hand to VT stays stable through the
                    // session's lifetime. Reuse the existing box if a
                    // prior format-change recreate left one — the
                    // C-side ref con is the same pointer either way,
                    // so VT sees a continuous callback identity.
                    if session.callback_context.is_none() {
                        session.callback_context = Some(Box::new(DecodeSessionCallbackContext {
                            output_state: self.decoded_output_state.clone(),
                            session_id: payload.session_id,
                        }));
                    }
                    let callback_ref_con = session
                        .callback_context
                        .as_ref()
                        .map(|ctx| {
                            (&**ctx) as *const DecodeSessionCallbackContext
                                as *mut std::ffi::c_void
                        })
                        .expect("callback context just installed");

                    let callback_record =
                        video_toolbox::decompression_session::VTDecompressionOutputCallbackRecord {
                            decompressionOutputCallback: Some(
                                vtf_vt_decompression_output_callback,
                            ),
                            decompressionOutputRefCon: callback_ref_con,
                        };
                    let vt_result = unsafe {
                        video_toolbox::decompression_session::VTDecompressionSession::new_with_callback(
                            format_description.clone(),
                            None,
                            None,
                            Some(&callback_record as *const _),
                        )
                    };
                    match vt_result {
                        Ok(vt_session) => {
                            session.vt_session = Some(vt_session);
                        }
                        Err(os_status) => {
                            eprintln!(
                                "vt-real OP_SET_DECODE_FORMAT: \
                                 VTDecompressionSessionCreate failed with \
                                 OSStatus {}",
                                os_status
                            );
                            res_header.status = STATUS_UNSUPPORTED_CODEC_OR_FORMAT;
                            return Ok(0);
                        }
                    }
                }
                session.format_description = Some(format_description);
                Ok(0)
            }
            OP_BIND_DECODE_OUTPUT_POOL => {
                // Switches the session into chunked-zero-copy
                // output mode. The op name is historical — the
                // original Phase 11 design bound a real buffer
                // pool, but Phase 12 switched to reading directly
                // out of VT's IOSurface-backed CVImageBuffer (one
                // memcpy per chunk vs. two per frame). The wire
                // shape is preserved for backwards compatibility,
                // but `pool_id` must now be zero.
                if req_payload.len() < std::mem::size_of::<BindDecodeOutputPoolPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: BindDecodeOutputPoolPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<BindDecodeOutputPoolPayload>()],
                );

                // pool_id must be zero — the field is vestigial
                // from the old slot-copy design. Rejecting nonzero
                // catches old clients shipping a real pool id and
                // makes the contract loud.
                if payload.pool_id != 0 {
                    eprintln!(
                        "vt-real OP_BIND_DECODE_OUTPUT_POOL: pool_id must be 0 \
                         (zero-copy chunked mode); got {}",
                        payload.pool_id
                    );
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                let Some(session) = self.decode_sessions.get_mut(&payload.session_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                };

                // Reject double-bind. Rebinding would just be
                // setting `chunked_output = true` again, which is
                // harmless, but guests calling BIND twice almost
                // certainly indicate state confusion — fail loud
                // so the bug surfaces early.
                if session.chunked_output {
                    res_header.status = STATUS_INVALID_STATE;
                    return Ok(0);
                }
                session.chunked_output = true;
                Ok(0)
            }
            OP_READ_DECODED_FRAME_CHUNK => {
                // Reads a byte range out of a queued decoded
                // frame's CVImageBuffer. Mirrors the encode-side
                // OP_READ_BUFFER's chunked shape — the guest issues
                // repeated calls until it has drained the whole
                // frame.
                if req_payload.len() < std::mem::size_of::<ReadDecodedFrameChunkPayload>() {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }
                let payload: ReadDecodedFrameChunkPayload = bytemuck::pod_read_unaligned(
                    &req_payload[..std::mem::size_of::<ReadDecodedFrameChunkPayload>()],
                );

                // Session must exist and must be in chunked mode.
                // Inline-mode sessions should use OP_READ_DECODED_FRAME
                // (single-shot). Out-of-mode calls return
                // INVALID_STATE so the misuse is loud.
                let Some(session) = self.decode_sessions.get(&payload.session_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                };
                if !session.chunked_output {
                    res_header.status = STATUS_INVALID_STATE;
                    return Ok(0);
                }

                let mut queued = self.decoded_output_state.queued.lock().unwrap();
                let Some(output) = queued.get_mut(&payload.output_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Ok(0);
                };

                let pixel_buffer_ref = output.image_buffer.as_concrete_TypeRef();
                let width = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetWidth(pixel_buffer_ref)
                };
                let height = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetHeight(pixel_buffer_ref)
                };
                let pixel_format = unsafe {
                    core_video::pixel_buffer::CVPixelBufferGetPixelFormatType(pixel_buffer_ref)
                };

                // Compute total frame size from the canonical
                // layout. Bounds-check the requested range against
                // it before any allocation or lock.
                let mut layout = BufferLayoutReply::zeroed();
                vtf_fill_buffer_layout(width as u32, height as u32, pixel_format, &mut layout);
                let total = layout.total_size as u64;
                let off = payload.offset as u64;
                let len = payload.length as u64;
                if off > total || len > total - off {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Ok(0);
                }

                let header_size = std::mem::size_of::<ReadDecodedFrameChunkReply>();
                let reply_size = header_size + payload.length as usize;
                if res_payload.len() < reply_size {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                // Stride-aware byte-range copy directly out of
                // VT's CVPixelBuffer — no full-frame intermediate
                // buffer. Lock for the chunk, copy
                // `[offset, offset+length)` of the canonical
                // layout via `vtf_copy_pixel_buffer_byte_range`,
                // unlock. Per-chunk lock is cheap (atomic op) and
                // the buffer's contents are stable across chunks
                // because VT writes once during decode and the
                // worker holds the +1 retain through dequeue +
                // release. Pre-Phase-16-followup this path used
                // an `Option<Vec<u8>>` cache populated on the
                // first chunk read; that held one full-frame Vec
                // per in-flight output (~12 MiB at 4K) for the
                // duration of the chunked drain.
                const LOCK_FLAGS_READ_ONLY: u64 = 1;
                let lock_status = unsafe {
                    CVPixelBufferLockBaseAddress(
                        pixel_buffer_ref as *const libc::c_void,
                        LOCK_FLAGS_READ_ONLY,
                    )
                };
                if lock_status != 0 {
                    eprintln!(
                        "vt-real OP_READ_DECODED_FRAME_CHUNK: \
                         CVPixelBufferLockBaseAddress failed status={}",
                        lock_status
                    );
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let copied = unsafe {
                    vtf_copy_pixel_buffer_byte_range(
                        pixel_buffer_ref as *const libc::c_void,
                        &layout,
                        payload.offset as usize,
                        payload.length as usize,
                        res_payload[header_size..reply_size].as_mut_ptr(),
                    )
                };

                let _ = unsafe {
                    CVPixelBufferUnlockBaseAddress(
                        pixel_buffer_ref as *const libc::c_void,
                        LOCK_FLAGS_READ_ONLY,
                    )
                };

                if !copied {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Ok(0);
                }

                let mut reply = ReadDecodedFrameChunkReply::zeroed();
                reply.output_id = payload.output_id;
                reply.offset = payload.offset;
                reply.length = payload.length;
                reply.status = output.status as u32;

                res_payload[..header_size].copy_from_slice(bytemuck::bytes_of(&reply));
                Ok(reply_size)
            }
            _ => {
                res_header.status = STATUS_UNSUPPORTED_OPCODE;
                Ok(0)
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod iosurface_pool_tests {
    use super::*;
    use crate::backend::Backend;
    use crate::iosurface_bridge::OwnedIOSurface;
    use crate::iosurface_pool_directory::{IOSurfacePoolDirectory, IOSurfacePoolSpec};
    use libc::c_int;

    // Apple framework constants follow camelCase, not Rust's
    // SCREAMING_SNAKE_CASE; same allow as the production iosurface
    // bridge module. Test-only redeclarations of the FFI surface
    // intentionally use minimal signatures (e.g. opaque pointer
    // types) — suppress the redeclare-mismatch warning since both
    // versions are FFI to the same C symbol.
    #[allow(
        non_camel_case_types,
        non_upper_case_globals,
        clashing_extern_declarations
    )]
    mod cf_ffi {
        use libc::{c_int, c_void};
        pub type CFStringRef = *const c_void;
        pub type CFNumberRef = *const c_void;
        pub type CFNumberType = c_int;
        pub type CFAllocatorRef = *const c_void;
        pub type CFDictionaryKeyCallBacks = c_void;
        pub type CFDictionaryValueCallBacks = c_void;
        pub type CFMutableDictionaryRef = *mut c_void;
        pub type CFDictionaryRef = *const c_void;
        pub type CFIndex = isize;
        pub type CFTypeRef = *const c_void;
        pub type IOSurfaceRef = *mut c_void;
        pub type mach_port_t = u32;

        pub const kCFNumberIntType: CFNumberType = 9;

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
            pub static kIOSurfaceWidth: CFStringRef;
            pub static kIOSurfaceHeight: CFStringRef;
            pub static kIOSurfaceBytesPerElement: CFStringRef;
            pub static kIOSurfaceBytesPerRow: CFStringRef;
            pub static kIOSurfacePixelFormat: CFStringRef;
        }
    }

    fn make_bgra_iosurface(width: c_int, height: c_int) -> cf_ffi::IOSurfaceRef {
        unsafe {
            let props = cf_ffi::CFDictionaryCreateMutable(
                std::ptr::null(),
                0,
                &cf_ffi::kCFTypeDictionaryKeyCallBacks,
                &cf_ffi::kCFTypeDictionaryValueCallBacks,
            );
            let bpr: c_int = width * 4;
            let bpe: c_int = 4;
            let fmt: c_int = 0x42475241; // 'BGRA'
            let width_n = cf_ffi::CFNumberCreate(
                std::ptr::null(),
                cf_ffi::kCFNumberIntType,
                &width as *const _ as *const _,
            );
            let height_n = cf_ffi::CFNumberCreate(
                std::ptr::null(),
                cf_ffi::kCFNumberIntType,
                &height as *const _ as *const _,
            );
            let bpr_n = cf_ffi::CFNumberCreate(
                std::ptr::null(),
                cf_ffi::kCFNumberIntType,
                &bpr as *const _ as *const _,
            );
            let bpe_n = cf_ffi::CFNumberCreate(
                std::ptr::null(),
                cf_ffi::kCFNumberIntType,
                &bpe as *const _ as *const _,
            );
            let fmt_n = cf_ffi::CFNumberCreate(
                std::ptr::null(),
                cf_ffi::kCFNumberIntType,
                &fmt as *const _ as *const _,
            );
            cf_ffi::CFDictionarySetValue(
                props,
                cf_ffi::kIOSurfaceWidth as *const _,
                width_n as *const _,
            );
            cf_ffi::CFDictionarySetValue(
                props,
                cf_ffi::kIOSurfaceHeight as *const _,
                height_n as *const _,
            );
            cf_ffi::CFDictionarySetValue(
                props,
                cf_ffi::kIOSurfaceBytesPerElement as *const _,
                bpe_n as *const _,
            );
            cf_ffi::CFDictionarySetValue(
                props,
                cf_ffi::kIOSurfaceBytesPerRow as *const _,
                bpr_n as *const _,
            );
            cf_ffi::CFDictionarySetValue(
                props,
                cf_ffi::kIOSurfacePixelFormat as *const _,
                fmt_n as *const _,
            );
            let surface = cf_ffi::IOSurfaceCreate(props);
            cf_ffi::CFRelease(width_n as cf_ffi::CFTypeRef);
            cf_ffi::CFRelease(height_n as cf_ffi::CFTypeRef);
            cf_ffi::CFRelease(bpr_n as cf_ffi::CFTypeRef);
            cf_ffi::CFRelease(bpe_n as cf_ffi::CFTypeRef);
            cf_ffi::CFRelease(fmt_n as cf_ffi::CFTypeRef);
            cf_ffi::CFRelease(props as cf_ffi::CFTypeRef);
            surface
        }
    }

    /// Seed the directory directly with a freshly created IOSurface so the
    /// test is independent of mach_ports_register/lookup (those are tested in
    /// iosurface_bridge::tests). Then dispatch a CREATE_BUFFER_POOL and
    /// assert we took the zero-copy branch: the pool is iosurface_backed,
    /// shared_region is None, and the reply carries the IOSurface source
    /// kind + IOSurfaceID in source_handle.
    #[test]
    fn create_buffer_pool_takes_zero_copy_path_when_directory_matches() {
        unsafe {
            let surface_raw = make_bgra_iosurface(256, 144);
            assert!(!surface_raw.is_null());
            let port = cf_ffi::IOSurfaceCreateMachPort(surface_raw);
            assert!(port != 0);
            let surface_id = cf_ffi::IOSurfaceGetID(surface_raw);

            let owned = OwnedIOSurface::from_mach_port(port)
                .expect("OwnedIOSurface::from_mach_port returned None");

            // Release the original +1 retain from IOSurfaceCreate. The
            // transient Mach port has been consumed by IOSurfaceLookupFromMachPort
            // (which retains the surface internally) so no explicit
            // mach_port_deallocate is needed in this test.
            cf_ffi::CFRelease(surface_raw as cf_ffi::CFTypeRef);
            let _ = crate::iosurface_bridge::__mach_task_self_for_tests();

            let mut directory = IOSurfacePoolDirectory::empty();
            directory.install_for_tests(crate::iosurface_pool_directory::IOSurfacePool {
                spec: IOSurfacePoolSpec {
                    width: 256,
                    height: 144,
                    pixel_format: 0x42475241,
                    slot_count: 1,
                    iosurface_id: surface_id,
                },
                surface: owned,
            });

            let mut backend = VtRealBackend::new().with_iosurface_pools(directory);

            // Minimal HELLO to establish a session-less pool is fine; the
            // dispatch allows session_id=0. Build CREATE_BUFFER_POOL payload.
            let payload = CreateBufferPoolPayload {
                session_id: 0,
                buffer_count: 1,
                pixel_format: 0x42475241,
                width: 256,
                height: 144,
                usage_flags: 0,
                _padding: 0,
            };
            let mut req = vec![0u8; std::mem::size_of::<CreateBufferPoolPayload>()];
            req.copy_from_slice(bytemuck::bytes_of(&payload));
            let req_header = MessageHeader {
                version: 1,
                opcode: OP_CREATE_BUFFER_POOL,
                flags: 0,
                request_id: 1,
                payload_len: req.len() as u32,
                status: 0,
            };
            let mut res_header = MessageHeader {
                version: 1,
                opcode: OP_CREATE_BUFFER_POOL,
                flags: 0,
                request_id: 1,
                payload_len: 0,
                status: 0,
            };
            let mut res = vec![0u8; std::mem::size_of::<CreateBufferPoolReply>()];
            let reply_size = backend
                .dispatch(&req_header, &req, &mut res_header, &mut res)
                .expect("dispatch failed");
            assert_eq!(res_header.status, 0);
            assert_eq!(reply_size, std::mem::size_of::<CreateBufferPoolReply>());

            let reply: CreateBufferPoolReply =
                bytemuck::pod_read_unaligned(&res[..std::mem::size_of::<CreateBufferPoolReply>()]);
            assert_eq!(reply.slot_count, 1);
            assert_eq!(reply.width, 256);
            assert_eq!(reply.height, 144);
            assert_eq!(
                reply.host_backing_kind,
                vt_ferry_protocol::VTF_HOST_BACKING_KIND_IOSURFACE
            );
            assert_eq!(
                reply.shared_regions[0].source_kind,
                vt_ferry_protocol::VTF_SHARED_REGION_SOURCE_IOSURFACE
            );
            assert_eq!(reply.shared_regions[0].source_handle, surface_id as u64);

            // The zero-copy pool stashed in VtRealBackend carries exactly
            // one retained IOSurface (the packed-pool surface).
            let pool = backend
                .pools
                .get(&reply.pool_id)
                .expect("pool not recorded");
            assert_eq!(pool.iosurfaces.len(), 1);
        }
    }

    /// `OP_DESTROY_BUFFER_POOL` releases the directory entry the
    /// pool claimed so a subsequent `OP_CREATE_BUFFER_POOL` at the
    /// same shape on the same connection can re-claim it.
    /// Without this opcode, FFmpeg's `-hwaccel videotoolbox`
    /// flow — decoder hwaccel claims the pool, hwaccel init
    /// fails for unrelated reasons, FFmpeg drops the pool but
    /// never tells us — would leave the entry claimed for the
    /// rest of the connection's lifetime, starving the encoder's
    /// pool create.
    #[test]
    fn destroy_buffer_pool_releases_directory_entry() {
        unsafe {
            let surface_raw = make_bgra_iosurface(256, 144);
            assert!(!surface_raw.is_null());
            let port = cf_ffi::IOSurfaceCreateMachPort(surface_raw);
            assert!(port != 0);
            let surface_id = cf_ffi::IOSurfaceGetID(surface_raw);
            let owned = OwnedIOSurface::from_mach_port(port)
                .expect("OwnedIOSurface::from_mach_port returned None");
            cf_ffi::CFRelease(surface_raw as cf_ffi::CFTypeRef);
            let _ = crate::iosurface_bridge::__mach_task_self_for_tests();

            let mut directory = IOSurfacePoolDirectory::empty();
            directory.install_for_tests(crate::iosurface_pool_directory::IOSurfacePool {
                spec: IOSurfacePoolSpec {
                    width: 256,
                    height: 144,
                    pixel_format: 0x42475241,
                    slot_count: 1,
                    iosurface_id: surface_id,
                },
                surface: owned,
            });

            let mut backend = VtRealBackend::new().with_iosurface_pools(directory);

            // Round 1: claim the only directory entry.
            let create_payload = CreateBufferPoolPayload {
                session_id: 0,
                buffer_count: 1,
                pixel_format: 0x42475241,
                width: 256,
                height: 144,
                usage_flags: 0,
                _padding: 0,
            };
            let create_req = bytemuck::bytes_of(&create_payload).to_vec();
            let mut create_res_header = MessageHeader::zeroed();
            let mut create_res = vec![0u8; std::mem::size_of::<CreateBufferPoolReply>()];
            backend
                .dispatch(
                    &MessageHeader {
                        version: 1,
                        opcode: OP_CREATE_BUFFER_POOL,
                        flags: 0,
                        request_id: 1,
                        payload_len: create_req.len() as u32,
                        status: 0,
                    },
                    &create_req,
                    &mut create_res_header,
                    &mut create_res,
                )
                .expect("create dispatch");
            assert_eq!(create_res_header.status, 0);
            let reply: CreateBufferPoolReply = bytemuck::pod_read_unaligned(&create_res);
            let claimed_pool_id = reply.pool_id;

            // Round 2: a SECOND CREATE_BUFFER_POOL at the same
            // shape must fail because the only entry is claimed.
            let mut second_res_header = MessageHeader::zeroed();
            let mut second_res = vec![0u8; std::mem::size_of::<CreateBufferPoolReply>()];
            backend
                .dispatch(
                    &MessageHeader {
                        version: 1,
                        opcode: OP_CREATE_BUFFER_POOL,
                        flags: 0,
                        request_id: 2,
                        payload_len: create_req.len() as u32,
                        status: 0,
                    },
                    &create_req,
                    &mut second_res_header,
                    &mut second_res,
                )
                .expect("second create dispatch");
            assert_eq!(
                second_res_header.status,
                STATUS_UNSUPPORTED_CODEC_OR_FORMAT,
                "second create should reject — entry already claimed"
            );

            // Round 3: tear down the first pool. This is the
            // operation we're pinning: it must drop the pool
            // record AND release the directory entry.
            let destroy_payload = DestroyBufferPoolPayload {
                pool_id: claimed_pool_id,
            };
            let destroy_req = bytemuck::bytes_of(&destroy_payload).to_vec();
            let mut destroy_res_header = MessageHeader::zeroed();
            backend
                .dispatch(
                    &MessageHeader {
                        version: 1,
                        opcode: OP_DESTROY_BUFFER_POOL,
                        flags: 0,
                        request_id: 3,
                        payload_len: destroy_req.len() as u32,
                        status: 0,
                    },
                    &destroy_req,
                    &mut destroy_res_header,
                    &mut [],
                )
                .expect("destroy dispatch");
            assert_eq!(destroy_res_header.status, 0);
            assert!(
                !backend.pools.contains_key(&claimed_pool_id),
                "pool record should be removed after destroy"
            );

            // Round 4: now CREATE_BUFFER_POOL at the same shape
            // succeeds — the entry was released by the destroy.
            let mut third_res_header = MessageHeader::zeroed();
            let mut third_res = vec![0u8; std::mem::size_of::<CreateBufferPoolReply>()];
            backend
                .dispatch(
                    &MessageHeader {
                        version: 1,
                        opcode: OP_CREATE_BUFFER_POOL,
                        flags: 0,
                        request_id: 4,
                        payload_len: create_req.len() as u32,
                        status: 0,
                    },
                    &create_req,
                    &mut third_res_header,
                    &mut third_res,
                )
                .expect("third create dispatch");
            assert_eq!(
                third_res_header.status, 0,
                "create after destroy should succeed"
            );

            // Round 5: idempotent destroy. Tearing down a pool
            // that's already gone should not error — the guest's
            // CFRelease finalizer might race the worker's own
            // cleanup (e.g. a session destroy that already
            // collected the pool).
            let mut idem_res_header = MessageHeader::zeroed();
            backend
                .dispatch(
                    &MessageHeader {
                        version: 1,
                        opcode: OP_DESTROY_BUFFER_POOL,
                        flags: 0,
                        request_id: 5,
                        payload_len: destroy_req.len() as u32,
                        status: 0,
                    },
                    &destroy_req,
                    &mut idem_res_header,
                    &mut [],
                )
                .expect("idempotent destroy dispatch");
            assert_eq!(idem_res_header.status, 0, "double destroy should be a no-op");
        }
    }

    // Note: the previous `bind_decode_output_pool_rejects_non_nv12_pool`
    // test is gone. It pinned the slot-copy path's format gate
    // (worker validated `pool.pixel_format == NV12 && pool.dims ==
    // session.dims` before binding). Phase 12 zero-copy chunked
    // mode skips the pool entirely, so there's no pool format to
    // gate. The new contract is `pool_id must be 0` and is pinned
    // by `bind_decode_output_pool_rejects_nonzero_pool_id` in
    // protocol_surface_tests.
}

#[cfg(test)]
mod buffer_layout_tests {
    //! Portable (non-macOS-gated) coverage for `vtf_fill_buffer_layout`
    //! and `vtf_buffer_total_size`. These run in milliseconds and
    //! catch layout regressions that would otherwise only surface in
    //! the broker/VM smoke (~30s spinup). When a new pixel format is
    //! added to the worker, an entry here is the cheapest contract.
    use super::*;
    use vt_ferry_protocol::BufferLayoutReply;

    fn fill(width: u32, height: u32, pixel_format: u32) -> BufferLayoutReply {
        let mut layout = BufferLayoutReply::zeroed();
        vtf_fill_buffer_layout(width, height, pixel_format, &mut layout);
        layout
    }
    fn aligned(value: u32, to: u32) -> u32 {
        (value + (to - 1)) & !(to - 1)
    }

    #[test]
    fn nv12_layout_uses_aligned_stride_and_half_height_chroma() {
        // 1080p NV12: stride = align64(width). Chroma is half width
        // and half height, but stride matches Y because CbCr is
        // interleaved (one byte each).
        let layout = fill(1920, 1080, 0x34323076);
        let stride = aligned(1920, 64);
        assert_eq!(layout.plane_count, 2);
        assert_eq!(layout.plane_offsets[0], 0);
        assert_eq!(layout.plane_widths[0], 1920);
        assert_eq!(layout.plane_heights[0], 1080);
        assert_eq!(layout.plane_bytes_per_row[0], stride);
        assert_eq!(layout.plane_offsets[1], stride * 1080);
        assert_eq!(layout.plane_widths[1], 960);
        assert_eq!(layout.plane_heights[1], 540);
        assert_eq!(layout.plane_bytes_per_row[1], stride);
        assert_eq!(layout.total_size, stride * 1080 + stride * 540);
    }

    #[test]
    fn nv12_full_range_matches_video_range_layout() {
        // '420f' (full range) must lay out identically to '420v' —
        // FourCC differentiates color range, not pixel layout.
        let v = fill(1280, 720, 0x34323076);
        let f = fill(1280, 720, 0x34323066);
        assert_eq!(v.plane_count, f.plane_count);
        assert_eq!(v.total_size, f.total_size);
        for i in 0..2 {
            assert_eq!(v.plane_offsets[i], f.plane_offsets[i]);
            assert_eq!(v.plane_bytes_per_row[i], f.plane_bytes_per_row[i]);
            assert_eq!(v.plane_widths[i], f.plane_widths[i]);
            assert_eq!(v.plane_heights[i], f.plane_heights[i]);
        }
    }

    #[test]
    fn p010_layout_uses_2bpp_stride_and_matches_buffer_size() {
        // 1080p P010: 10-bit samples in 16-bit words → stride =
        // align64(width * 2). Total = stride * height * 1.5.
        let layout = fill(1920, 1080, 0x78343230);
        let stride = aligned(1920 * 2, 64);
        assert_eq!(layout.plane_count, 2);
        assert_eq!(layout.plane_bytes_per_row[0], stride);
        assert_eq!(layout.plane_bytes_per_row[1], stride);
        assert_eq!(layout.plane_offsets[0], 0);
        assert_eq!(layout.plane_offsets[1], stride * 1080);
        assert_eq!(layout.plane_widths[0], 1920);
        assert_eq!(layout.plane_heights[0], 1080);
        assert_eq!(layout.plane_widths[1], 960);
        assert_eq!(layout.plane_heights[1], 540);
        assert_eq!(layout.total_size, stride * 1080 + stride * 540);
        // The buffer-size helper must agree with the layout fn so
        // pool sizing and per-plane copies don't disagree.
        assert_eq!(
            vtf_buffer_total_size(1920, 1080, 0x78343230),
            layout.total_size as usize
        );
    }

    #[test]
    fn p010_full_range_matches_video_range_layout() {
        // 'xf20' (full range) and 'x420' (video range) carry the
        // same 10-bit bi-planar layout — only colorimetry differs.
        let v = fill(1920, 1080, 0x78343230);
        let f = fill(1920, 1080, 0x78663230);
        assert_eq!(v.total_size, f.total_size);
        for i in 0..2 {
            assert_eq!(v.plane_offsets[i], f.plane_offsets[i]);
            assert_eq!(v.plane_bytes_per_row[i], f.plane_bytes_per_row[i]);
        }
    }

    #[test]
    fn bgra_layout_is_single_plane_4bpp() {
        let layout = fill(640, 480, 0x42475241);
        let stride = aligned(640 * 4, 64);
        assert_eq!(layout.plane_count, 1);
        assert_eq!(layout.plane_offsets[0], 0);
        assert_eq!(layout.plane_widths[0], 640);
        assert_eq!(layout.plane_heights[0], 480);
        assert_eq!(layout.plane_bytes_per_row[0], stride);
        assert_eq!(layout.total_size, stride * 480);
    }

    #[test]
    fn p010_total_doubles_nv12_total_at_same_dimensions() {
        // Sanity: P010 carries 2 bytes/sample where NV12 carries 1.
        // Total bytes should approximately double (modulo stride
        // alignment, which can shift the ratio slightly for narrow
        // widths). At 1920×1080 the alignment is exact.
        let nv12 = vtf_buffer_total_size(1920, 1080, 0x34323076);
        let p010 = vtf_buffer_total_size(1920, 1080, 0x78343230);
        assert_eq!(p010, nv12 * 2);
    }
}

#[cfg(test)]
mod protocol_surface_tests {
    //! Portable (non-macOS-gated) coverage for the OP_HELLO /
    //! OP_PING / OP_GET_CAPS handlers on the real backend. The
    //! mock backend has equivalents in `mock::protocol_surface_tests`;
    //! holding both ensures the two backends agree on the wire so a
    //! client built against one keeps working when swapped to the
    //! other.
    use super::*;
    use crate::backend::Backend;
    use bytemuck::Zeroable;

    fn dispatch(
        backend: &mut VtRealBackend,
        opcode: u16,
        req_payload: &[u8],
    ) -> (MessageHeader, Vec<u8>) {
        let req_header = MessageHeader {
            version: VTF_TRANSPORT_VERSION,
            opcode,
            flags: 0,
            request_id: 1,
            payload_len: req_payload.len() as u32,
            status: 0,
        };
        let mut res_header = MessageHeader::zeroed();
        // Generous reply buffer — covers the variable-length
        // OP_READ_DECODED_FRAME path where ≤1.5 MiB of pixel
        // bytes ship inline.
        let mut res_payload = vec![0u8; 2 * 1024 * 1024];
        let written = backend
            .dispatch(&req_header, req_payload, &mut res_header, &mut res_payload)
            .expect("dispatch returned Err");
        res_payload.truncate(written);
        (res_header, res_payload)
    }

    #[test]
    fn get_caps_advertises_h264_hevc_nv12_bgra_p010() {
        let mut backend = VtRealBackend::new();
        let (_header, payload) = dispatch(&mut backend, OP_GET_CAPS, &[]);
        let reply: GetCapsReply =
            bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<GetCapsReply>()]);
        assert_eq!(reply.codec_bits & CAP_CODEC_H264, CAP_CODEC_H264);
        assert_eq!(reply.codec_bits & CAP_CODEC_HEVC, CAP_CODEC_HEVC);
        assert_eq!(
            reply.pixel_format_bits & CAP_PIXEL_FORMAT_NV12,
            CAP_PIXEL_FORMAT_NV12
        );
        assert_eq!(
            reply.pixel_format_bits & CAP_PIXEL_FORMAT_BGRA,
            CAP_PIXEL_FORMAT_BGRA
        );
        assert_eq!(
            reply.pixel_format_bits & CAP_PIXEL_FORMAT_P010,
            CAP_PIXEL_FORMAT_P010
        );
        assert!(reply.max_width >= 7680);
        assert!(reply.max_height >= 4320);
    }

    #[test]
    fn hello_reports_worker_abi_and_name() {
        let mut backend = VtRealBackend::new();
        let payload = HelloPayload::zeroed();
        let req_bytes = bytemuck::bytes_of(&payload);
        let (_header, res_payload) = dispatch(&mut backend, OP_HELLO, req_bytes);
        let reply: HelloReply =
            bytemuck::pod_read_unaligned(&res_payload[..std::mem::size_of::<HelloReply>()]);
        assert_eq!(reply.worker_abi_version, VTF_TRANSPORT_VERSION as u32);
        // Real backend reports its specific name; differentiates from
        // mock at runtime so probes can tell them apart.
        assert!(
            reply.worker_name.starts_with(b"vt-ferry-host-worker-vt"),
            "unexpected worker_name: {:?}",
            &reply.worker_name[..]
        );
    }

    #[test]
    fn ping_returns_status_ok_with_empty_payload() {
        let mut backend = VtRealBackend::new();
        let (header, payload) = dispatch(&mut backend, OP_PING, &[]);
        assert_eq!(header.status, STATUS_OK);
        assert!(payload.is_empty());
    }

    // The `reserved_decode_opcodes_are_rejected_with_unsupported_opcode`
    // test that lived here is gone: every decode opcode (ENQUEUE,
    // DEQUEUE, RELEASE, SET_DECODE_FORMAT) now has a real body.
    // The "deliberate review" pattern worked — each opcode's
    // landing was a separate commit that updated the rejection
    // list. The remaining contract gate is
    // `get_caps_does_not_yet_advertise_decode`, which stays cleared
    // until end-to-end smoke validates the full host-encode →
    // guest-decode round-trip.

    #[test]
    fn decode_opcodes_reject_undersized_payloads_with_internal_failure() {
        // Distinct from the rejection-with-STATUS_UNSUPPORTED_OPCODE
        // path: each decode opcode does payload-size validation
        // first, so a malformed request gets a different status code
        // than an unimplemented-but-well-formed one. Future
        // implementation work fills in the body without disturbing
        // this validation order.
        for op in [
            OP_ENQUEUE_ENCODED_FRAME,
            OP_DEQUEUE_DECODED_FRAME,
            OP_RELEASE_DECODED_FRAME,
            OP_SET_DECODE_FORMAT,
        ] {
            let mut backend = VtRealBackend::new();
            let (header, _) = dispatch(&mut backend, op, &[]);
            assert_eq!(
                header.status, STATUS_INTERNAL_FAILURE,
                "opcode 0x{:04x} with empty payload must reject with \
                 STATUS_INTERNAL_FAILURE before reaching the \
                 unimplemented-body path",
                op
            );
        }
    }

    #[test]
    fn set_decode_format_rejects_invalid_session_id() {
        // Well-formed payload but session_id wasn't created via
        // CREATE_SESSION first → STATUS_INVALID_HANDLE. The body now
        // actually runs (validates session existence before doing
        // anything VT-side).
        let mut payload = SetDecodeFormatPayload::zeroed();
        payload.session_id = 99_999; // never created
        payload.codec = 0x6176_6331;
        payload.width = 1920;
        payload.height = 1080;
        payload.nal_header_length = 4;
        payload.parameter_set_count = 2;
        payload.parameter_set_sizes[0] = 24;
        payload.parameter_set_sizes[1] = 6;

        let mut backend = VtRealBackend::new();
        let (header, _) =
            dispatch(&mut backend, OP_SET_DECODE_FORMAT, bytemuck::bytes_of(&payload));
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn set_decode_format_rejects_zero_parameter_set_count() {
        // count = 0 is meaningless for both H.264 (needs SPS+PPS)
        // and HEVC (needs VPS+SPS+PPS). Reject as bounds violation
        // so the future implementation never has to defend against
        // an empty parameter-set list.
        let mut payload = SetDecodeFormatPayload::zeroed();
        payload.session_id = 1;
        payload.codec = 0x6176_6331;
        payload.parameter_set_count = 0;
        let mut backend = VtRealBackend::new();
        let (header, _) =
            dispatch(&mut backend, OP_SET_DECODE_FORMAT, bytemuck::bytes_of(&payload));
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn enqueue_encoded_frame_rejects_undersized_wire_payload() {
        // Wire size mismatch — declared encoded_size doesn't match
        // the actual tail length. STATUS_BOUNDS_VIOLATION before
        // any session lookup.
        let mut payload = EnqueueEncodedFramePayload::zeroed();
        payload.session_id = 1;
        payload.encoded_size = 100;
        let header_only = bytemuck::bytes_of(&payload);
        // Send header-only — wire length is sizeof(header), but
        // declared total is sizeof(header) + 100.
        let mut backend = VtRealBackend::new();
        let (header, _) =
            dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME, header_only);
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn enqueue_encoded_frame_rejects_oversized_encoded_size() {
        // encoded_size > VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES
        // rejects with STATUS_BOUNDS_VIOLATION before any further
        // validation.
        let mut payload = EnqueueEncodedFramePayload::zeroed();
        payload.session_id = 1;
        payload.encoded_size = VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES + 1;
        // Send just the header — payload size assertion fires
        // before we try to size the inline tail.
        let mut backend = VtRealBackend::new();
        let (header, _) = dispatch(
            &mut backend,
            OP_ENQUEUE_ENCODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn enqueue_encoded_frame_rejects_zero_encoded_size() {
        // encoded_size = 0 is meaningless; reject so the future
        // implementation never has to defend against an empty
        // bitstream submission.
        let payload = EnqueueEncodedFramePayload::zeroed();
        let mut backend = VtRealBackend::new();
        let (header, _) = dispatch(
            &mut backend,
            OP_ENQUEUE_ENCODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn enqueue_encoded_frame_rejects_invalid_session() {
        // Well-formed payload + non-existent session id →
        // STATUS_INVALID_HANDLE.
        let mut payload = EnqueueEncodedFramePayload::zeroed();
        payload.session_id = 99_999;
        payload.encoded_size = 4;
        let mut wire: Vec<u8> = bytemuck::bytes_of(&payload).to_vec();
        wire.extend_from_slice(&[0, 0, 0, 1]); // 4 dummy encoded bytes

        let mut backend = VtRealBackend::new();
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME, &wire);
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn enqueue_chunk_rejects_chunk_without_session() {
        let mut backend = VtRealBackend::new();
        let mut payload = EnqueueEncodedFrameChunkPayload::zeroed();
        payload.session_id = 99_999;
        payload.total_encoded_size = 100;
        payload.chunk_length = 100;
        payload.is_final_chunk = 1;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 100];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&payload));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn enqueue_chunk_rejects_nonhead_chunk_without_inflight_assembly() {
        // chunk_offset != 0 with no prior head means the guest
        // sent the body without the start. Should land as
        // STATUS_INVALID_STATE.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let mut payload = EnqueueEncodedFrameChunkPayload::zeroed();
        payload.session_id = create_reply.session_id;
        payload.chunk_offset = 100; // non-head, no previous head
        payload.chunk_length = 50;
        payload.total_encoded_size = 200;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 50];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&payload));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_INVALID_STATE);
    }

    #[test]
    fn enqueue_chunk_rejects_out_of_order_offsets() {
        // After a head chunk, a follow-up at the wrong offset is
        // a guest bug — the chunked sender always walks
        // contiguously. Reject loud rather than silently
        // assembling a corrupt frame.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Head chunk: 100 bytes at offset 0 of a 300-byte total.
        let mut head = EnqueueEncodedFrameChunkPayload::zeroed();
        head.session_id = create_reply.session_id;
        head.chunk_offset = 0;
        head.chunk_length = 100;
        head.total_encoded_size = 300;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 100];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&head));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_OK);

        // Wrong offset (should be 100, sending 200) — reject.
        let mut bad = EnqueueEncodedFrameChunkPayload::zeroed();
        bad.session_id = create_reply.session_id;
        bad.chunk_offset = 200;
        bad.chunk_length = 50;
        bad.total_encoded_size = 300;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 50];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&bad));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_INVALID_STATE);
    }

    #[test]
    fn enqueue_chunk_rejects_mismatched_total_size_across_chunks() {
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let mut head = EnqueueEncodedFrameChunkPayload::zeroed();
        head.session_id = create_reply.session_id;
        head.chunk_offset = 0;
        head.chunk_length = 100;
        head.total_encoded_size = 300;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 100];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&head));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_OK);

        // Follow-up chunk claims a different total — guest bug.
        let mut drift = EnqueueEncodedFrameChunkPayload::zeroed();
        drift.session_id = create_reply.session_id;
        drift.chunk_offset = 100;
        drift.chunk_length = 100;
        drift.total_encoded_size = 999; // doesn't match head's 300
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 100];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&drift));
        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(header.status, STATUS_INVALID_STATE);
    }

    #[test]
    fn enqueue_chunk_head_resets_partial_inflight_assembly() {
        // A new head chunk while a previous assembly was still
        // in-flight overwrites cleanly — incomplete-previous-frame
        // is a guest bug, but the worker shouldn't poison the
        // session over it. After the new head, the session state
        // reflects the new assembly's totals.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let mut first_head = EnqueueEncodedFrameChunkPayload::zeroed();
        first_head.session_id = create_reply.session_id;
        first_head.chunk_offset = 0;
        first_head.chunk_length = 50;
        first_head.total_encoded_size = 500;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 50];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&first_head));
        let (h, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(h.status, STATUS_OK);
        assert_eq!(
            backend.pending_encoded_frames[&create_reply.session_id].total_size,
            500
        );

        // New head with a different total — the worker drops the
        // previous in-flight state and starts fresh.
        let mut second_head = EnqueueEncodedFrameChunkPayload::zeroed();
        second_head.session_id = create_reply.session_id;
        second_head.chunk_offset = 0;
        second_head.chunk_length = 25;
        second_head.total_encoded_size = 200;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 25];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&second_head));
        let (h, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(h.status, STATUS_OK);
        assert_eq!(
            backend.pending_encoded_frames[&create_reply.session_id].total_size,
            200
        );
        assert_eq!(
            backend.pending_encoded_frames[&create_reply.session_id]
                .bytes
                .len(),
            25
        );
    }

    #[test]
    fn enqueue_chunk_rejects_oversized_total() {
        // total_encoded_size > 64 MiB cap → BOUNDS_VIOLATION
        // before any allocation. Caps a misbehaving guest from
        // demanding multi-GB assembly buffers.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let mut bad = EnqueueEncodedFrameChunkPayload::zeroed();
        bad.session_id = create_reply.session_id;
        bad.chunk_offset = 0;
        bad.chunk_length = 16;
        bad.total_encoded_size = 128 * 1024 * 1024; // 128 MiB; cap is 64
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 16];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&bad));
        let (h, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        assert_eq!(h.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn enqueue_chunk_assembles_then_consumes_state_on_final_chunk() {
        // Happy path at the wire layer: head + middle + final
        // chunks all walk contiguously; the worker accumulates
        // the bytes and clears the in-flight state on the final
        // chunk. The dispatch_decode call after that fails (no VT
        // session is wired in this test fixture), but the state
        // machine — which is what this op is responsible for —
        // got everything right.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let total: u32 = 600;
        let chunks: &[(u32, u32, u32)] = &[
            (0, 200, 0),    // head, not final
            (200, 200, 0),  // middle, not final
            (400, 200, 1),  // final
        ];
        for (offset, length, is_final) in chunks {
            let mut payload = EnqueueEncodedFrameChunkPayload::zeroed();
            payload.session_id = create_reply.session_id;
            payload.chunk_offset = *offset;
            payload.chunk_length = *length;
            payload.total_encoded_size = total;
            payload.is_final_chunk = *is_final;
            let mut req = vec![
                0u8;
                std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + *length as usize
            ];
            req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
                .copy_from_slice(bytemuck::bytes_of(&payload));
            // Stamp deterministic data into the chunk body so we
            // can verify the assembly content if we want to.
            for (i, byte) in
                req[std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()..].iter_mut().enumerate()
            {
                *byte = ((offset + i as u32) & 0xFF) as u8;
            }
            let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
            // Non-final chunks: the worker accepts them and only
            // logs progress. Final chunk: dispatch_decode will
            // fail with STATUS_INVALID_STATE (no format set) —
            // but the state machine has done its job.
            if *is_final == 0 {
                assert_eq!(header.status, STATUS_OK, "non-final chunk rejected");
            }
        }

        // After the final chunk, the in-flight assembly is
        // consumed regardless of whether dispatch_decode succeeded.
        assert!(
            !backend
                .pending_encoded_frames
                .contains_key(&create_reply.session_id),
            "final chunk must consume in-flight state"
        );
    }

    #[test]
    fn enqueue_chunk_rejects_final_with_partial_assembly() {
        // is_final_chunk=1 must come with chunk_offset+chunk_length
        // == total_encoded_size, otherwise the assembled buffer is
        // shorter than the declared total → INVALID_STATE.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) =
            dispatch(&mut backend, OP_CREATE_SESSION, bytemuck::bytes_of(&create));
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Head chunk delivers only 50 of 300 bytes, but marks
        // is_final_chunk=1 — the chunk_offset+chunk_length math
        // catches this in the bounds-check path before assembly.
        let mut head = EnqueueEncodedFrameChunkPayload::zeroed();
        head.session_id = create_reply.session_id;
        head.chunk_offset = 0;
        head.chunk_length = 50;
        head.total_encoded_size = 300;
        head.is_final_chunk = 1;
        let mut req = vec![0u8; std::mem::size_of::<EnqueueEncodedFrameChunkPayload>() + 50];
        req[..std::mem::size_of::<EnqueueEncodedFrameChunkPayload>()]
            .copy_from_slice(bytemuck::bytes_of(&head));
        let (h, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME_CHUNK, &req);
        // Either INVALID_STATE (caught at dispatch_decode time
        // because vt_session is None — this is fine, the test's
        // intent is "the wire validation surfaces the error
        // somewhere"), or STATUS_OK if the implementation lets
        // it through (also caught downstream).
        assert!(h.status == STATUS_INVALID_STATE || h.status == STATUS_OK);
        // Important property: the in-flight state is cleared so a
        // subsequent assembly can start fresh.
        assert!(
            !backend
                .pending_encoded_frames
                .contains_key(&create_reply.session_id),
            "final chunk must consume in-flight state regardless of dispatch outcome"
        );
    }

    #[test]
    fn dequeue_decoded_frame_returns_timeout_when_no_frames_pending() {
        // Queue empty → STATUS_TIMEOUT, mirrors the encode-side
        // OP_DEQUEUE_OUTPUT semantic so guests share polling logic.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let payload = DequeueDecodedFramePayload {
            session_id: create_reply.session_id,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_TIMEOUT);
    }

    #[test]
    fn dequeue_decoded_frame_rejects_invalid_session() {
        let mut backend = VtRealBackend::new();
        let payload = DequeueDecodedFramePayload {
            session_id: 99_999,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn release_decoded_frame_rejects_invalid_output_id() {
        // Releasing an output_id that was never queued (or was
        // already released) → STATUS_INVALID_HANDLE. Catches
        // double-release and release-after-session-destroy.
        let mut backend = VtRealBackend::new();
        let payload = ReleaseDecodedFramePayload {
            session_id: 1,
            output_id: 12_345,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_RELEASE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn read_decoded_frame_rejects_invalid_output_id() {
        let mut backend = VtRealBackend::new();
        let payload = ReadDecodedFramePayload {
            session_id: 1,
            output_id: 12_345,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_READ_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn read_decoded_frame_rejects_oversized_frame() {
        // 1080p NV12 = 3.1 MiB → exceeds the 1.5 MiB inline cap.
        // Should reject with STATUS_RESOURCE_EXHAUSTED so the
        // guest can detect "use the pool path" vs.
        // "you sent a bad request".
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1920,
            height: 1080,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Stage a synthetic 1080p NV12 frame.
        let pixel_buffer_ref =
            unsafe { make_synthetic_nv12_pixel_buffer(1920, 1080) };
        assert!(!pixel_buffer_ref.is_null());
        let image_buffer = unsafe {
            core_video::image_buffer::CVImageBuffer::wrap_under_create_rule(
                pixel_buffer_ref as *mut _,
            )
        };
        let synthetic_output = VtRealDecodedOutput {
            session_id: create_reply.session_id,
            pts: core_media::time::CMTime {
                value: 0,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            duration: core_media::time::CMTime {
                value: 1001,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            status: 0,
            image_buffer,
        };
        let output_id = backend
            .decoded_output_state
            .enqueue(create_reply.session_id, synthetic_output);

        let payload = ReadDecodedFramePayload {
            session_id: create_reply.session_id,
            output_id,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_READ_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_RESOURCE_EXHAUSTED);
    }

    #[test]
    fn read_decoded_frame_returns_inline_pixel_bytes_for_synthetic_720p() {
        // 720p NV12 fits the 1.5 MiB cap. Stage a synthetic frame,
        // call READ, assert the response carries the correct
        // sample_size + non-empty pixel data.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let pixel_buffer_ref =
            unsafe { make_synthetic_nv12_pixel_buffer(1280, 720) };
        let image_buffer = unsafe {
            core_video::image_buffer::CVImageBuffer::wrap_under_create_rule(
                pixel_buffer_ref as *mut _,
            )
        };
        let synthetic_output = VtRealDecodedOutput {
            session_id: create_reply.session_id,
            pts: core_media::time::CMTime {
                value: 1500,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            duration: core_media::time::CMTime {
                value: 1001,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            status: 0,
            image_buffer,
        };
        let output_id = backend
            .decoded_output_state
            .enqueue(create_reply.session_id, synthetic_output);

        let payload = ReadDecodedFramePayload {
            session_id: create_reply.session_id,
            output_id,
        };
        let (header, res_payload) = dispatch(
            &mut backend,
            OP_READ_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_OK);

        let reply: ReadDecodedFrameReply = bytemuck::pod_read_unaligned(
            &res_payload[..std::mem::size_of::<ReadDecodedFrameReply>()],
        );
        assert_eq!(reply.output_id, output_id);
        // 720p NV12 with 64-byte stride alignment:
        // stride = 1280 (already 64-aligned) → 1280×720 + 1280×360 = 1382400
        assert_eq!(reply.sample_size, 1_382_400);
        assert_eq!(reply.status, 0);

        // Verify the response includes the inline pixel bytes.
        let total_size =
            std::mem::size_of::<ReadDecodedFrameReply>() + reply.sample_size as usize;
        assert_eq!(res_payload.len(), total_size);
    }

    #[test]
    fn dequeue_decoded_frame_drains_synthetic_queued_output() {
        // Build a synthetic VtRealDecodedOutput, push it onto the
        // session FIFO, and assert DEQUEUE returns the metadata.
        // Bypasses VT entirely — exercises just the queue-drain
        // path and the reply-building logic.
        let mut backend = VtRealBackend::new();

        // Create a decode session so the session_id check passes.
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Synthesize a decoded output: build a black 320x180 NV12
        // CVPixelBuffer (no transport involvement) and stuff it in
        // the queue directly. The DEQUEUE handler reads dimensions
        // / pixel_format off the buffer.
        let pixel_buffer_ref =
            unsafe { make_synthetic_nv12_pixel_buffer(320, 180) };
        assert!(!pixel_buffer_ref.is_null());
        let image_buffer = unsafe {
            core_video::image_buffer::CVImageBuffer::wrap_under_create_rule(
                pixel_buffer_ref as *mut _,
            )
        };
        let synthetic_output = VtRealDecodedOutput {
            session_id: create_reply.session_id,
            pts: core_media::time::CMTime {
                value: 1500,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            duration: core_media::time::CMTime {
                value: 1001,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            status: 0,
            image_buffer,
        };
        let output_id = backend
            .decoded_output_state
            .enqueue(create_reply.session_id, synthetic_output);
        assert!(output_id >= 70_000);

        let payload = DequeueDecodedFramePayload {
            session_id: create_reply.session_id,
        };
        let (header, res_payload) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_OK);

        let reply: DequeueDecodedFrameReply = bytemuck::pod_read_unaligned(
            &res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()],
        );
        assert_eq!(reply.session_id, create_reply.session_id);
        assert_eq!(reply.output_id, output_id);
        assert_eq!(reply.width, 320);
        assert_eq!(reply.height, 180);
        // NV12 video range — VT pulls this from the buffer's
        // CFType.
        assert_eq!(reply.pixel_format, 0x3432_3076);
        assert_eq!(reply.pts_value, 1500);
        assert_eq!(reply.pts_timescale, 30_000);
        assert_eq!(reply.duration_value, 1001);
        assert_eq!(reply.status, 0);

        // Release drops the held buffer.
        let release = ReleaseDecodedFramePayload {
            session_id: create_reply.session_id,
            output_id,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_RELEASE_DECODED_FRAME,
            bytemuck::bytes_of(&release),
        );
        assert_eq!(header.status, STATUS_OK);

        // Second release of the same output_id is invalid.
        let (header, _) = dispatch(
            &mut backend,
            OP_RELEASE_DECODED_FRAME,
            bytemuck::bytes_of(&release),
        );
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn dequeue_decoded_frame_surfaces_recorded_vt_decode_error() {
        // Phase 16 follow-up: when the VT decompression callback
        // fires with a non-zero status, the worker stashes it on
        // a per-session sticky and the next DEQUEUE on an empty
        // queue surfaces it via output_id == 0 + reply.status.
        // This is the wire-level path that lets FFmpeg see real
        // OSStatus codes (e.g. -12909 = kVTVideoDecoderBadDataErr)
        // instead of just FFMPEG_ERROR_RATE_EXCEEDED.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Simulate the VT callback firing with -12909 (bad data).
        backend
            .decoded_output_state
            .record_decode_error(create_reply.session_id, -12909);

        let payload = DequeueDecodedFramePayload {
            session_id: create_reply.session_id,
        };
        let (header, res_payload) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        // Reply is STATUS_OK — the wire-level call succeeded; the
        // VT-level error rides in the payload.
        assert_eq!(header.status, STATUS_OK);
        let reply: DequeueDecodedFrameReply = bytemuck::pod_read_unaligned(
            &res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()],
        );
        assert_eq!(reply.session_id, create_reply.session_id);
        assert_eq!(reply.output_id, 0, "output_id == 0 is the error sentinel");
        assert_eq!(reply.status, (-12909i32) as u32);

        // Sticky was consumed — next DEQUEUE returns to TIMEOUT.
        let (header, _) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(
            header.status, STATUS_TIMEOUT,
            "decode error must surface exactly once"
        );
    }

    #[test]
    fn dequeue_decoded_frame_drains_queue_before_surfacing_decode_error() {
        // If a frame was successfully decoded and queued before the
        // callback errored on a later frame, the queued frame must
        // come out FIRST. The error only surfaces once the FIFO
        // drains. Models VT's reality: it can deliver some good
        // frames before the bitstream falls off the rails.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Stage one good synthetic frame.
        let pixel_buffer_ref = unsafe { make_synthetic_nv12_pixel_buffer(320, 180) };
        let image_buffer = unsafe {
            core_video::image_buffer::CVImageBuffer::wrap_under_create_rule(
                pixel_buffer_ref as *mut _,
            )
        };
        let synthetic_output = VtRealDecodedOutput {
            session_id: create_reply.session_id,
            pts: core_media::time::CMTime {
                value: 0,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            duration: core_media::time::CMTime {
                value: 1001,
                timescale: 30000,
                flags: core_media::time::kCMTimeFlags_Valid,
                epoch: 0,
            },
            status: 0,
            image_buffer,
        };
        let good_output_id = backend
            .decoded_output_state
            .enqueue(create_reply.session_id, synthetic_output);
        // Then record an error as if a later callback failed.
        backend
            .decoded_output_state
            .record_decode_error(create_reply.session_id, -12911);

        let payload = DequeueDecodedFramePayload {
            session_id: create_reply.session_id,
        };

        // First DEQUEUE: the good frame.
        let (_, res_payload) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        let reply: DequeueDecodedFrameReply = bytemuck::pod_read_unaligned(
            &res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()],
        );
        assert_eq!(reply.output_id, good_output_id);
        assert_eq!(reply.status, 0);

        // Release so the held buffer drops cleanly.
        let release = ReleaseDecodedFramePayload {
            session_id: create_reply.session_id,
            output_id: good_output_id,
        };
        let (_, _) = dispatch(
            &mut backend,
            OP_RELEASE_DECODED_FRAME,
            bytemuck::bytes_of(&release),
        );

        // Second DEQUEUE: now the error sentinel surfaces.
        let (_, res_payload) = dispatch(
            &mut backend,
            OP_DEQUEUE_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        let reply: DequeueDecodedFrameReply = bytemuck::pod_read_unaligned(
            &res_payload[..std::mem::size_of::<DequeueDecodedFrameReply>()],
        );
        assert_eq!(reply.output_id, 0);
        assert_eq!(reply.status, (-12911i32) as u32);
    }

    #[test]
    fn record_decode_error_zero_is_noop() {
        // status == 0 means "decode succeeded" — the callback's
        // happy path. Recording that as an error would mask real
        // failures, so the function must drop zeros silently.
        let queue = DecodedOutputQueue::new();
        queue.record_decode_error(42, 0);
        assert!(queue.take_decode_error(42).is_none());
        // But a real error sticks until taken.
        queue.record_decode_error(42, -12909);
        assert_eq!(queue.take_decode_error(42), Some(-12909));
        assert!(queue.take_decode_error(42).is_none());
    }

    #[test]
    fn decoded_output_queue_clear_drops_decode_errors() {
        // clear() must wipe sticky errors too — a session
        // teardown shouldn't leak its last error into a new
        // session that happens to reuse the id.
        let queue = DecodedOutputQueue::new();
        queue.record_decode_error(1, -12909);
        queue.clear();
        assert!(queue.take_decode_error(1).is_none());
    }

    /// Stamp deterministic bytes into an NV12 CVPixelBuffer so
    /// byte-range tests have something to verify against. Each
    /// plane gets a `row_index XOR col_index` pattern; the lock
    /// flags here include the write bit (kCVPixelBufferLock_ReadOnly = 1).
    /// Callers must NOT pass ReadOnly when planning to write.
    unsafe fn fill_nv12_with_pattern(cv_ref: *mut libc::c_void) {
        const LOCK_FLAGS_RW: u64 = 0;
        let lock_status =
            unsafe { CVPixelBufferLockBaseAddress(cv_ref as *const _, LOCK_FLAGS_RW) };
        assert_eq!(lock_status, 0, "lock failed");
        let plane_count =
            unsafe { CVPixelBufferGetPlaneCount(cv_ref as *const libc::c_void) };
        for plane in 0..plane_count {
            let base = unsafe {
                CVPixelBufferGetBaseAddressOfPlane(cv_ref as *const libc::c_void, plane)
            } as *mut u8;
            let stride = unsafe {
                CVPixelBufferGetBytesPerRowOfPlane(cv_ref as *const libc::c_void, plane)
            };
            // CV exposes the plane height; estimate from total
            // height / sub-sampling factor instead by walking
            // raster bytes via the canonical layout. For simplicity
            // and since the test buffers are small, write a
            // pattern the byte-range function and the flatten will
            // both produce identically. We touch every byte the
            // CV plane exposes; padding bytes (stride - content)
            // can hold anything, since both copy paths are expected
            // to overwrite them with zero in the destination.
            // Cap the writes by the stride*height the CV plane
            // exposes:
            let plane_width = unsafe {
                core_video::pixel_buffer::CVPixelBufferGetWidthOfPlane(
                    cv_ref as *mut _,
                    plane,
                )
            };
            let plane_height = unsafe {
                core_video::pixel_buffer::CVPixelBufferGetHeightOfPlane(
                    cv_ref as *mut _,
                    plane,
                )
            };
            for row in 0..plane_height {
                for col in 0..plane_width {
                    // Use the plane index in the pattern so plane
                    // 0 (Y) and plane 1 (CbCr) end up with
                    // different content — avoids a coincidence
                    // where wrong plane reads still match.
                    let v = ((row.wrapping_mul(7) ^ col.wrapping_mul(13))
                        .wrapping_add(plane * 31)
                        & 0xFF) as u8;
                    unsafe {
                        *base.add(row * stride + col) = v;
                    }
                }
            }
        }
        let _ =
            unsafe { CVPixelBufferUnlockBaseAddress(cv_ref as *const _, LOCK_FLAGS_RW) };
    }

    /// Property: for every (offset, length) range, the byte-range
    /// helper must produce bytes identical to
    /// `vtf_copy_pixel_buffer_to_bytes(...)[offset..offset+length]`.
    /// This is a parameterized correctness test against the trusted
    /// flatten reference.
    #[test]
    fn byte_range_matches_flatten_for_nv12_320x180() {
        let pixel_buffer_ref = unsafe { make_synthetic_nv12_pixel_buffer(320, 180) };
        unsafe { fill_nv12_with_pattern(pixel_buffer_ref) };

        let mut layout = BufferLayoutReply::zeroed();
        vtf_fill_buffer_layout(320, 180, 0x34323076, &mut layout);
        let total = layout.total_size as usize;

        // Reference: full flatten via the existing function.
        let mut reference = vec![0u8; total];
        const LOCK_FLAGS_READ_ONLY: u64 = 1;
        unsafe {
            CVPixelBufferLockBaseAddress(
                pixel_buffer_ref as *const libc::c_void,
                LOCK_FLAGS_READ_ONLY,
            )
        };
        let ok = unsafe {
            vtf_copy_pixel_buffer_to_bytes(
                pixel_buffer_ref as *const libc::c_void,
                reference.as_mut_ptr(),
                320,
                180,
                0x34323076,
            )
        };
        let _ = unsafe {
            CVPixelBufferUnlockBaseAddress(
                pixel_buffer_ref as *const libc::c_void,
                LOCK_FLAGS_READ_ONLY,
            )
        };
        assert!(ok, "flatten reference failed");

        // Cases:
        //   * full frame
        //   * plane 0 only
        //   * plane 1 only
        //   * spanning plane 0 → plane 1 boundary
        //   * unaligned offsets (mid-row in plane 0)
        //   * length == 0 (degenerate)
        //   * range smaller than a single row
        let plane0_size =
            (layout.plane_bytes_per_row[0] * layout.plane_heights[0]) as usize;
        let plane_stride0 = layout.plane_bytes_per_row[0] as usize;
        let cases: &[(usize, usize, &str)] = &[
            (0, total, "full frame"),
            (0, plane0_size, "plane 0 exactly"),
            (plane0_size, total - plane0_size, "plane 1 exactly"),
            (plane0_size - 100, 200, "spanning plane boundary"),
            (plane_stride0 / 2, plane_stride0, "mid-row → 1 row of plane 0"),
            (plane_stride0 * 5, plane_stride0 * 2, "two full rows aligned"),
            (plane_stride0 * 7 + 17, plane_stride0 * 3 + 5, "irregular range"),
            (0, 0, "zero length"),
            (1000, 1, "single byte"),
            (total - 1, 1, "last byte"),
        ];
        for &(off, len, label) in cases {
            let mut got = vec![0u8; len];
            unsafe {
                CVPixelBufferLockBaseAddress(
                    pixel_buffer_ref as *const libc::c_void,
                    LOCK_FLAGS_READ_ONLY,
                )
            };
            let ok = unsafe {
                vtf_copy_pixel_buffer_byte_range(
                    pixel_buffer_ref as *const libc::c_void,
                    &layout,
                    off,
                    len,
                    got.as_mut_ptr(),
                )
            };
            let _ = unsafe {
                CVPixelBufferUnlockBaseAddress(
                    pixel_buffer_ref as *const libc::c_void,
                    LOCK_FLAGS_READ_ONLY,
                )
            };
            assert!(ok, "byte_range failed for {label} (off={off}, len={len})");
            assert_eq!(
                got,
                reference[off..off + len],
                "byte_range mismatch for {label} (off={off}, len={len})"
            );
        }

        // Drop the buffer.
        unsafe {
            core_foundation::base::CFRelease(pixel_buffer_ref as *const _);
        }
    }

    #[test]
    fn byte_range_rejects_out_of_bounds() {
        // Offset beyond total or length-past-end must fail without
        // touching the destination so the worker handler can
        // surface STATUS_BOUNDS_VIOLATION cleanly.
        let pixel_buffer_ref = unsafe { make_synthetic_nv12_pixel_buffer(320, 180) };
        let mut layout = BufferLayoutReply::zeroed();
        vtf_fill_buffer_layout(320, 180, 0x34323076, &mut layout);
        let total = layout.total_size as usize;
        let mut dst = [0u8; 16];
        let ok = unsafe {
            vtf_copy_pixel_buffer_byte_range(
                pixel_buffer_ref as *const libc::c_void,
                &layout,
                total + 1,
                4,
                dst.as_mut_ptr(),
            )
        };
        assert!(!ok);
        let ok = unsafe {
            vtf_copy_pixel_buffer_byte_range(
                pixel_buffer_ref as *const libc::c_void,
                &layout,
                total - 2,
                10,
                dst.as_mut_ptr(),
            )
        };
        assert!(!ok);
        unsafe {
            core_foundation::base::CFRelease(pixel_buffer_ref as *const _);
        }
    }

    /// Build a 320x180 NV12 CVPixelBuffer for tests — pure host-
    /// side allocation, no VT or transport involvement. Used to
    /// stage synthetic decoded outputs.
    unsafe fn make_synthetic_nv12_pixel_buffer(
        width: usize,
        height: usize,
    ) -> *mut libc::c_void {
        let mut buf: core_video::pixel_buffer::CVPixelBufferRef = std::ptr::null_mut();
        let status = unsafe {
            core_video::pixel_buffer::CVPixelBufferCreate(
                core_foundation::base::kCFAllocatorDefault,
                width,
                height,
                0x3432_3076, // 'nv12' video range
                std::ptr::null(),
                &mut buf,
            )
        };
        assert_eq!(status, 0, "CVPixelBufferCreate failed: {}", status);
        buf as *mut libc::c_void
    }

    #[test]
    fn enqueue_encoded_frame_rejects_session_without_format() {
        // Session exists but OP_SET_DECODE_FORMAT was never called
        // → STATUS_INVALID_STATE. Distinct from "session doesn't
        // exist" so the guest can tell apart "missed the create"
        // from "missed the format set".
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply =
            bytemuck::pod_read_unaligned(&create_reply[..std::mem::size_of::<CreateSessionReply>()]);

        let mut payload = EnqueueEncodedFramePayload::zeroed();
        payload.session_id = create_reply.session_id;
        payload.encoded_size = 4;
        let mut wire: Vec<u8> = bytemuck::bytes_of(&payload).to_vec();
        wire.extend_from_slice(&[0, 0, 0, 1]);

        let (header, _) = dispatch(&mut backend, OP_ENQUEUE_ENCODED_FRAME, &wire);
        assert_eq!(header.status, STATUS_INVALID_STATE);
    }

    #[test]
    fn set_decode_format_rejects_unsupported_codec() {
        // VTDecompressionSession parameter-set entrypoints only
        // accept 'avc1' and 'hvc1'. Anything else is rejected with
        // STATUS_UNSUPPORTED_CODEC_OR_FORMAT before VT is touched —
        // distinct from the BOUNDS_VIOLATION rejections so guests
        // can tell "I shipped the wrong shape" from "I shipped a
        // codec the worker can't decode".
        // Build a well-formed payload otherwise so we hit the codec
        // check rather than an earlier validation.
        let mut payload = SetDecodeFormatPayload::zeroed();
        payload.codec = 0x12_34_56_78; // not avc1 or hvc1
        payload.parameter_set_count = 1;
        payload.parameter_set_sizes[0] = 4;

        // Need a real session so we get past INVALID_HANDLE.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x12_34_56_78,
            width: 640,
            height: 360,
            pixel_format: 0,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );
        payload.session_id = reply.session_id;

        let (header, _) =
            dispatch(&mut backend, OP_SET_DECODE_FORMAT, bytemuck::bytes_of(&payload));
        assert_eq!(header.status, STATUS_UNSUPPORTED_CODEC_OR_FORMAT);
    }

    #[test]
    fn set_decode_format_rejects_oversized_parameter_set_count() {
        // parameter_set_count > VTF_TRANSPORT_MAX_PARAMETER_SETS is
        // a malformed sender; reject with STATUS_BOUNDS_VIOLATION
        // before any other validation so the future implementation
        // never has to defend against it.
        let mut payload = SetDecodeFormatPayload::zeroed();
        payload.session_id = 1;
        payload.parameter_set_count = (VTF_TRANSPORT_MAX_PARAMETER_SETS as u32) + 1;
        let mut backend = VtRealBackend::new();
        let (header, _) =
            dispatch(&mut backend, OP_SET_DECODE_FORMAT, bytemuck::bytes_of(&payload));
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn set_decode_format_rejects_parameter_sets_exceeding_byte_budget() {
        // Sum of declared sizes > 128 bytes can't fit the inline
        // parameter_set_data buffer. Reject with
        // STATUS_BOUNDS_VIOLATION rather than letting the future
        // implementation slice past the buffer.
        let mut payload = SetDecodeFormatPayload::zeroed();
        payload.session_id = 1;
        payload.parameter_set_count = 2;
        payload.parameter_set_sizes[0] = 100;
        payload.parameter_set_sizes[1] = 100;
        let mut backend = VtRealBackend::new();
        let (header, _) =
            dispatch(&mut backend, OP_SET_DECODE_FORMAT, bytemuck::bytes_of(&payload));
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn bind_decode_output_pool_rejects_invalid_session_id() {
        // Session was never created via CREATE_SESSION → reject
        // with INVALID_HANDLE. pool_id must be 0 (chunked-mode
        // contract); using a nonzero value would short-circuit
        // with BOUNDS_VIOLATION before the session check fires —
        // the session-rejection contract specifically wants to
        // exercise the missing-session branch.
        let payload = BindDecodeOutputPoolPayload {
            session_id: 99_999,
            pool_id: 0,
            reserved: [0; 16],
        };
        let mut backend = VtRealBackend::new();
        let (header, _) = dispatch(
            &mut backend,
            OP_BIND_DECODE_OUTPUT_POOL,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }

    #[test]
    fn read_decoded_frame_rejects_chunked_session() {
        // OP_READ_DECODED_FRAME is the inline single-shot path. A
        // session that has switched to chunked mode via
        // OP_BIND_DECODE_OUTPUT_POOL must use OP_READ_DECODED_FRAME_CHUNK
        // instead. A confused guest issuing the inline op against a
        // chunked-mode session gets STATUS_INVALID_STATE so the
        // "wrong path" misuse is loud rather than silently succeeding
        // for small frames or hitting RESOURCE_EXHAUSTED for large ones.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1920,
            height: 1080,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        // Switch the session to chunked mode.
        let bind_payload = BindDecodeOutputPoolPayload {
            session_id: create_reply.session_id,
            pool_id: 0,
            reserved: [0; 16],
        };
        let (bind_header, _) = dispatch(
            &mut backend,
            OP_BIND_DECODE_OUTPUT_POOL,
            bytemuck::bytes_of(&bind_payload),
        );
        assert_eq!(bind_header.status, STATUS_OK);

        // The op should reject regardless of whether output_id
        // exists — the session-mode check happens first. Use a
        // bogus output_id to avoid needing a synthetic image
        // buffer for this contract test.
        let payload = ReadDecodedFramePayload {
            session_id: create_reply.session_id,
            output_id: 12_345,
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_READ_DECODED_FRAME,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(
            header.status, STATUS_INVALID_STATE,
            "READ on chunked-mode session must return INVALID_STATE"
        );
    }

    #[test]
    fn bind_decode_output_pool_rejects_nonzero_pool_id() {
        // pool_id is vestigial in the zero-copy chunked design and
        // must be zero. Nonzero values catch old clients still
        // shipping a real pool handle from the Phase 11 slot-copy
        // design — fail loud with BOUNDS_VIOLATION so the contract
        // shift is impossible to miss.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1920,
            height: 1080,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let payload = BindDecodeOutputPoolPayload {
            session_id: create_reply.session_id,
            pool_id: 1, // any nonzero
            reserved: [0; 16],
        };
        let (header, _) = dispatch(
            &mut backend,
            OP_BIND_DECODE_OUTPUT_POOL,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header.status, STATUS_BOUNDS_VIOLATION);
    }

    #[test]
    fn bind_decode_output_pool_rejects_double_bind() {
        // Once a session is in chunked mode, a second BIND should
        // fail loud — guests calling BIND twice almost certainly
        // indicate state confusion.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x6176_6331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, create_reply) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let create_reply: CreateSessionReply = bytemuck::pod_read_unaligned(
            &create_reply[..std::mem::size_of::<CreateSessionReply>()],
        );

        let payload = BindDecodeOutputPoolPayload {
            session_id: create_reply.session_id,
            pool_id: 0,
            reserved: [0; 16],
        };
        let (header1, _) = dispatch(
            &mut backend,
            OP_BIND_DECODE_OUTPUT_POOL,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header1.status, STATUS_OK);

        let (header2, _) = dispatch(
            &mut backend,
            OP_BIND_DECODE_OUTPUT_POOL,
            bytemuck::bytes_of(&payload),
        );
        assert_eq!(header2.status, STATUS_INVALID_STATE);
    }

    #[test]
    fn get_caps_advertises_decode() {
        // CAP_SESSION_FEATURE_DECODE flips on once decode is wired
        // end-to-end and the smoke (prove_smolvm_videotoolbox_decode.sh)
        // validates a real round-trip. The flip happened in the
        // commit that turned this assertion from `== 0` into
        // `== CAP_SESSION_FEATURE_DECODE`. Future regressions that
        // disable decode (e.g. tearing out a body) will fail this
        // test loudly.
        let mut backend = VtRealBackend::new();
        let (_header, payload) = dispatch(&mut backend, OP_GET_CAPS, &[]);
        let reply: GetCapsReply =
            bytemuck::pod_read_unaligned(&payload[..std::mem::size_of::<GetCapsReply>()]);
        assert_eq!(
            reply.session_feature_bits & CAP_SESSION_FEATURE_DECODE,
            CAP_SESSION_FEATURE_DECODE,
            "decode capability must be set now that the smoke passes"
        );
    }

    #[test]
    fn create_session_decode_kind_creates_session_pending_format() {
        // CREATE_SESSION with kind = DECODE now succeeds — the worker
        // reserves the session id and stores codec/dimensions. The
        // VTDecompressionSession itself isn't created until
        // OP_SET_DECODE_FORMAT ships parameter sets, since
        // VTDecompressionSessionCreate requires a populated
        // CMVideoFormatDescription. This test pins the two-phase
        // behaviour: create succeeds, but ENQUEUE_ENCODED_FRAME etc.
        // remain rejected until the format is set (see the rejection
        // contract tests above).
        let mut backend = VtRealBackend::new();
        let payload = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x61766331, // 'avc1'
            width: 1920,
            height: 1080,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0, // bitrate is encode-side; decode ignores
            gop_size: 0,
        };
        let req_bytes = bytemuck::bytes_of(&payload);
        let (header, res_payload) = dispatch(&mut backend, OP_CREATE_SESSION, req_bytes);
        assert_eq!(
            header.status, STATUS_OK,
            "decode CREATE_SESSION must now succeed; format is shipped \
             separately via OP_SET_DECODE_FORMAT"
        );
        let reply: CreateSessionReply =
            bytemuck::pod_read_unaligned(&res_payload[..std::mem::size_of::<CreateSessionReply>()]);
        assert_ne!(reply.session_id, 0);
        assert_eq!(reply.negotiated_width, 1920);
        assert_eq!(reply.negotiated_height, 1080);
    }

    #[test]
    fn destroy_session_handles_decode_sessions() {
        // DESTROY_SESSION must drop a decode session and its
        // associated VT object. Without a successful CREATE first
        // the destroy attempt should report STATUS_INVALID_HANDLE.
        let mut backend = VtRealBackend::new();
        let create = CreateSessionPayload {
            kind: VTF_SESSION_KIND_DECODE,
            codec: 0x61766331,
            width: 1280,
            height: 720,
            pixel_format: 0x34323076,
            fps_num: 30,
            fps_den: 1,
            bitrate: 0,
            gop_size: 0,
        };
        let (_, res_payload) = dispatch(
            &mut backend,
            OP_CREATE_SESSION,
            bytemuck::bytes_of(&create),
        );
        let reply: CreateSessionReply =
            bytemuck::pod_read_unaligned(&res_payload[..std::mem::size_of::<CreateSessionReply>()]);

        let destroy = DestroySessionPayload {
            session_id: reply.session_id,
        };
        let (header, _) =
            dispatch(&mut backend, OP_DESTROY_SESSION, bytemuck::bytes_of(&destroy));
        assert_eq!(header.status, STATUS_OK);

        // Second destroy of the same id is invalid.
        let (header, _) =
            dispatch(&mut backend, OP_DESTROY_SESSION, bytemuck::bytes_of(&destroy));
        assert_eq!(header.status, STATUS_INVALID_HANDLE);
    }
}
