use crate::backend::Backend;
use bytemuck::{Zeroable, bytes_of, pod_read_unaligned};
use vt_ferry_protocol::*;

const MAX_SESSIONS: usize = 32;
const MAX_POOLS: usize = 64;
const MAX_BUFFERS: usize = 256;
const MAX_OUTPUTS: usize = 128;
const DEFAULT_MAX_OUTPUT_BYTES: usize = MAX_OUTPUTS * 512;
const DEFAULT_POOL_BUFFERS: u32 = 4;
const ENCODE_OUTPUT_FLAG_SYNC: u32 = 1 << 0;
const BUFFER_STATE_GUEST_WRITABLE: u32 = 0;
const BUFFER_STATE_QUEUED_TO_HOST: u32 = 1;
const BUFFER_STATE_RECYCLED: u32 = 2;

// `kind` is recorded at session-create time but the mock dispatch
// only branches on opcode, not on the session-kind field.
#[allow(dead_code)]
struct MockSession {
    id: u64,
    kind: u32,
    codec: u32,
    width: u32,
    height: u32,
    pixel_format: u32,
    prepared: bool,
    frames_encoded: u64,
}

struct MockPool {
    id: u64,
    session_id: u64,
    width: u32,
    height: u32,
    pixel_format: u32,
    max_buffers: u32,
    buffer_count: u32,
    slot_region_size: u32,
    backing: Vec<u8>,
}

struct MockBuffer {
    id: u64,
    pool_id: u64,
    generation: u64,
    slot_index: u32,
    width: u32,
    height: u32,
    pixel_format: u32,
    state: u32,
    total_size: usize,
}

struct MockOutput {
    reply: DequeueOutputReply,
    source_buffer_id: u64,
    source_buffer_generation: u64,
    sample_data: Vec<u8>,
    dequeued: bool,
}

