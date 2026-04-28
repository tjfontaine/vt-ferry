pub const VTF_TRANSPORT_VERSION: u16 = 1;
pub const CAP_CODEC_H264: u64 = 1u64 << 0;
pub const CAP_CODEC_HEVC: u64 = 1u64 << 1;
pub const CAP_PIXEL_FORMAT_NV12: u64 = 1u64 << 0;
pub const CAP_PIXEL_FORMAT_BGRA: u64 = 1u64 << 1;
/// 10-bit 4:2:0 bi-planar video range — Apple's
/// `kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange` (`'x420'`,
/// FourCC `0x78343230`). HDR / Main10 input format paired with
/// `hevc_videotoolbox`. Layout matches NV12 but every sample is a
/// 16-bit word (10 used + 6 unused), so stride is `width * 2`
/// bytes and total is `stride * height * 1.5`.
pub const CAP_PIXEL_FORMAT_P010: u64 = 1u64 << 2;
pub const CAP_SESSION_FEATURE_ASYNC_COMPLETE: u64 = 1u64 << 0;
pub const CAP_SESSION_FEATURE_BUFFER_SYNC: u64 = 1u64 << 1;
/// Worker advertises a working decode path (`VTF_SESSION_KIND_DECODE`
/// + the `OP_ENQUEUE_ENCODED_FRAME` / `OP_DEQUEUE_DECODED_FRAME`
/// ops). Reserved on the bit-field; cleared until decode is
/// actually implemented. Guests probe this before issuing a decode
/// CREATE_SESSION so they can fall back to a software decoder
/// rather than racing to the worker rejection.
pub const CAP_SESSION_FEATURE_DECODE: u64 = 1u64 << 2;
pub const VTF_HOST_BACKING_KIND_UNKNOWN: u32 = 0;
pub const VTF_HOST_BACKING_KIND_IOSURFACE: u32 = 1;

