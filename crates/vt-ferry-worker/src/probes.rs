use std::hint::black_box;

pub fn register_dtrace_probes() {
    let enabled = std::env::var("VT_FERRY_DTRACE_PROBES")
        .ok()
        .filter(|value| value != "0")
        .is_some();
    if !enabled {
        return;
    }
    if let Err(err) = usdt::register_probes() {
        eprintln!("host-worker: failed to register DTrace USDT probes: {err}");
    }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_mailbox_doorbell() {
    crate::vt_ferry_worker::mailbox_doorbell!(|| ());
    black_box(0x1001_u64);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_mailbox_request_begin(
    opcode: u64,
    request_id: u64,
    payload_len: u64,
) {
    crate::vt_ferry_worker::mailbox_request_begin!(|| (opcode, request_id, payload_len));
    black_box((0x1002_u64, opcode, request_id, payload_len));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_mailbox_request_end(
    opcode: u64,
    request_id: u64,
    status: u64,
    response_len: u64,
) {
    crate::vt_ferry_worker::mailbox_request_end!(|| (opcode, request_id, status, response_len));
    black_box((0x1003_u64, opcode, request_id, status, response_len));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_mailbox_irq(status: u64) {
    crate::vt_ferry_worker::mailbox_irq!(|| status);
    black_box((0x1004_u64, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_begin(session_id: u64, sample_size: u64) {
    crate::vt_ferry_worker::vt_output_begin!(|| (session_id, sample_size));
    black_box((0x2001_u64, session_id, sample_size));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_shared_copy_begin(
    output_id: u64,
    sample_size: u64,
    allocation_len: u64,
) {
    crate::vt_ferry_worker::vt_output_shared_copy_begin!(|| {
        (output_id, sample_size, allocation_len)
    });
    black_box((0x2002_u64, output_id, sample_size, allocation_len));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_shared_copy_end(output_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_output_shared_copy_end!(|| (output_id, status));
    black_box((0x2003_u64, output_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_inline_copy_begin(output_id: u64, sample_size: u64) {
    crate::vt_ferry_worker::vt_output_inline_copy_begin!(|| (output_id, sample_size));
    black_box((0x2004_u64, output_id, sample_size));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_inline_copy_end(output_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_output_inline_copy_end!(|| (output_id, status));
    black_box((0x2005_u64, output_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_output_queued(
    output_id: u64,
    sample_size: u64,
    storage_kind: u64,
) {
    crate::vt_ferry_worker::vt_output_queued!(|| (output_id, sample_size, storage_kind));
    black_box((0x2006_u64, output_id, sample_size, storage_kind));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_read_output_copy_begin(
    output_id: u64,
    sample_size: u64,
    storage_kind: u64,
) {
    crate::vt_ferry_worker::vt_read_output_copy_begin!(|| (output_id, sample_size, storage_kind));
    black_box((0x2007_u64, output_id, sample_size, storage_kind));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_read_output_copy_end(output_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_read_output_copy_end!(|| (output_id, status));
    black_box((0x2008_u64, output_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_release_output(
    output_id: u64,
    sample_size: u64,
    storage_kind: u64,
) {
    crate::vt_ferry_worker::vt_release_output!(|| (output_id, sample_size, storage_kind));
    black_box((0x2009_u64, output_id, sample_size, storage_kind));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_source_wrap_begin(
    buffer_id: u64,
    slot_index: u64,
    mapped_size: u64,
    pixel_format: u64,
) {
    crate::vt_ferry_worker::vt_source_wrap_begin!(|| {
        (buffer_id, slot_index, mapped_size, pixel_format)
    });
    black_box((0x2010_u64, buffer_id, slot_index, mapped_size, pixel_format));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_source_wrap_end(buffer_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_source_wrap_end!(|| (buffer_id, status));
    black_box((0x2011_u64, buffer_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_session_create_begin(
    width: u64,
    height: u64,
    codec: u64,
    pixel_format: u64,
) {
    crate::vt_ferry_worker::vt_session_create_begin!(|| (width, height, codec, pixel_format));
    black_box((0x2101_u64, width, height, codec, pixel_format));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_session_create_end(session_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_session_create_end!(|| (session_id, status));
    black_box((0x2102_u64, session_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_set_property_begin(session_id: u64, value_kind: u64) {
    crate::vt_ferry_worker::vt_set_property_begin!(|| (session_id, value_kind));
    black_box((0x2103_u64, session_id, value_kind));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_set_property_end(session_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_set_property_end!(|| (session_id, status));
    black_box((0x2104_u64, session_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_prepare_begin(session_id: u64) {
    crate::vt_ferry_worker::vt_prepare_begin!(|| session_id);
    black_box((0x2105_u64, session_id));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_prepare_end(session_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_prepare_end!(|| (session_id, status));
    black_box((0x2106_u64, session_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_encode_frame_begin(
    session_id: u64,
    buffer_id: u64,
    generation: u64,
) {
    crate::vt_ferry_worker::vt_encode_frame_begin!(|| (session_id, buffer_id, generation));
    black_box((0x2107_u64, session_id, buffer_id, generation));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_encode_frame_end(session_id: u64, buffer_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_encode_frame_end!(|| (session_id, buffer_id, status));
    black_box((0x2108_u64, session_id, buffer_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_encode_output_begin(
    session_id: u64,
    buffer_id: u64,
    generation: u64,
    sample_size: u64,
) {
    crate::vt_ferry_worker::vt_encode_output_begin!(|| {
        (session_id, buffer_id, generation, sample_size)
    });
    black_box((0x2109_u64, session_id, buffer_id, generation, sample_size));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_complete_frames_begin(session_id: u64) {
    crate::vt_ferry_worker::vt_complete_frames_begin!(|| session_id);
    black_box((0x2110_u64, session_id));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_vt_complete_frames_end(session_id: u64, status: u64) {
    crate::vt_ferry_worker::vt_complete_frames_end!(|| (session_id, status));
    black_box((0x2111_u64, session_id, status));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_cv_pixel_buffer_lock_begin(buffer_id: u64) {
    crate::vt_ferry_worker::cv_pixel_buffer_lock_begin!(|| buffer_id);
    black_box((0x2112_u64, buffer_id));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn vt_ferry_probe_cv_pixel_buffer_lock_end(buffer_id: u64, status: u64) {
    crate::vt_ferry_worker::cv_pixel_buffer_lock_end!(|| (buffer_id, status));
    black_box((0x2113_u64, buffer_id, status));
}