pub struct MockBackend {
    next_host_id: u64,
    next_output_id: u64,
    max_output_count: usize,
    max_output_bytes: usize,
    output_bytes_total: usize,
    sessions: Vec<MockSession>,
    pools: Vec<MockPool>,
    buffers: Vec<MockBuffer>,
    outputs: Vec<MockOutput>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            next_host_id: 1000,
            next_output_id: 5000,
            max_output_count: MAX_OUTPUTS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            output_bytes_total: 0,
            sessions: Vec::new(),
            pools: Vec::new(),
            buffers: Vec::new(),
            outputs: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.next_host_id = 1000;
        self.next_output_id = 5000;
        self.max_output_count = MAX_OUTPUTS;
        self.max_output_bytes = DEFAULT_MAX_OUTPUT_BYTES;
        self.output_bytes_total = 0;
        self.sessions.clear();
        self.pools.clear();
        self.buffers.clear();
        self.outputs.clear();
    }

    fn find_session_idx(&self, session_id: u64) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.id == session_id)
    }

    fn find_pool_idx(&self, pool_id: u64) -> Option<usize> {
        self.pools.iter().position(|pool| pool.id == pool_id)
    }

    fn find_buffer_idx(&self, buffer_id: u64) -> Option<usize> {
        self.buffers
            .iter()
            .position(|buffer| buffer.id == buffer_id)
    }

    fn find_recyclable_buffer_idx(&self, pool_id: u64) -> Option<usize> {
        self.buffers
            .iter()
            .position(|buffer| buffer.pool_id == pool_id && buffer.state == BUFFER_STATE_RECYCLED)
    }

    fn count_outputs_for_session(&self, session_id: u64) -> u32 {
        self.outputs
            .iter()
            .filter(|output| output.reply.session_id == session_id)
            .count() as u32
    }

    fn dequeue_output_idx(&mut self, session_id: u64) -> Option<usize> {
        let idx = self
            .outputs
            .iter()
            .position(|output| output.reply.session_id == session_id && !output.dequeued)?;
        self.outputs[idx].dequeued = true;
        Some(idx)
    }

    fn take_output_idx(&self, output_id: u64) -> Option<usize> {
        self.outputs
            .iter()
            .position(|output| output.reply.output_id == output_id && output.dequeued)
    }

    fn release_output(&mut self, output_id: u64) -> bool {
        let Some(index) = self
            .outputs
            .iter()
            .position(|output| output.reply.output_id == output_id)
        else {
            return false;
        };

        let source_buffer_id = self.outputs[index].source_buffer_id;
        let source_buffer_generation = self.outputs[index].source_buffer_generation;
        if let Some(buffer_idx) = self.find_buffer_idx(source_buffer_id) {
            let buffer = &mut self.buffers[buffer_idx];
            if buffer.generation == source_buffer_generation
                && buffer.state == BUFFER_STATE_QUEUED_TO_HOST
            {
                buffer.state = BUFFER_STATE_GUEST_WRITABLE;
            }
        }

        let output = self.outputs.remove(index);
        self.output_bytes_total = self
            .output_bytes_total
            .saturating_sub(output.reply.sample_size as usize);
        true
    }

    fn remove_session(&mut self, session_id: u64) {
        self.sessions.retain(|session| session.id != session_id);
    }

    fn remove_buffers_for_pool(&mut self, pool_id: u64) {
        self.buffers.retain(|buffer| buffer.pool_id != pool_id);
    }

    fn remove_pools_for_session(&mut self, session_id: u64) {
        let pool_ids: Vec<u64> = self
            .pools
            .iter()
            .filter(|pool| pool.session_id == session_id)
            .map(|pool| pool.id)
            .collect();
        self.pools.retain(|pool| pool.session_id != session_id);
        for pool_id in pool_ids {
            self.remove_buffers_for_pool(pool_id);
        }
    }

    fn remove_outputs_for_session(&mut self, session_id: u64) {
        let removed_bytes: usize = self
            .outputs
            .iter()
            .filter(|output| output.reply.session_id == session_id)
            .map(|output| output.reply.sample_size as usize)
            .sum();
        self.outputs
            .retain(|output| output.reply.session_id != session_id);
        self.output_bytes_total = self.output_bytes_total.saturating_sub(removed_bytes);
    }

    fn recycle_buffer(
        &mut self,
        payload: &RecycleBufferPayload,
        res_header: &mut MessageHeader,
    ) -> bool {
        let Some(pool_idx) = self.find_pool_idx(payload.pool_id) else {
            res_header.status = STATUS_INVALID_HANDLE;
            return false;
        };
        let Some(buffer_idx) = self.find_buffer_idx(payload.buffer_id) else {
            res_header.status = STATUS_INVALID_HANDLE;
            return false;
        };

        let buffer = &mut self.buffers[buffer_idx];
        if self.pools[pool_idx].id != buffer.pool_id {
            res_header.status = STATUS_INVALID_HANDLE;
            return false;
        }
        if buffer.generation != payload.generation {
            res_header.status = STATUS_STALE_GENERATION;
            return false;
        }
        if buffer.state == BUFFER_STATE_RECYCLED {
            res_header.status = STATUS_INVALID_STATE;
            return false;
        }
        buffer.state = BUFFER_STATE_RECYCLED;
        true
    }

    fn slot_bytes_mut(&mut self, pool_idx: usize, slot_index: u32) -> Option<&mut [u8]> {
        let pool = &mut self.pools[pool_idx];
        slot_range(slot_index, pool.slot_region_size, pool.backing.len())
            .map(|range| &mut pool.backing[range])
    }

    fn slot_bytes(&self, pool_idx: usize, slot_index: u32) -> Option<&[u8]> {
        let pool = &self.pools[pool_idx];
        slot_range(slot_index, pool.slot_region_size, pool.backing.len())
            .map(|range| &pool.backing[range])
    }
}