/// Shared-region `source_kind` values as seen on `SharedRegionReply`.
///
/// Mirrors the kinds the host-side broker / worker use to describe how a
/// shared-region slot is backed. When the kind is
/// `VTF_SHARED_REGION_SOURCE_IOSURFACE`, `source_handle` carries the
/// 32-bit IOSurfaceID (zero-extended to u64) so the worker can recover the
/// live `IOSurfaceRef` from its `IOSurfaceRegistry` (which was populated
/// from Mach ports the launcher registered via `mach_ports_register`
/// before `posix_spawn`). Pages behind the IOSurface are already mapped
/// into the guest physical address space via libkrun's
/// `KRUN_SHARED_REGION_SOURCE_HOST_VA` path — no file, no fd, no mmap.
pub const VTF_SHARED_REGION_SOURCE_UNKNOWN: u32 = 0;
pub const VTF_SHARED_REGION_SOURCE_IOSURFACE: u32 = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MessageHeader {
    pub version: u16,
    pub opcode: u16,
    pub flags: u32,
    pub request_id: u64,
    pub payload_len: u32,
    pub status: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HelloPayload {
    pub client_abi_version: u32,
    pub reserved: u32,
    pub requested_features: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HelloReply {
    pub worker_abi_version: u32,
    pub reserved: u32,
    pub supported_features: u64,
    pub worker_name: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GetCapsReply {
    pub codec_bits: u64,
    pub pixel_format_bits: u64,
    pub session_feature_bits: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub max_inflight_frames: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CreateSessionPayload {
    pub kind: u32,
    pub codec: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub bitrate: u32,
    pub gop_size: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CreateSessionReply {
    pub session_id: u64,
    pub negotiated_width: u32,
    pub negotiated_height: u32,
    pub pixel_format: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CreateBufferPoolPayload {
    pub session_id: u64,
    pub buffer_count: u32,
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
    pub usage_flags: u32,
    pub _padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BufferLayoutReply {
    pub plane_count: u32,
    pub total_size: u32,
    pub plane_offsets: [u32; 4],
    pub plane_widths: [u32; 4],
    pub plane_heights: [u32; 4],
    pub plane_bytes_per_row: [u32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SharedRegionReply {
    pub mapping_id: u64,
    pub region_size: u64,
    pub source_offset: u64,
    pub flags: u32,
    pub source_kind: u32,
    pub source_handle: u64,
    pub source_path: [u8; 256],
}

pub const VTF_TRANSPORT_MAX_POOL_SLOTS: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PoolBufferLeaseReply {
    pub buffer_id: u64,
    pub generation: u64,
    pub slot_index: u32,
    pub slot_offset: u32,
    pub host_backing_kind: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CreateBufferPoolReply {
    pub pool_id: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub slot_count: u32,
    pub buffer_region_size: u32,
    pub host_backing_kind: u32,
    pub layout: BufferLayoutReply,
    pub shared_regions: [SharedRegionReply; VTF_TRANSPORT_MAX_POOL_SLOTS],
    pub buffer_leases: [PoolBufferLeaseReply; VTF_TRANSPORT_MAX_POOL_SLOTS],
}

pub const CREATE_BUFFER_POOL_COMPACT_REPLY_FORMAT: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CreateBufferPoolCompactReply {
    pub pool_id: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub slot_count: u32,
    pub buffer_region_size: u32,
    pub host_backing_kind: u32,
    pub format: u32,
    pub reserved: u32,
    pub layout: BufferLayoutReply,
    pub buffer_leases: [PoolBufferLeaseReply; VTF_TRANSPORT_MAX_POOL_SLOTS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AllocBufferPayload {
    pub pool_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AllocBufferReply {
    pub buffer_id: u64,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub slot_index: u32,
    pub slot_offset: u32,
    pub host_backing_kind: u32,
    pub layout: BufferLayoutReply,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadBufferPayload {
    pub buffer_id: u64,
    pub generation: u64,
    pub offset: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadBufferReply {
    pub buffer_id: u64,
    pub generation: u64,
    pub offset: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WriteBufferPayload {
    pub buffer_id: u64,
    pub generation: u64,
    pub offset: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RecycleBufferPayload {
    pub pool_id: u64,
    pub buffer_id: u64,
    pub generation: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SetPropertyPayload {
    pub session_id: u64,
    pub property_key_proxy_id: u64,
    pub property_value_proxy_id: u64,
    pub property_value_kind: u32,
    pub property_number_type: u32,
    pub property_sint64: i64,
    pub property_f64: f64,
    pub property_bool: u32,
    pub reserved: u32,
    pub property_key: [u8; 64],
    pub property_string: [u8; 64],
    pub property_array_count: u32,
    pub property_dict_pair_count: u32,
    pub property_array_i64: [i64; 2],
    pub property_dict_keys: [[u8; 32]; 2],
    pub property_dict_sint64: [i64; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PrepareSessionPayload {
    pub session_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EncodeFramePayload {
    pub session_id: u64,
    pub image_buffer_proxy_id: u64,
    pub image_buffer_host_id: u64,
    pub image_buffer_generation: u64,
    pub pts_value: i64,
    pub pts_timescale: i32,
    pub duration_timescale: i32,
    pub duration_value: i64,
}

pub const VTF_TRANSPORT_MAX_ENCODE_BATCH: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EncodeFrameBatchPayload {
    pub session_id: u64,
    pub frame_count: u32,
    pub reserved: u32,
    pub frames: [EncodeFramePayload; VTF_TRANSPORT_MAX_ENCODE_BATCH],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueOutputPayload {
    pub session_id: u64,
}

/// Maximum parameter sets carried inline in a wire payload.
///
/// H.264 emits 2 (SPS + PPS); HEVC emits 3 (VPS + SPS + PPS). One
/// extra slot for headroom (some streams legally carry multiple SPS
/// or PPS for parameter-set switching). Encode-side `DequeueOutputReply`
/// and decode-side `SetDecodeFormatPayload` share this cap so the
/// two sides see a single contract.
pub const VTF_TRANSPORT_MAX_PARAMETER_SETS: usize = 4;

/// Inline byte budget for the union of parameter-set bytes packed
/// into a wire payload. Comfortable for typical 1080p VPS/SPS/PPS
/// combined (~65 bytes) and 4K HEVC parameter sets (~80 bytes)
/// without bloating the per-payload size beyond ~256 bytes.
pub const VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES: usize = 128;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueOutputReply {
    pub session_id: u64,
    pub output_id: u64,
    pub codec: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub pts_value: i64,
    pub pts_timescale: i32,
    pub duration_timescale: i32,
    pub duration_value: i64,
    pub sample_size: u32,
    pub sample_flags: u32,
    pub parameter_set_count: u32,
    pub nal_header_length: u32,
    /// Inline parameter sets carried with the encoded output.
    ///
    /// H.264 uses 2 sets (SPS + PPS); HEVC uses 3 (VPS + SPS + PPS).
    /// We keep one extra slot for headroom — see
    /// `VTF_TRANSPORT_MAX_PARAMETER_SETS` /
    /// `VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES`.
    pub parameter_set_sizes: [u32; VTF_TRANSPORT_MAX_PARAMETER_SETS],
    pub parameter_set_data: [u8; VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES],
    pub output_slot_index: u32,
    pub output_slot_offset: u32,
    pub output_region_size: u32,
    pub reserved: u32,
}

pub const VTF_TRANSPORT_MAX_OUTPUT_BATCH: usize = 24;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueOutputBatchPayload {
    pub session_id: u64,
    pub max_outputs: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueOutputBatchReply {
    pub session_id: u64,
    pub output_count: u32,
    pub reserved: u32,
    pub outputs: [DequeueOutputReply; VTF_TRANSPORT_MAX_OUTPUT_BATCH],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadOutputPayload {
    pub output_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadOutputReply {
    pub output_id: u64,
    pub sample_size: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReleaseOutputPayload {
    pub output_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReleaseOutputBatchPayload {
    pub output_count: u32,
    pub reserved: u32,
    pub output_ids: [u64; VTF_TRANSPORT_MAX_OUTPUT_BATCH],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DestroySessionPayload {
    pub session_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrainPayload {
    pub session_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrainReply {
    pub session_id: u64,
    pub pending_outputs: u32,
    pub reserved: u32,
}

// ---------- decode-path payloads (Phase 10 scaffolding) ----------
//
// Symmetric to the encode-side ENCODE_FRAME / DEQUEUE_OUTPUT /
// RELEASE_OUTPUT trio:
//   * encode  → guest uploads pixel data, host emits compressed
//               sample buffers
//   * decode  → guest uploads compressed sample buffers, host emits
//               pixel buffers
//
// Both directions reuse the existing buffer-pool infrastructure.
// For decode, the guest creates a pool sized for encoded frames
// (variable, but bounded by `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`
// per slot) and writes the bitstream there before issuing
// ENQUEUE_ENCODED_FRAME. The worker creates an internal output pool
// for decoded `CVPixelBuffer`s, surfaced via DEQUEUE_DECODED_FRAME
// and recycled via RELEASE_DECODED_FRAME.
//
// Until the worker actually wires `VTDecompressionSessionRef`, all
// three opcodes are dispatched but return STATUS_UNSUPPORTED_OPCODE
// (preserving the contract pinned by
// `vt_real::protocol_surface_tests::reserved_decode_opcodes_are_rejected_with_unsupported_opcode`).

/// Maximum encoded-frame inline size for `OP_ENQUEUE_ENCODED_FRAME`.
/// Sized at 4 MiB to cover 4K H.264 IDR keyframes from low-preset /
/// high-bitrate encoders. The previous 256 KiB cap was set against a
/// "4K HEVC IDR is 64–128 KiB" assumption that turned out to be
/// wrong for H.264 — `libx264 -preset ultrafast` at 4K30 produces
/// IDRs in the 300+ KiB range, and real-world high-bitrate 4K H.264
/// content can exceed 2 MiB per IDR. Hitting the cap is fatal: the
/// guest shim returns -12902 before the IDR ever reaches the worker,
/// so VT never establishes reference frames and every subsequent
/// P-frame fails as `kVTVideoDecoderBadDataErr` (-12909).
pub const VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES: u32 = 4 * 1024 * 1024;

/// Sample-buffer flags carried with `EnqueueEncodedFramePayload`.
/// Mirrors the subset of `kCMSampleAttachmentKey_*` semantics the
/// host worker needs to faithfully recreate a decode-side
/// `CMSampleBuffer`.
pub const VTF_ENCODED_FRAME_FLAG_KEYFRAME: u32 = 1u64 as u32; // 0x1
pub const VTF_ENCODED_FRAME_FLAG_END_OF_STREAM: u32 = 0x2;
pub const VTF_ENCODED_FRAME_FLAG_DEPENDS_ON_OTHERS: u32 = 0x4;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EnqueueEncodedFramePayload {
    pub session_id: u64,
    /// Buffer slot in the guest-managed encoded-input pool.
    pub buffer_proxy_id: u64,
    pub buffer_host_id: u64,
    pub buffer_generation: u64,
    /// Actual encoded-byte count used in the buffer slot. May be
    /// smaller than the slot's allocated capacity (slots are sized
    /// for worst-case keyframes; P/B frames write fewer bytes).
    pub encoded_size: u32,
    pub flags: u32,
    pub pts_value: i64,
    pub pts_timescale: i32,
    pub duration_timescale: i32,
    pub duration_value: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueDecodedFramePayload {
    pub session_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DequeueDecodedFrameReply {
    pub session_id: u64,
    /// Unique handle for this decoded frame; guest passes it back via
    /// RELEASE_DECODED_FRAME when the consumer is done with the pixel
    /// buffer (so the worker can recycle the slot).
    ///
    /// `output_id == 0` is the **VT-decode-error sentinel**: VT's
    /// decompression callback fired with non-zero status (e.g.
    /// `kVTVideoDecoderBadDataErr (-12909)`), there's no real frame
    /// to fetch, and `reply.status` carries the OSStatus directly.
    /// Worker normal `output_id`s start at 70_000 so 0 is safely
    /// distinct. The guest shim treats this as an error signal,
    /// returns the OSStatus from the decode entrypoint, and skips
    /// the buffer/release machinery for this reply.
    pub output_id: u64,
    /// Buffer slot in the worker-managed decoded-output pool. Zero
    /// when the session has no bound output pool — the guest then
    /// reads the pixel bytes inline via `OP_READ_DECODED_FRAME`.
    /// Nonzero when pool-bound: `buffer_host_id` matches the slot's
    /// `VtRealBuffer::id` and the guest skips the inline read.
    pub buffer_proxy_id: u64,
    pub buffer_host_id: u64,
    pub buffer_generation: u64,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub flags: u32,
    pub pts_value: i64,
    pub pts_timescale: i32,
    pub duration_timescale: i32,
    pub duration_value: i64,
    pub status: u32,
    /// Pool slot index when pool-bound; zero on the inline path.
    /// Distinct from `buffer_host_id` because the guest indexes its
    /// per-pool slot table by `slot_index`, not the worker-side
    /// buffer id.
    pub slot_index: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReleaseDecodedFramePayload {
    pub session_id: u64,
    pub output_id: u64,
}

/// Maximum decoded-frame inline byte budget. The transport
/// response cap is 2 MiB (`VTF_TRANSPORT_STORAGE_CAPACITY` in the
/// worker's server module); we leave headroom for the reply
/// header + framing. 1.5 MiB comfortably fits 720p NV12
/// (~1.32 MiB) and most planar formats up to that resolution.
/// Larger frames need the future worker-managed output pool path
/// — the worker rejects with `STATUS_RESOURCE_EXHAUSTED` and the
/// guest is expected to fall back to a pool-bound flow.
pub const VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES: u32 = 1_500_000;

/// Read decoded pixel bytes inline. Mirrors the encode-side
/// `OP_READ_OUTPUT`: the guest first dequeues a frame's metadata
/// (`OP_DEQUEUE_DECODED_FRAME`), then reads the pixel data via
/// this op using the returned `output_id`. Reply is variable-
/// length: fixed `ReadDecodedFrameReply` header followed by
/// `sample_size` bytes of pixel data inline.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadDecodedFramePayload {
    pub session_id: u64,
    pub output_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadDecodedFrameReply {
    pub output_id: u64,
    pub sample_size: u32,
    pub status: u32,
}

/// Switches a decode session to the zero-copy chunked output path.
///
/// **History**: the original design (Phase 11) had this op bind a
/// guest-allocated buffer pool, and the worker copied decoded
/// pixels into pool slots at dequeue time. The pool path bought us
/// "lift the 1.5 MiB inline cap" but added a worker-side memcpy
/// per frame. The current op skips the pool entirely — VT's
/// internal IOSurface-backed `CVImageBuffer` IS the destination,
/// chunked reads pull bytes straight out of it.
///
/// **`pool_id` is now vestigial** and must be zero. Field is kept
/// at its original wire offset so the protocol numbering stays
/// stable across the zero-copy refactor (no
/// `VTF_TRANSPORT_VERSION` bump). Future ops can use the
/// reserved tail for genuine flags.
///
/// Reply is empty (status only).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BindDecodeOutputPoolPayload {
    pub session_id: u64,
    /// Vestigial — must be zero. Originally a buffer pool handle
    /// for the slot-copy decode path; the zero-copy chunked path
    /// reads directly out of VT's `CVImageBuffer` so no pool is
    /// involved.
    pub pool_id: u64,
    /// Reserved for future flags. Must be zero today.
    pub reserved: [u8; 16],
}

/// Reads `length` bytes at byte `offset` of a queued decoded
/// frame's pixel data. Sent repeatedly by the guest to drain a
/// frame in chunks once `OP_DEQUEUE_DECODED_FRAME` reports
/// chunked mode (nonzero `buffer_host_id` carrying the
/// `output_id`).
///
/// Worker locks the underlying `CVImageBuffer` for read, copies
/// `length` bytes from the canonical layout offset to the
/// response, unlocks. Bounds-checked against
/// `width × height × bytes_per_pixel` per `BufferLayoutReply`;
/// out-of-range reads return `STATUS_BOUNDS_VIOLATION`. The
/// `output_id` must reference a frame that's been dequeued but
/// not yet released — otherwise `STATUS_INVALID_HANDLE`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadDecodedFrameChunkPayload {
    pub session_id: u64,
    pub output_id: u64,
    pub offset: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ReadDecodedFrameChunkReply {
    pub output_id: u64,
    pub offset: u32,
    pub length: u32,
    pub status: u32,
    pub reserved: u32,
}

/// Decode-side companion to the encode reply's parameter-set
/// section. Sent by the guest after `OP_CREATE_SESSION` (with kind
/// = `VTF_SESSION_KIND_DECODE`) and before the first
/// `OP_ENQUEUE_ENCODED_FRAME`. Used both for initial format
/// configuration and for mid-stream format changes (e.g. ABR
/// ladder switches that change SPS / level).
///
/// **Why a separate op rather than payload-on-CREATE_SESSION**:
/// `VTDecompressionSession` does not sniff inline Annex-B
/// parameter sets — it requires a `CMVideoFormatDescription`
/// already populated with parameter sets at session-create time.
/// Splitting the format set into its own op keeps `CREATE_SESSION`
/// symmetric with the encode side AND lets future ABR-ladder
/// streams call `SET_DECODE_FORMAT` repeatedly without recreating
/// the session if `VTDecompressionSessionCanAcceptFormatDescription`
/// returns true. When the worker can't absorb the new format, it
/// recreates the underlying VT session transparently to the guest.
///
/// **NAL header length**: AVCC uses length-prefix framing
/// (typically 4 bytes); Annex-B uses start codes (`00 00 00 01`)
/// and reports `nal_header_length = 0`. The worker uses this to
/// build the `CMVideoFormatDescription` correctly.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SetDecodeFormatPayload {
    pub session_id: u64,
    /// Codec FourCC: `'avc1'` (0x6176_6331) or `'hvc1'` (0x6876_6331).
    pub codec: u32,
    pub width: u32,
    pub height: u32,
    /// 4 for AVCC framing; 0 for Annex-B start-code framing.
    pub nal_header_length: u32,
    /// Number of parameter sets actually populated in
    /// `parameter_set_sizes` / `parameter_set_data`. H.264 = 2,
    /// HEVC = 3. Bounded by `VTF_TRANSPORT_MAX_PARAMETER_SETS`.
    pub parameter_set_count: u32,
    /// Reserved for alignment and forward-compat (e.g. future
    /// "force recreate" or "expect ABR switch" hints).
    pub flags: u32,
    /// Per-set sizes; sizes [parameter_set_count..] must be zero.
    pub parameter_set_sizes: [u32; VTF_TRANSPORT_MAX_PARAMETER_SETS],
    /// Concatenated parameter-set bytes packed in declared order
    /// (e.g. SPS then PPS for H.264, VPS then SPS then PPS for
    /// HEVC). The worker slices this with `parameter_set_sizes`.
    pub parameter_set_data: [u8; VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES],
}

pub const OP_HELLO: u16 = 0x0001;
/// Liveness probe with empty payload. Worker replies with `STATUS_OK` and
/// an empty payload. The guest-shim issues this from a background thread
/// when the connection has been idle, so a hung-but-not-dead worker
/// surfaces as a poisoned backend rather than waiting for the next real
/// call to hit `SO_RCVTIMEO`.
pub const OP_PING: u16 = 0x0002;
pub const OP_GET_CAPS: u16 = 0x0010;
pub const OP_CREATE_SESSION: u16 = 0x0020;
pub const OP_SET_PROPERTY: u16 = 0x0022;
pub const OP_PREPARE_SESSION: u16 = 0x0023;
pub const OP_CREATE_BUFFER_POOL: u16 = 0x0030;
pub const OP_ALLOC_BUFFER: u16 = 0x0032;
pub const OP_READ_BUFFER: u16 = 0x0034;
pub const OP_WRITE_BUFFER: u16 = 0x0036;
pub const OP_RECYCLE_BUFFER: u16 = 0x0060;
pub const OP_ENCODE_FRAME: u16 = 0x0040;
pub const OP_ENCODE_FRAME_BATCH: u16 = 0x0042;
pub const OP_DEQUEUE_OUTPUT: u16 = 0x0050;
pub const OP_DEQUEUE_OUTPUT_BATCH: u16 = 0x0056;
pub const OP_READ_OUTPUT: u16 = 0x0052;
pub const OP_RELEASE_OUTPUT: u16 = 0x0054;
pub const OP_RELEASE_OUTPUT_BATCH: u16 = 0x0058;
pub const OP_DRAIN: u16 = 0x0070;
pub const OP_DESTROY_SESSION: u16 = 0x0080;

/// Decode-path opcodes (reserved; not yet implemented).
///
/// Encode flows guest→worker as `ENCODE_FRAME`/`DEQUEUE_OUTPUT`:
/// guest hands pixel buffers in, worker emits compressed sample
/// buffers out. Decode inverts the direction — guest hands
/// compressed sample buffers in via `OP_ENQUEUE_ENCODED_FRAME`,
/// worker emits decoded `CVPixelBuffer`s out via
/// `OP_DEQUEUE_DECODED_FRAME`. Numbering chosen so encode-side
/// `0x0040..0x0058` and decode-side `0x0090..0x0098` don't
/// alias; the gap to `0x0080` (DESTROY_SESSION) leaves room for
/// future encode-side additions.
///
/// The decode path is now feature-complete: VT decode runs
/// end-to-end through `VTDecompressionSession` for H.264 + HEVC,
/// 1080p inline + ≥1080p chunked output, ABR mid-stream format
/// changes, and >4 MiB chunked encoded input.
pub const OP_ENQUEUE_ENCODED_FRAME: u16 = 0x0090;
pub const OP_DEQUEUE_DECODED_FRAME: u16 = 0x0092;
pub const OP_RELEASE_DECODED_FRAME: u16 = 0x0094;
/// Configures (or reconfigures) the format description for a
/// decode session. See `SetDecodeFormatPayload` for the wire
/// contract and the rationale for a separate op.
pub const OP_SET_DECODE_FORMAT: u16 = 0x0096;
/// Reads decoded pixel bytes inline (variable-length response).
/// Mirrors `OP_READ_OUTPUT` for the encode side. Capped at
/// `VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES` (1.5 MiB / ~720p NV12);
/// larger frames use the pool-binding path via
/// `OP_BIND_DECODE_OUTPUT_POOL`.
pub const OP_READ_DECODED_FRAME: u16 = 0x0098;
/// Switches a decode session from the inline single-shot
/// `OP_READ_DECODED_FRAME` path to the zero-copy chunked path.
/// After this op fires for a given `session_id`, the worker:
///
///   - leaves decoded `CVImageBuffer`s in the queue on
///     `OP_DEQUEUE_DECODED_FRAME` (no slot acquisition, no copy)
///   - signals chunked mode in the dequeue reply by populating
///     `buffer_host_id` with the `output_id`
///   - serves pixel bytes via `OP_READ_DECODED_FRAME_CHUNK`,
///     which reads directly out of VT's IOSurface-backed
///     `CVImageBuffer` (one CPU-side memcpy per chunk, vs. the
///     two memcpys the previous slot-copy path required)
///
/// The `pool_id` field in `BindDecodeOutputPoolPayload` is now
/// vestigial — must be zero. The op name and payload are kept
/// from the original slot-copy design so the wire numbering
/// stays stable across the zero-copy refactor; the body is
/// otherwise unrelated to buffer pools.
pub const OP_BIND_DECODE_OUTPUT_POOL: u16 = 0x009A;
/// Reads a chunk of a queued decoded frame's pixel bytes
/// directly out of VT's `CVImageBuffer`. Only valid for sessions
/// that have switched to the zero-copy chunked path via
/// `OP_BIND_DECODE_OUTPUT_POOL`. Variable-length response: fixed
/// `ReadDecodedFrameChunkReply` header followed by `length`
/// bytes of pixel data.
///
/// Chunking enables decoded frames larger than the inline
/// `VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES` budget (1.5 MiB) to
/// flow through transport response budgets that cap each
/// individual response. The guest issues N chunked reads to
/// drain a frame; total bytes = `width × height × bytes_per_pixel`
/// laid out per `BufferLayoutReply`. See `ReadDecodedFrameChunkPayload`.
pub const OP_READ_DECODED_FRAME_CHUNK: u16 = 0x009C;

/// Tear down a buffer pool that the guest no longer needs.
///
/// Sent from the guest shim's `CVPixelBufferPool` finalizer when
/// the caller `CFRelease`s the pool. The worker drops its
/// `VtRealPool` record and, if the pool had claimed a launcher-
/// registered IOSurface directory entry, releases that entry so a
/// later `OP_CREATE_BUFFER_POOL` can re-claim it. Without this
/// signal a connection that creates two pools sequentially at the
/// same shape would starve the second pool — which is exactly
/// what FFmpeg's `-hwaccel videotoolbox` decoder + h264_videotoolbox
/// encoder pattern does (decode hwaccel claims the pool, then
/// VTDecompressionSessionCreate fails for unrelated reasons,
/// FFmpeg drops the pool but never tells us).
///
/// `pool_id` from `CreateBufferPoolReply::pool_id`.
/// Status:
///   STATUS_OK on success (pool removed, entry released if any)
///   STATUS_INVALID_HANDLE if the pool_id isn't known to the worker
///     (idempotent in spirit — release on an unknown id is a no-op
///     from the guest's perspective)
pub const OP_DESTROY_BUFFER_POOL: u16 = 0x009E;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DestroyBufferPoolPayload {
    pub pool_id: u64,
}

/// Chunked variant of `OP_ENQUEUE_ENCODED_FRAME` for encoded
/// frames larger than `VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES`.
/// The single-shot form's tail must fit one transport packet,
/// which caps it at 4 MiB today; this op delivers an arbitrarily-
/// large encoded frame via repeated calls, each carrying a
/// `chunk_offset`-positioned slice. The worker accumulates per
/// session and triggers VT decode when `is_final_chunk == 1`.
///
/// Wire layout: header followed by `chunk_length` bytes of
/// encoded data inline. Identical timing/flags fields to
/// `EnqueueEncodedFramePayload` so the worker can dispatch the
/// assembled frame through the same VT decode path.
///
/// Sequence rules:
///   * `chunk_offset == 0` starts a new assembly; any
///     prior in-flight assembly for the same session is
///     dropped (incomplete-previous-frame is a guest bug, but
///     the worker recovers cleanly).
///   * subsequent chunks must have `chunk_offset == previous
///     chunk_offset + previous chunk_length`; out-of-order is
///     rejected with STATUS_INVALID_STATE.
///   * `total_encoded_size` must match across all chunks of one
///     assembly; mismatched values abort with STATUS_INVALID_STATE.
///   * `is_final_chunk == 1` is allowed on any chunk (including
///     `chunk_offset == 0` for "single-chunk happens to fit");
///     when seen, the worker validates `chunk_offset +
///     chunk_length == total_encoded_size` then dispatches decode.
///   * `pts_*` and `duration_*` are only honored on the chunk
///     with `chunk_offset == 0` (the head). Subsequent chunks'
///     timing fields are ignored.
pub const OP_ENQUEUE_ENCODED_FRAME_CHUNK: u16 = 0x00A0;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EnqueueEncodedFrameChunkPayload {
    pub session_id: u64,
    pub pts_value: i64,
    pub duration_value: i64,
    /// Byte offset of this chunk within the encoded frame. The
    /// first chunk has `chunk_offset == 0`; subsequent chunks must
    /// be contiguous (no gaps, no overlap).
    pub chunk_offset: u32,
    /// Bytes of encoded data that follow this header inline.
    pub chunk_length: u32,
    /// Total encoded frame size. Replicated on every chunk for
    /// drift detection; the worker rejects mismatched values.
    pub total_encoded_size: u32,
    /// Non-zero on the last chunk of the assembly. Triggers VT
    /// decode and clears the per-session in-flight buffer.
    pub is_final_chunk: u32,
    /// Mirrors `EnqueueEncodedFramePayload::flags`. Only the
    /// head chunk's value is honored.
    pub flags: u32,
    pub pts_timescale: i32,
    pub duration_timescale: i32,
    /// Reserved for future use. Currently must be zero. Present
    /// to keep the struct's size a multiple of the i64 alignment
    /// so `bytemuck::Pod` can derive without padding.
    pub _reserved: u32,
}

pub const STATUS_OK: u32 = 0;
pub const STATUS_UNSUPPORTED_OPCODE: u32 = 1;
pub const STATUS_UNSUPPORTED_VERSION: u32 = 2;
pub const STATUS_UNSUPPORTED_CODEC_OR_FORMAT: u32 = 3;
pub const STATUS_INVALID_HANDLE: u32 = 4;
pub const STATUS_STALE_GENERATION: u32 = 5;
pub const STATUS_INVALID_STATE: u32 = 6;
pub const STATUS_BOUNDS_VIOLATION: u32 = 7;
pub const STATUS_INTERNAL_FAILURE: u32 = 8;
pub const STATUS_TIMEOUT: u32 = 9;
pub const STATUS_RESOURCE_EXHAUSTED: u32 = 10;
pub const STATUS_PROPERTY_NOT_SUPPORTED: u32 = 11;
/// Session kind (`CreateSessionPayload::kind`) values. Only encode is
/// implemented today; decode is reserved so the worker can give a
/// distinct rejection that's separable from "unsupported codec" or
/// "invalid payload" — useful both for guests that opportunistically
/// probe decode support and for telemetry that wants to count
/// not-yet-implemented requests separately from genuine misuse.
pub const VTF_SESSION_KIND_ENCODE: u32 = 1;
pub const VTF_SESSION_KIND_DECODE: u32 = 2;

#[cfg(test)]
mod wire_contract_tests {
    //! Pin the protocol's wire shape. These structs cross the
    //! transport between guest and host worker; a silent layout or
    //! constant change would break ABI compatibility with already-
    //! deployed clients. Tests run in microseconds and catch the
    //! kind of "added a field, forgot to bump the size" bug that
    //! would otherwise only surface at deserialize-time on a real
    //! VM.
    use super::*;
    use bytemuck::Zeroable;
    use std::mem::size_of;

    /// Wire-size pins for the headers and replies that clients
    /// parse first — `MessageHeader` decides framing, `HelloReply`
    /// gates ABI negotiation, `GetCapsReply` drives capability
    /// probing. A change here MUST be a deliberate version bump.
    #[test]
    fn message_header_size_is_stable() {
        // u16 + u16 + u32 + u64 + u32 + u32 = 24 bytes
        assert_eq!(size_of::<MessageHeader>(), 24);
    }

    #[test]
    fn hello_payload_and_reply_are_stable() {
        // HelloPayload: u32 + u32 + u64 = 16 bytes
        assert_eq!(size_of::<HelloPayload>(), 16);
        // HelloReply: u32 + u32 + u64 + [u8; 32] = 48 bytes
        assert_eq!(size_of::<HelloReply>(), 48);
    }

    #[test]
    fn get_caps_reply_is_stable() {
        // 3 × u64 + 4 × u32 = 40 bytes
        assert_eq!(size_of::<GetCapsReply>(), 40);
    }

    #[test]
    fn buffer_layout_reply_pins_at_72_bytes() {
        // u32 plane_count + u32 total_size + 4×u32 offsets +
        // 4×u32 widths + 4×u32 heights + 4×u32 strides
        // = (1 + 1 + 4 + 4 + 4 + 4) × 4 = 72 bytes (guest reads
        // via pod_read_unaligned; layout drift would skew chroma
        // offsets silently).
        assert_eq!(size_of::<BufferLayoutReply>(), 72);
    }

    /// Pixel-format capability bits must not collide. New formats
    /// add a new shift; this test catches typos like "1u64 << 1"
    /// being reused.
    #[test]
    fn pixel_format_caps_are_disjoint() {
        let bits = [
            CAP_PIXEL_FORMAT_NV12,
            CAP_PIXEL_FORMAT_BGRA,
            CAP_PIXEL_FORMAT_P010,
        ];
        for (i, a) in bits.iter().enumerate() {
            assert!(*a != 0, "pixel format cap {i} is zero");
            assert!(
                a.is_power_of_two(),
                "pixel format cap {i} ({:#x}) must be a single bit",
                a
            );
            for (j, b) in bits.iter().enumerate().skip(i + 1) {
                assert_eq!(
                    a & b,
                    0,
                    "pixel format caps {i} and {j} overlap: {:#x} & {:#x}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn codec_caps_are_disjoint() {
        assert!(CAP_CODEC_H264.is_power_of_two());
        assert!(CAP_CODEC_HEVC.is_power_of_two());
        assert_eq!(CAP_CODEC_H264 & CAP_CODEC_HEVC, 0);
    }

    #[test]
    fn session_feature_caps_are_disjoint() {
        let bits = [
            CAP_SESSION_FEATURE_ASYNC_COMPLETE,
            CAP_SESSION_FEATURE_BUFFER_SYNC,
            CAP_SESSION_FEATURE_DECODE,
        ];
        for (i, a) in bits.iter().enumerate() {
            assert!(a.is_power_of_two(), "feature cap {i} not a single bit");
            for (j, b) in bits.iter().enumerate().skip(i + 1) {
                assert_eq!(a & b, 0, "feature caps {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn status_codes_are_unique() {
        let codes = [
            STATUS_OK,
            STATUS_UNSUPPORTED_OPCODE,
            STATUS_UNSUPPORTED_VERSION,
            STATUS_UNSUPPORTED_CODEC_OR_FORMAT,
            STATUS_INVALID_HANDLE,
            STATUS_STALE_GENERATION,
            STATUS_INVALID_STATE,
            STATUS_BOUNDS_VIOLATION,
            STATUS_INTERNAL_FAILURE,
            STATUS_TIMEOUT,
            STATUS_RESOURCE_EXHAUSTED,
            STATUS_PROPERTY_NOT_SUPPORTED,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            codes.len(),
            "status codes must be unique"
        );
    }

    #[test]
    fn opcodes_are_unique_across_encode_and_decode() {
        // Encode-side opcodes plus the reserved decode-side range.
        // Decode opcodes sit in 0x0090+ specifically to avoid
        // colliding with future encode-side additions in the gap
        // before 0x0080 (DESTROY_SESSION).
        let opcodes = [
            OP_HELLO,
            OP_PING,
            OP_GET_CAPS,
            OP_CREATE_SESSION,
            OP_SET_PROPERTY,
            OP_PREPARE_SESSION,
            OP_CREATE_BUFFER_POOL,
            OP_ALLOC_BUFFER,
            OP_READ_BUFFER,
            OP_WRITE_BUFFER,
            OP_RECYCLE_BUFFER,
            OP_ENCODE_FRAME,
            OP_ENCODE_FRAME_BATCH,
            OP_DEQUEUE_OUTPUT,
            OP_DEQUEUE_OUTPUT_BATCH,
            OP_READ_OUTPUT,
            OP_RELEASE_OUTPUT,
            OP_RELEASE_OUTPUT_BATCH,
            OP_DRAIN,
            OP_DESTROY_SESSION,
            OP_ENQUEUE_ENCODED_FRAME,
            OP_DEQUEUE_DECODED_FRAME,
            OP_RELEASE_DECODED_FRAME,
            OP_SET_DECODE_FORMAT,
            OP_READ_DECODED_FRAME,
            OP_BIND_DECODE_OUTPUT_POOL,
            OP_READ_DECODED_FRAME_CHUNK,
        ];
        let mut sorted = opcodes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            opcodes.len(),
            "opcodes must be unique across encode + decode reservations"
        );
    }

    #[test]
    fn decode_opcodes_sit_in_0x0090_range() {
        // The reservation comment promises a specific numbering:
        // decode-side ops live in 0x0090+ so encode-side can grow
        // into 0x0060–0x008f without collision.
        let decode_ops = [
            OP_ENQUEUE_ENCODED_FRAME,
            OP_DEQUEUE_DECODED_FRAME,
            OP_RELEASE_DECODED_FRAME,
            OP_SET_DECODE_FORMAT,
            OP_READ_DECODED_FRAME,
            OP_BIND_DECODE_OUTPUT_POOL,
            OP_READ_DECODED_FRAME_CHUNK,
        ];
        for op in decode_ops {
            assert!(op >= 0x0090, "decode op {:#06x} below range floor", op);
            assert!(op < 0x00a0, "decode op {:#06x} above range ceiling", op);
        }
    }

    #[test]
    fn read_decoded_frame_payload_and_reply_are_stable() {
        // Payload: session_id + output_id = 16 bytes
        assert_eq!(size_of::<ReadDecodedFramePayload>(), 16);
        // Reply: output_id + sample_size + status = 16 bytes header.
        // Pixel data follows variable-length after the header.
        assert_eq!(size_of::<ReadDecodedFrameReply>(), 16);
    }

    #[test]
    fn bind_decode_output_pool_payload_is_stable() {
        // session_id (8) + pool_id (8) + reserved (16) = 32 bytes.
        // The reserved tail keeps room for future flags without
        // bumping payload size or risking unaligned reads.
        assert_eq!(size_of::<BindDecodeOutputPoolPayload>(), 32);
    }

    #[test]
    fn read_decoded_frame_chunk_payload_and_reply_are_stable() {
        // Payload: session_id (8) + output_id (8) + offset (4) +
        //          length (4) = 24 bytes
        assert_eq!(size_of::<ReadDecodedFrameChunkPayload>(), 24);
        // Reply header: output_id (8) + offset (4) + length (4) +
        //               status (4) + reserved (4) = 24 bytes.
        // Pixel chunk follows variable-length after the header.
        assert_eq!(size_of::<ReadDecodedFrameChunkReply>(), 24);
    }

    #[test]
    fn max_decoded_frame_bytes_fits_720p_nv12() {
        // 720p NV12: 1280 × 720 × 1.5 = 1_382_400 bytes. Cap must
        // accommodate that with headroom for stride alignment.
        assert!(VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES >= 1_400_000);
    }

    #[test]
    fn session_kind_values_are_unique_and_nonzero() {
        // Zero is reserved for "unset / no kind" so the worker can
        // distinguish a default-initialized payload from an
        // explicit kind selection.
        assert_ne!(VTF_SESSION_KIND_ENCODE, 0);
        assert_ne!(VTF_SESSION_KIND_DECODE, 0);
        assert_ne!(VTF_SESSION_KIND_ENCODE, VTF_SESSION_KIND_DECODE);
    }

    #[test]
    fn transport_version_is_nonzero() {
        // Version 0 would conflict with default-zeroed headers
        // and make "unsupported version" rejection ambiguous.
        assert_ne!(VTF_TRANSPORT_VERSION, 0);
    }

    /// Decode-path payloads (Phase 10 scaffolding). Sizes pinned so
    /// any field addition during decode bring-up has to update the
    /// expected size here, forcing a deliberate review.
    #[test]
    fn enqueue_encoded_frame_payload_is_stable() {
        // session_id (8) + 3 × u64 buffer (24) + 2 × u32 (8) +
        // pts_value (8) + 2 × i32 timescales (8) +
        // duration_value (8) = 64 bytes
        assert_eq!(size_of::<EnqueueEncodedFramePayload>(), 64);
    }

    #[test]
    fn enqueue_encoded_frame_chunk_payload_is_stable() {
        // session_id (8) + pts_value (8) + duration_value (8)
        // + 5 × u32 (chunk_offset/length/total/is_final/flags = 20)
        // + 2 × i32 timescales (8) + _reserved (4) = 56 bytes
        // (the explicit _reserved keeps the struct's size a
        // multiple of i64 alignment so bytemuck::Pod accepts it
        // without padding)
        assert_eq!(size_of::<EnqueueEncodedFrameChunkPayload>(), 56);
    }

    #[test]
    fn dequeue_decoded_frame_payload_and_reply_are_stable() {
        assert_eq!(size_of::<DequeueDecodedFramePayload>(), 8);
        // session_id + output_id (16) + 3 × u64 buffer (24) +
        // 4 × u32 width/height/pixel_format/flags (16) +
        // pts_value (8) + 2 × i32 timescales (8) +
        // duration_value (8) + status + reserved (8) = 88 bytes
        assert_eq!(size_of::<DequeueDecodedFrameReply>(), 88);
    }

    #[test]
    fn release_decoded_frame_payload_is_stable() {
        // session_id + output_id = 16 bytes
        assert_eq!(size_of::<ReleaseDecodedFramePayload>(), 16);
    }

    #[test]
    fn set_decode_format_payload_is_stable() {
        // session_id (8) + 6 × u32 (24) + sizes [u32; 4] (16) +
        // data [u8; 128] (128) = 176 bytes
        assert_eq!(size_of::<SetDecodeFormatPayload>(), 176);
        // Mirrors the encode-side reply's parameter_set_data
        // capacity so the two sides see one shape.
        let payload = SetDecodeFormatPayload::zeroed();
        assert_eq!(
            payload.parameter_set_data.len(),
            VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES
        );
        assert_eq!(
            payload.parameter_set_sizes.len(),
            VTF_TRANSPORT_MAX_PARAMETER_SETS
        );
    }

    #[test]
    fn parameter_set_caps_match_dequeue_output_reply_shape() {
        // The encode-side DequeueOutputReply uses exactly these
        // capacities; if the constants drift, encode and decode
        // will disagree on the parameter-set frame contract.
        let reply = DequeueOutputReply::zeroed();
        assert_eq!(
            reply.parameter_set_sizes.len(),
            VTF_TRANSPORT_MAX_PARAMETER_SETS
        );
        assert_eq!(
            reply.parameter_set_data.len(),
            VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES
        );
    }

    #[test]
    fn parameter_set_caps_fit_hevc_idr_parameter_sets() {
        // HEVC at 4K typically emits VPS (~30 B) + SPS (~50 B) +
        // PPS (~10 B) ≈ 90 B. The 128-byte cap leaves headroom
        // for HDR / Main10 metadata in the SPS.
        assert!(VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES >= 96);
        // 4 slots accommodates HEVC's 3 sets plus one for headroom
        // (multiple SPS per stream is legal).
        assert!(VTF_TRANSPORT_MAX_PARAMETER_SETS >= 3);
    }

    #[test]
    fn encoded_frame_flags_are_disjoint_single_bits() {
        let bits = [
            VTF_ENCODED_FRAME_FLAG_KEYFRAME,
            VTF_ENCODED_FRAME_FLAG_END_OF_STREAM,
            VTF_ENCODED_FRAME_FLAG_DEPENDS_ON_OTHERS,
        ];
        for (i, a) in bits.iter().enumerate() {
            assert!(a.is_power_of_two(), "flag {i} ({:#x}) not a single bit", a);
            for (j, b) in bits.iter().enumerate().skip(i + 1) {
                assert_eq!(a & b, 0, "flags {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn max_encoded_frame_bytes_fits_4k_h264_idr() {
        // Floor: 4K H.264 IDRs from libx264 -preset ultrafast hit
        // ~300 KiB on the synthetic test clip; high-bitrate
        // real-world 4K can exceed 2 MiB. Cap must exceed both.
        // Hitting the cap silently strands every subsequent P-frame
        // as kVTVideoDecoderBadDataErr (no reference frames).
        assert!(VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES >= 3 * 1024 * 1024);
    }
}