fn slot_range(
    slot_index: u32,
    slot_region_size: u32,
    backing_len: usize,
) -> Option<std::ops::Range<usize>> {
    if slot_region_size == 0 {
        return None;
    }
    let start = (slot_index as usize).checked_mul(slot_region_size as usize)?;
    let end = start.checked_add(slot_region_size as usize)?;
    if end > backing_len {
        return None;
    }
    Some(start..end)
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for MockBackend {
    fn reset_from_env(&mut self) {
        self.reset();
        self.max_output_count =
            parse_size_env("VT_FERRY_MOCK_MAX_OUTPUTS", MAX_OUTPUTS).min(MAX_OUTPUTS);
        self.max_output_bytes =
            parse_size_env("VT_FERRY_MOCK_MAX_OUTPUT_BYTES", DEFAULT_MAX_OUTPUT_BYTES);
    }

    fn dispatch(
        &mut self,
        req_header: &MessageHeader,
        req_payload: &[u8],
        res_header: &mut MessageHeader,
        res_payload: &mut [u8],
    ) -> Result<usize, ()> {
        *res_header = MessageHeader::zeroed();

        if req_header.version != VTF_TRANSPORT_VERSION {
            res_header.status = STATUS_UNSUPPORTED_VERSION;
            return Err(());
        }

        match req_header.opcode {
            OP_HELLO => {
                let mut reply = HelloReply::zeroed();
                reply.worker_abi_version = VTF_TRANSPORT_VERSION as u32;
                set_worker_name(&mut reply.worker_name, b"vt-ferry-host-worker\0");
                write_pod(res_payload, &reply, res_header)
            }
            OP_PING => Ok(0),
            OP_GET_CAPS => {
                let mut reply = GetCapsReply::zeroed();
                reply.codec_bits = CAP_CODEC_H264 | CAP_CODEC_HEVC;
                reply.pixel_format_bits =
                    CAP_PIXEL_FORMAT_NV12 | CAP_PIXEL_FORMAT_BGRA | CAP_PIXEL_FORMAT_P010;
                reply.session_feature_bits =
                    CAP_SESSION_FEATURE_ASYNC_COMPLETE | CAP_SESSION_FEATURE_BUFFER_SYNC;
                reply.max_width = 7680;
                reply.max_height = 4320;
                reply.max_inflight_frames = self.max_output_count as u32;
                write_pod(res_payload, &reply, res_header)
            }
            OP_CREATE_SESSION => {
                if self.sessions.len() >= MAX_SESSIONS {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                }
                let Some(payload) = read_pod::<CreateSessionPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };

                let session = MockSession {
                    id: self.next_host_id,
                    kind: payload.kind,
                    codec: payload.codec,
                    width: payload.width,
                    height: payload.height,
                    pixel_format: payload.pixel_format,
                    prepared: false,
                    frames_encoded: 0,
                };
                self.next_host_id += 1;
                self.sessions.push(session);

                let mut reply = CreateSessionReply::zeroed();
                reply.session_id = self.sessions.last().unwrap().id;
                reply.negotiated_width = payload.width;
                reply.negotiated_height = payload.height;
                reply.pixel_format = payload.pixel_format;
                write_pod(res_payload, &reply, res_header)
            }
            OP_CREATE_BUFFER_POOL => {
                if self.pools.len() >= MAX_POOLS {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                }
                let Some(payload) = read_pod::<CreateBufferPoolPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if payload.session_id != 0 && self.find_session_idx(payload.session_id).is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                }

                let mut layout = BufferLayoutReply::zeroed();
                fill_buffer_layout(
                    payload.width,
                    payload.height,
                    payload.pixel_format,
                    &mut layout,
                );
                let slot_region_size = layout.total_size;
                let max_buffers = if payload.buffer_count == 0 {
                    DEFAULT_POOL_BUFFERS
                } else {
                    payload.buffer_count
                };

                let backing_size = (max_buffers as usize)
                    .checked_mul(slot_region_size as usize)
                    .ok_or(())?;
                let backing = vec![0u8; backing_size];

                let pool = MockPool {
                    id: self.next_host_id,
                    session_id: payload.session_id,
                    width: payload.width,
                    height: payload.height,
                    pixel_format: payload.pixel_format,
                    max_buffers,
                    buffer_count: 0,
                    slot_region_size,
                    backing,
                };
                self.next_host_id += 1;
                self.pools.push(pool);
                let pool_idx = self.pools.len() - 1;

                let mut reply = CreateBufferPoolReply::zeroed();
                reply.pool_id = self.pools[pool_idx].id;
                reply.width = payload.width;
                reply.height = payload.height;
                reply.pixel_format = payload.pixel_format;
                reply.slot_count = max_buffers;
                reply.buffer_region_size = max_buffers * layout.total_size;
                reply.layout = layout;
                // Mock backend keeps pool backing in-process; the guest must
                // use READ_BUFFER / WRITE_BUFFER (the shared_regions array
                // stays zeroed because no mappable handle is exposed).
                write_pod(res_payload, &reply, res_header)
            }
            OP_ALLOC_BUFFER => {
                let Some(payload) = read_pod::<AllocBufferPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(pool_idx) = self.find_pool_idx(payload.pool_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };

                if let Some(buffer_idx) = self.find_recyclable_buffer_idx(payload.pool_id) {
                    let (buffer_id, generation, width, height, pixel_format, slot_index) = {
                        let buffer = &mut self.buffers[buffer_idx];
                        buffer.generation += 1;
                        buffer.state = BUFFER_STATE_GUEST_WRITABLE;
                        (
                            buffer.id,
                            buffer.generation,
                            buffer.width,
                            buffer.height,
                            buffer.pixel_format,
                            buffer.slot_index,
                        )
                    };

                    let Some(bytes) = self.slot_bytes_mut(pool_idx, slot_index) else {
                        res_header.status = STATUS_INTERNAL_FAILURE;
                        return Err(());
                    };
                    bytes.fill(0);

                    let mut reply = AllocBufferReply::zeroed();
                    fill_buffer_layout(width, height, pixel_format, &mut reply.layout);
                    reply.buffer_id = buffer_id;
                    reply.generation = generation;
                    reply.width = width;
                    reply.height = height;
                    reply.pixel_format = pixel_format;
                    reply.slot_index = slot_index;
                    reply.slot_offset = slot_index * reply.layout.total_size;
                    return write_pod(res_payload, &reply, res_header);
                }

                if self.pools[pool_idx].buffer_count >= self.pools[pool_idx].max_buffers
                    || self.buffers.len() >= MAX_BUFFERS
                {
                    res_header.status = STATUS_RESOURCE_EXHAUSTED;
                    return Err(());
                }

                let slot_index = self.pools[pool_idx].buffer_count;
                let total_size = self.pools[pool_idx].slot_region_size as usize;
                let Some(bytes) = self.slot_bytes_mut(pool_idx, slot_index) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                bytes.fill(0);

                let buffer = MockBuffer {
                    id: self.next_host_id,
                    pool_id: self.pools[pool_idx].id,
                    generation: 1,
                    slot_index,
                    width: self.pools[pool_idx].width,
                    height: self.pools[pool_idx].height,
                    pixel_format: self.pools[pool_idx].pixel_format,
                    state: BUFFER_STATE_GUEST_WRITABLE,
                    total_size,
                };
                self.next_host_id += 1;
                self.pools[pool_idx].buffer_count += 1;
                self.buffers.push(buffer);

                let mut reply = AllocBufferReply::zeroed();
                let buffer = self.buffers.last().unwrap();
                fill_buffer_layout(
                    buffer.width,
                    buffer.height,
                    buffer.pixel_format,
                    &mut reply.layout,
                );
                reply.buffer_id = buffer.id;
                reply.generation = buffer.generation;
                reply.width = buffer.width;
                reply.height = buffer.height;
                reply.pixel_format = buffer.pixel_format;
                reply.slot_index = buffer.slot_index;
                reply.slot_offset = buffer.slot_index * reply.layout.total_size;
                write_pod(res_payload, &reply, res_header)
            }
            OP_READ_BUFFER => {
                let Some(payload) = read_pod::<ReadBufferPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(buffer_idx) = self.find_buffer_idx(payload.buffer_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let buffer = &self.buffers[buffer_idx];
                if buffer.generation != payload.generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Err(());
                }
                if buffer.state == BUFFER_STATE_RECYCLED {
                    res_header.status = STATUS_INVALID_STATE;
                    return Err(());
                }
                if payload.offset as usize > buffer.total_size
                    || payload.length as usize > buffer.total_size - payload.offset as usize
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Err(());
                }
                let Some(pool_idx) = self.find_pool_idx(buffer.pool_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let Some(bytes) = self.slot_bytes(pool_idx, buffer.slot_index) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };

                let mut reply = ReadBufferReply::zeroed();
                reply.buffer_id = buffer.id;
                reply.generation = buffer.generation;
                reply.offset = payload.offset;
                reply.length = payload.length;
                write_pod_and_bytes(
                    res_payload,
                    &reply,
                    &bytes[payload.offset as usize
                        ..payload.offset as usize + payload.length as usize],
                    res_header,
                )
            }
            OP_WRITE_BUFFER => {
                if req_payload.len() < std::mem::size_of::<WriteBufferPayload>() {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Err(());
                }
                let payload: WriteBufferPayload =
                    pod_read_unaligned(&req_payload[..std::mem::size_of::<WriteBufferPayload>()]);
                let Some(buffer_idx) = self.find_buffer_idx(payload.buffer_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let buffer = &self.buffers[buffer_idx];
                if buffer.generation != payload.generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Err(());
                }
                if buffer.state != BUFFER_STATE_GUEST_WRITABLE {
                    res_header.status = STATUS_INVALID_STATE;
                    return Err(());
                }
                if payload.offset as usize > buffer.total_size
                    || payload.length as usize > buffer.total_size - payload.offset as usize
                    || req_payload.len()
                        != std::mem::size_of::<WriteBufferPayload>() + payload.length as usize
                {
                    res_header.status = STATUS_BOUNDS_VIOLATION;
                    return Err(());
                }
                let Some(pool_idx) = self.find_pool_idx(buffer.pool_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let Some(bytes) = self.slot_bytes_mut(pool_idx, buffer.slot_index) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let payload_bytes = &req_payload[std::mem::size_of::<WriteBufferPayload>()..];
                bytes[payload.offset as usize..payload.offset as usize + payload.length as usize]
                    .copy_from_slice(payload_bytes);
                Ok(0)
            }
            OP_RECYCLE_BUFFER => {
                let Some(payload) = read_pod::<RecycleBufferPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if self.recycle_buffer(&payload, res_header) {
                    Ok(0)
                } else {
                    Err(())
                }
            }
            OP_SET_PROPERTY => {
                let Some(payload) = read_pod::<SetPropertyPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if self.find_session_idx(payload.session_id).is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                }
                Ok(0)
            }
            OP_PREPARE_SESSION => {
                let Some(payload) = read_pod::<PrepareSessionPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(session_idx) = self.find_session_idx(payload.session_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                self.sessions[session_idx].prepared = true;
                Ok(0)
            }
            OP_ENCODE_FRAME => {
                let Some(payload) = read_pod::<EncodeFramePayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(session_idx) = self.find_session_idx(payload.session_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                if !self.sessions[session_idx].prepared {
                    res_header.status = STATUS_INVALID_STATE;
                    return Err(());
                }
                if payload.image_buffer_proxy_id == 0 {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                }
                let Some(buffer_idx) = self.find_buffer_idx(payload.image_buffer_host_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let buffer = &self.buffers[buffer_idx];
                if buffer.generation != payload.image_buffer_generation {
                    res_header.status = STATUS_STALE_GENERATION;
                    return Err(());
                }
                if buffer.state == BUFFER_STATE_RECYCLED
                    || buffer.state != BUFFER_STATE_GUEST_WRITABLE
                {
                    res_header.status = STATUS_INVALID_STATE;
                    return Err(());
                }

                let Some(pool_idx) = self.find_pool_idx(buffer.pool_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let Some(bytes) = self.slot_bytes(pool_idx, buffer.slot_index) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let input_prefix_bytes = {
                    let input_prefix = 8.min(buffer.total_size);
                    bytes[..input_prefix].to_vec()
                };

                let session = &mut self.sessions[session_idx];
                let is_sync = session.frames_encoded == 0;
                let nal_unit_type = if is_sync { 0x65u8 } else { 0x41u8 };
                let payload_bytes = {
                    let mut bytes = vec![nal_unit_type];
                    bytes.extend(input_prefix_bytes.iter().copied().take(8));
                    while bytes.len() < 24 {
                        bytes.push(
                            (reply_seed(
                                payload.session_id,
                                self.next_output_id,
                                bytes.len() as u64,
                            ) & 0xff) as u8,
                        );
                    }
                    bytes
                };
                let sample_size = (4 + payload_bytes.len()) as u32;
                if self.outputs.len() >= self.max_output_count {
                    res_header.status = STATUS_RESOURCE_EXHAUSTED;
                    return Err(());
                }
                if sample_size as usize > self.max_output_bytes
                    || self.output_bytes_total
                        > self.max_output_bytes.saturating_sub(sample_size as usize)
                {
                    res_header.status = STATUS_RESOURCE_EXHAUSTED;
                    return Err(());
                }

                let mut reply = DequeueOutputReply::zeroed();
                reply.session_id = payload.session_id;
                reply.output_id = self.next_output_id;
                reply.codec = session.codec;
                reply.width = session.width;
                reply.height = session.height;
                reply.pixel_format = session.pixel_format;
                reply.pts_value = payload.pts_value;
                reply.pts_timescale = payload.pts_timescale;
                reply.duration_value = payload.duration_value;
                reply.duration_timescale = payload.duration_timescale;
                reply.sample_size = sample_size;
                reply.sample_flags = if is_sync { ENCODE_OUTPUT_FLAG_SYNC } else { 0 };
                reply.parameter_set_count = 2;
                reply.nal_header_length = 4;
                reply.parameter_set_sizes[0] = 4;
                reply.parameter_set_sizes[1] = 4;
                reply.parameter_set_data[..8]
                    .copy_from_slice(&[0x67, 0x64, 0x00, 0x1f, 0x68, 0xee, 0x3c, 0x80]);

                let mut sample_data = vec![0u8; sample_size as usize];
                let nal_len = payload_bytes.len() as u32;
                sample_data[0..4].copy_from_slice(&nal_len.to_be_bytes());
                sample_data[4..].copy_from_slice(&payload_bytes);

                self.next_output_id += 1;
                self.output_bytes_total += sample_size as usize;
                self.outputs.push(MockOutput {
                    reply,
                    source_buffer_id: buffer.id,
                    source_buffer_generation: buffer.generation,
                    sample_data,
                    dequeued: false,
                });
                self.buffers[buffer_idx].state = BUFFER_STATE_QUEUED_TO_HOST;
                session.frames_encoded += 1;
                Ok(0)
            }
            OP_DEQUEUE_OUTPUT => {
                let Some(payload) = read_pod::<DequeueOutputPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(output_idx) = self.dequeue_output_idx(payload.session_id) else {
                    res_header.status = STATUS_TIMEOUT;
                    return Err(());
                };
                let reply = self.outputs[output_idx].reply;
                write_pod(res_payload, &reply, res_header)
            }
            OP_DEQUEUE_OUTPUT_BATCH => {
                let Some(payload) = read_pod::<DequeueOutputBatchPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let max_outputs = (payload.max_outputs as usize)
                    .min(VTF_TRANSPORT_MAX_OUTPUT_BATCH)
                    .max(1);
                let mut reply = DequeueOutputBatchReply::zeroed();
                reply.session_id = payload.session_id;

                while (reply.output_count as usize) < max_outputs {
                    let Some(output_idx) = self.dequeue_output_idx(payload.session_id) else {
                        break;
                    };
                    reply.outputs[reply.output_count as usize] = self.outputs[output_idx].reply;
                    reply.output_count += 1;
                }

                if reply.output_count == 0 {
                    res_header.status = STATUS_TIMEOUT;
                    return Err(());
                }

                write_pod(res_payload, &reply, res_header)
            }
            OP_READ_OUTPUT => {
                let Some(payload) = read_pod::<ReadOutputPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                let Some(output_idx) = self.take_output_idx(payload.output_id) else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                };
                let output = &self.outputs[output_idx];
                let mut reply = ReadOutputReply::zeroed();
                reply.output_id = output.reply.output_id;
                reply.sample_size = output.reply.sample_size;
                write_pod_and_bytes(res_payload, &reply, &output.sample_data, res_header)
            }
            OP_RELEASE_OUTPUT => {
                let Some(payload) = read_pod::<ReleaseOutputPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if self.release_output(payload.output_id) {
                    Ok(0)
                } else {
                    res_header.status = STATUS_INVALID_HANDLE;
                    Err(())
                }
            }
            OP_DESTROY_SESSION => {
                let Some(payload) = read_pod::<DestroySessionPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if self.find_session_idx(payload.session_id).is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                }
                self.remove_outputs_for_session(payload.session_id);
                self.remove_pools_for_session(payload.session_id);
                self.remove_session(payload.session_id);
                Ok(0)
            }
            OP_DRAIN => {
                let Some(payload) = read_pod::<DrainPayload>(req_payload) else {
                    res_header.status = STATUS_INTERNAL_FAILURE;
                    return Err(());
                };
                if self.find_session_idx(payload.session_id).is_none() {
                    res_header.status = STATUS_INVALID_HANDLE;
                    return Err(());
                }
                let mut reply = DrainReply::zeroed();
                reply.session_id = payload.session_id;
                reply.pending_outputs = self.count_outputs_for_session(payload.session_id);
                write_pod(res_payload, &reply, res_header)
            }
            _ => {
                res_header.status = STATUS_UNSUPPORTED_OPCODE;
                Err(())
            }
        }
    }
}

fn parse_size_env(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default_value)
}

fn reply_seed(session_id: u64, output_id: u64, index: u64) -> u64 {
    session_id
        .wrapping_mul(131)
        .wrapping_add(output_id.wrapping_mul(17))
        .wrapping_add(index.wrapping_mul(29))
}

fn read_pod<T: bytemuck::Pod>(bytes: &[u8]) -> Option<T> {
    if bytes.len() < std::mem::size_of::<T>() {
        return None;
    }
    Some(pod_read_unaligned(&bytes[..std::mem::size_of::<T>()]))
}

fn write_pod<T: bytemuck::Pod>(
    res_payload: &mut [u8],
    reply: &T,
    _res_header: &mut MessageHeader,
) -> Result<usize, ()> {
    let size = std::mem::size_of::<T>();
    if res_payload.len() < size {
        return Err(());
    }
    res_payload[..size].copy_from_slice(bytes_of(reply));
    Ok(size)
}

fn write_pod_and_bytes<T: bytemuck::Pod>(
    res_payload: &mut [u8],
    reply: &T,
    extra_bytes: &[u8],
    _res_header: &mut MessageHeader,
) -> Result<usize, ()> {
    let header_size = std::mem::size_of::<T>();
    let total_size = header_size + extra_bytes.len();
    if res_payload.len() < total_size {
        return Err(());
    }
    res_payload[..header_size].copy_from_slice(bytes_of(reply));
    res_payload[header_size..total_size].copy_from_slice(extra_bytes);
    Ok(total_size)
}

fn set_worker_name(buffer: &mut [u8; 32], name: &[u8]) {
    let copy_len = buffer.len().min(name.len());
    buffer[..copy_len].copy_from_slice(&name[..copy_len]);
}

fn align_size(value: usize, alignment: usize) -> usize {
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

fn fill_buffer_layout(
    width: u32,
    height: u32,
    pixel_format: u32,
    layout_out: &mut BufferLayoutReply,
) {
    *layout_out = BufferLayoutReply::zeroed();
    match pixel_format {
        0x3432_3076 | 0x3432_3066 => {
            let stride = align_size(width as usize, 64) as u32;
            layout_out.plane_count = 2;
            layout_out.plane_offsets[0] = 0;
            layout_out.plane_widths[0] = width;
            layout_out.plane_heights[0] = height;
            layout_out.plane_bytes_per_row[0] = stride;
            layout_out.plane_offsets[1] = stride * height;
            layout_out.plane_widths[1] = width / 2;
            layout_out.plane_heights[1] = height / 2;
            layout_out.plane_bytes_per_row[1] = stride;
            layout_out.total_size = layout_out.plane_offsets[1] + stride * (height / 2);
        }
        0x7834_3230 | 0x7866_3230 => {
            // P010: 10-bit 4:2:0 bi-planar, 2 bytes/sample. Same layout
            // shape as NV12 (Y + interleaved CbCr at half resolution)
            // but stride doubles for the 16-bit word width.
            let stride = align_size(width as usize * 2, 64) as u32;
            layout_out.plane_count = 2;
            layout_out.plane_offsets[0] = 0;
            layout_out.plane_widths[0] = width;
            layout_out.plane_heights[0] = height;
            layout_out.plane_bytes_per_row[0] = stride;
            layout_out.plane_offsets[1] = stride * height;
            layout_out.plane_widths[1] = width / 2;
            layout_out.plane_heights[1] = height / 2;
            layout_out.plane_bytes_per_row[1] = stride;
            layout_out.total_size = layout_out.plane_offsets[1] + stride * (height / 2);
        }
        _ => {
            let stride = align_size(width as usize * 4, 64) as u32;
            layout_out.plane_count = 1;
            layout_out.plane_offsets[0] = 0;
            layout_out.plane_widths[0] = width;
            layout_out.plane_heights[0] = height;
            layout_out.plane_bytes_per_row[0] = stride;
            layout_out.total_size = stride * height;
        }
    }
}

#[cfg(test)]
mod protocol_surface_tests {
    //! Mock-backend unit coverage for the protocol surface clients
    //! probe against. The smoke tests exercise the real backend via
    //! the broker / VM (~30s spinup); these run in <1ms and catch
    //! regressions where a newly added codec / pixel format is wired
    //! into one backend but forgotten in the other.
    use super::*;

    fn dispatch(
        backend: &mut MockBackend,
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
        let mut res_payload = vec![0u8; 4096];
        let written = backend
            .dispatch(&req_header, req_payload, &mut res_header, &mut res_payload)
            .expect("dispatch returned Err");
        res_payload.truncate(written);
        (res_header, res_payload)
    }

    #[test]
    fn get_caps_advertises_h264_hevc_nv12_bgra_p010() {
        let mut backend = MockBackend::new();
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
        assert!(reply.max_width >= 3840);
        assert!(reply.max_height >= 2160);
    }

    #[test]
    fn hello_reports_worker_abi_and_name() {
        let mut backend = MockBackend::new();
        let payload = HelloPayload::zeroed();
        let req_bytes = bytemuck::bytes_of(&payload);
        let (_header, res_payload) = dispatch(&mut backend, OP_HELLO, req_bytes);
        let reply: HelloReply =
            bytemuck::pod_read_unaligned(&res_payload[..std::mem::size_of::<HelloReply>()]);
        assert_eq!(reply.worker_abi_version, VTF_TRANSPORT_VERSION as u32);
        // Name is null-terminated; check the prefix.
        assert!(
            reply.worker_name.starts_with(b"vt-ferry-host-worker"),
            "unexpected worker_name: {:?}",
            &reply.worker_name[..]
        );
    }

    #[test]
    fn ping_returns_status_ok_with_empty_payload() {
        let mut backend = MockBackend::new();
        let (header, payload) = dispatch(&mut backend, OP_PING, &[]);
        assert_eq!(header.status, STATUS_OK);
        assert!(payload.is_empty());
    }

    #[test]
    fn fill_buffer_layout_p010_matches_real_backend_shape() {
        // The mock and real backends must agree on layout shape so
        // the same client code runs against either. This is a
        // narrower check than the vt_real::buffer_layout_tests
        // suite — it just asserts the mock now handles P010 (it
        // previously fell through to BGRA).
        let mut layout = BufferLayoutReply::zeroed();
        fill_buffer_layout(1920, 1080, 0x78343230, &mut layout);
        assert_eq!(layout.plane_count, 2);
        let stride = align_size(1920 * 2, 64) as u32;
        assert_eq!(layout.plane_bytes_per_row[0], stride);
        assert_eq!(layout.plane_bytes_per_row[1], stride);
        assert_eq!(layout.plane_offsets[1], stride * 1080);
        assert_eq!(layout.total_size, stride * 1080 + stride * 540);
    }

    #[test]
    fn fill_buffer_layout_p010_full_range_matches_video_range() {
        let mut a = BufferLayoutReply::zeroed();
        let mut b = BufferLayoutReply::zeroed();
        fill_buffer_layout(1280, 720, 0x78343230, &mut a);
        fill_buffer_layout(1280, 720, 0x78663230, &mut b);
        assert_eq!(a.total_size, b.total_size);
        assert_eq!(a.plane_offsets[1], b.plane_offsets[1]);
        assert_eq!(a.plane_bytes_per_row[0], b.plane_bytes_per_row[0]);
    }
}
