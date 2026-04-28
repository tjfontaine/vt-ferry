#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::corefoundation::{
    kCFBooleanFalse, kCFBooleanTrue, kCFTypeArrayCallBacks, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks, vtf_cf_string, Boolean, CFArrayRef, CFDictionaryRef,
    CFStringRef,
};
use crate::runtime::*;
use std::ffi::c_void;
use std::ptr;

pub type OSStatus = i32;
pub const noErr: i32 = 0;
pub const kCMFormatDescriptionError_InvalidParameter: i32 = -12710;
pub const kVTAllocationFailedErr: i32 = -12900;

pub const kCMTimeFlags_Valid: u32 = 1 | 0; // matching C code basic bitmask
pub const kCMTimeFlags_Indefinite: u32 = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CMTime {
    pub value: i64,
    pub timescale: i32,
    pub flags: u32,
    pub epoch: i64,
}

pub type CMBlockBufferRef = CFTypeRef;
pub type CMSampleBufferRef = CFTypeRef;
pub type CMFormatDescriptionRef = CFTypeRef;
pub type CMBlockBufferFlags = u32;
pub type CMVideoCodecType = u32;
pub type CMItemCount = isize;
pub type CFAllocatorRef = CFTypeRef;
pub const VTF_ENCODE_OUTPUT_FLAG_SYNC: u32 = 1;

#[repr(C)]
pub struct CMSampleTimingInfo {
    pub duration: CMTime,
    pub presentationTimeStamp: CMTime,
    pub decodeTimeStamp: CMTime,
}

pub type CMSampleBufferMakeDataReadyCallback = Option<unsafe extern "C" fn()>;

#[repr(C)]
pub struct vtf_cm_block_buffer {
    pub base: vtf_cf_object,
    pub bytes: *mut u8,
    pub length: usize,
    pub owns_bytes: bool,
    pub output_id: u64,
}

#[repr(C)]
pub struct vtf_cm_format_description {
    pub base: vtf_cf_object,
    pub codec_type: CMVideoCodecType,
    pub width: i32,
    pub height: i32,
    pub extensions: CFDictionaryRef,
    pub nal_header_length: i32,
    pub parameter_set_count: usize,
    pub parameter_set_sizes: [usize; 4],
    pub parameter_sets: [*mut u8; 4],
}

#[repr(C)]
pub struct vtf_cm_sample_buffer {
    pub base: vtf_cf_object,
    pub data_buffer: CMBlockBufferRef,
    pub format_description: CMFormatDescriptionRef,
    pub decode_timestamp: CMTime,
    pub presentation_timestamp: CMTime,
    pub total_sample_size: usize,
    pub attachments: CFTypeRef,         // CFArrayRef
    pub source_image_buffer: CFTypeRef, // CVImageBufferRef
    pub output_id: u64,
}

#[no_mangle]
pub static kCMTimeInvalid: CMTime = CMTime {
    value: 0,
    timescale: 0,
    flags: 0,
    epoch: 0,
};
#[no_mangle]
pub static kCMTimeIndefinite: CMTime = CMTime {
    value: 0,
    timescale: 0,
    flags: kCMTimeFlags_Valid | kCMTimeFlags_Indefinite,
    epoch: 0,
};

static mut vtf_kCMSampleAttachmentKey_NotSync_storage: vtf_cf_string = vtf_cf_string {
    base: vtf_cf_object {
        magic: 0x534d5654,
        type_id: VTF_TYPE_STRING,
        refcount: std::sync::atomic::AtomicI32::new(1),
        flags: VTF_OBJECT_FLAG_STATIC,
        proxy_id: 0,
        generation: 1,
        host_id: 0,
        finalize: None,
    },
    bytes: b"NotSync\0".as_ptr(),
    length: 7,
    owns_bytes: false,
};

static mut vtf_kCMFormatDescriptionKey_PixelAspectRatioHorizontalSpacing_storage: vtf_cf_string =
    vtf_cf_string {
        base: vtf_cf_object {
            magic: 0x534d5654,
            type_id: VTF_TYPE_STRING,
            refcount: std::sync::atomic::AtomicI32::new(1),
            flags: VTF_OBJECT_FLAG_STATIC,
            proxy_id: 0,
            generation: 1,
            host_id: 0,
            finalize: None,
        },
        bytes: b"HorizontalSpacing\0".as_ptr(),
        length: 17,
        owns_bytes: false,
    };

static mut vtf_kCMFormatDescriptionKey_PixelAspectRatioVerticalSpacing_storage: vtf_cf_string =
    vtf_cf_string {
        base: vtf_cf_object {
            magic: 0x534d5654,
            type_id: VTF_TYPE_STRING,
            refcount: std::sync::atomic::AtomicI32::new(1),
            flags: VTF_OBJECT_FLAG_STATIC,
            proxy_id: 0,
            generation: 1,
            host_id: 0,
            finalize: None,
        },
        bytes: b"VerticalSpacing\0".as_ptr(),
        length: 15,
        owns_bytes: false,
    };

static mut vtf_kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms_storage:
    vtf_cf_string = vtf_cf_string {
    base: vtf_cf_object {
        magic: 0x534d5654,
        type_id: VTF_TYPE_STRING,
        refcount: std::sync::atomic::AtomicI32::new(1),
        flags: VTF_OBJECT_FLAG_STATIC,
        proxy_id: 0,
        generation: 1,
        host_id: 0,
        finalize: None,
    },
    bytes: b"SampleDescriptionExtensionAtoms\0".as_ptr(),
    length: 31,
    owns_bytes: false,
};

static mut vtf_kCMFormatDescriptionExtension_VerbatimSampleDescription_storage: vtf_cf_string =
    vtf_cf_string {
        base: vtf_cf_object {
            magic: 0x534d5654,
            type_id: VTF_TYPE_STRING,
            refcount: std::sync::atomic::AtomicI32::new(1),
            flags: VTF_OBJECT_FLAG_STATIC,
            proxy_id: 0,
            generation: 1,
            host_id: 0,
            finalize: None,
        },
        bytes: b"VerbatimSampleDescription\0".as_ptr(),
        length: 25,
        owns_bytes: false,
    };

#[repr(transparent)]
pub struct ExportedSymbol(pub *const c_void);
unsafe impl Sync for ExportedSymbol {}
unsafe impl Send for ExportedSymbol {}

#[no_mangle]
pub static kCMSampleAttachmentKey_NotSync: ExportedSymbol = ExportedSymbol(
    &raw const vtf_kCMSampleAttachmentKey_NotSync_storage as *const _ as *const c_void,
);
#[no_mangle]
pub static kCMFormatDescriptionKey_PixelAspectRatioHorizontalSpacing: ExportedSymbol =
    ExportedSymbol(
        &raw const vtf_kCMFormatDescriptionKey_PixelAspectRatioHorizontalSpacing_storage
            as *const _ as *const c_void,
    );
#[no_mangle]
pub static kCMFormatDescriptionKey_PixelAspectRatioVerticalSpacing: ExportedSymbol =
    ExportedSymbol(
        &raw const vtf_kCMFormatDescriptionKey_PixelAspectRatioVerticalSpacing_storage
            as *const _ as *const c_void,
    );
#[no_mangle]
pub static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: ExportedSymbol =
    ExportedSymbol(
        &raw const vtf_kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms_storage
            as *const _ as *const c_void,
    );
#[no_mangle]
pub static kCMFormatDescriptionExtension_VerbatimSampleDescription: ExportedSymbol =
    ExportedSymbol(
        &raw const vtf_kCMFormatDescriptionExtension_VerbatimSampleDescription_storage
            as *const _ as *const c_void,
    );

unsafe fn vtf_finalize_block_buffer(obj: *mut vtf_cf_object) {
    let bb = obj as *mut vtf_cm_block_buffer;
    if (*bb).owns_bytes && !(*bb).bytes.is_null() && (*bb).length > 0 {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut((*bb).bytes, (*bb).length));
    }
    let _ = Box::from_raw(bb);
}

unsafe fn vtf_finalize_format_description(obj: *mut vtf_cf_object) {
    let fd = obj as *mut vtf_cm_format_description;
    crate::runtime::CFRelease((*fd).extensions);
    for i in 0..(*fd).parameter_set_count {
        if i < 4 && !(*fd).parameter_sets[i].is_null() {
            let _ = Box::from_raw(std::slice::from_raw_parts_mut(
                (*fd).parameter_sets[i],
                (*fd).parameter_set_sizes[i],
            ));
        }
    }
    let _ = Box::from_raw(fd);
}

unsafe fn vtf_finalize_sample_buffer(obj: *mut vtf_cf_object) {
    let sb = obj as *mut vtf_cm_sample_buffer;
    if (*sb).output_id != 0 {
        let _ = crate::transport::release_output((*sb).output_id);
    }
    if !(*sb).source_image_buffer.is_null() {
        let pixel_buffer = (*sb).source_image_buffer as *mut crate::corevideo::vtf_cv_pixel_buffer;
        if !pixel_buffer.is_null() {
            (*pixel_buffer).state = crate::corevideo::VTF_CV_BUFFER_STATE_GUEST_WRITABLE;
        }
    }
    crate::runtime::CFRelease((*sb).source_image_buffer);
    crate::runtime::CFRelease((*sb).data_buffer);
    crate::runtime::CFRelease((*sb).format_description);
    crate::runtime::CFRelease((*sb).attachments);
    let _ = Box::from_raw(sb);
}

unsafe fn vtf_copy_from_mmio(src: *const u8, dst: *mut u8, len: usize) {
    for index in 0..len {
        *dst.add(index) = std::ptr::read_volatile(src.add(index));
    }
    crate::runtime::vtf_record_data_copy(len);
}

#[no_mangle]
pub extern "C" fn CMTimeMake(value: i64, timescale: i32) -> CMTime {
    if timescale == 0 {
        kCMTimeInvalid
    } else {
        CMTime {
            value,
            timescale,
            flags: kCMTimeFlags_Valid,
            epoch: 0,
        }
    }
}

pub fn is_cmtime_invalid(time: CMTime) -> bool {
    (time.flags & kCMTimeFlags_Valid) == 0
}

#[no_mangle]
pub extern "C" fn CMTimeCompare(lhs: CMTime, rhs: CMTime) -> i32 {
    if is_cmtime_invalid(lhs) && is_cmtime_invalid(rhs) {
        return 0;
    }
    if is_cmtime_invalid(lhs) {
        return -1;
    }
    if is_cmtime_invalid(rhs) {
        return 1;
    }

    let lhs_v = (lhs.value as f64) / (lhs.timescale as f64);
    let rhs_v = (rhs.value as f64) / (rhs.timescale as f64);

    if lhs_v < rhs_v {
        -1
    } else if lhs_v > rhs_v {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn CMBlockBufferCreateWithMemoryBlock(
    _structureAllocator: CFAllocatorRef,
    memoryBlock: *mut c_void,
    blockLength: usize,
    _blockAllocator: CFAllocatorRef,
    _customBlockSource: *const c_void,
    offsetToData: usize,
    dataLength: usize,
    _flags: CMBlockBufferFlags,
    blockBufferOut: *mut CMBlockBufferRef,
) -> OSStatus {
    if blockBufferOut.is_null()
        || offsetToData > blockLength
        || dataLength > (blockLength - offsetToData)
    {
        return kCMFormatDescriptionError_InvalidParameter;
    }

    let bb = Box::into_raw(Box::new(vtf_cm_block_buffer {
        base: vtf_cf_object::init(VTF_TYPE_BLOCK_BUFFER, Some(vtf_finalize_block_buffer)),
        bytes: ptr::null_mut(),
        length: dataLength,
        owns_bytes: true,
        output_id: 0,
    }));

    if dataLength > 0 {
        let mut slice = vec![0u8; dataLength];
        if !memoryBlock.is_null() {
            let src = (memoryBlock as *const u8).add(offsetToData);
            for index in 0..dataLength {
                *slice.as_mut_ptr().add(index) = std::ptr::read_volatile(src.add(index));
            }
            crate::runtime::vtf_record_data_copy(dataLength);
        }
        (*bb).bytes = Box::into_raw(slice.into_boxed_slice()) as *mut u8;
    }

    *blockBufferOut = bb as CMBlockBufferRef;
    noErr
}

#[no_mangle]
pub unsafe extern "C" fn CMBlockBufferGetDataLength(buffer_ref: CMBlockBufferRef) -> usize {
    if buffer_ref.is_null() {
        return 0;
    }
    let bb = buffer_ref as *const vtf_cm_block_buffer;
    (*bb).length
}

#[no_mangle]
pub unsafe extern "C" fn CMBlockBufferCopyDataBytes(
    buffer_ref: CMBlockBufferRef,
    offsetToData: usize,
    dataLength: usize,
    destination: *mut c_void,
) -> OSStatus {
    if buffer_ref.is_null() || destination.is_null() {
        return kCMFormatDescriptionError_InvalidParameter;
    }
    let bb = buffer_ref as *const vtf_cm_block_buffer;
    if offsetToData > (*bb).length || dataLength > ((*bb).length - offsetToData) {
        return kCMFormatDescriptionError_InvalidParameter;
    }
    let source = (*bb).bytes.add(offsetToData);
    let destination = destination.cast::<u8>();
    if !(*bb).owns_bytes && (*bb).output_id != 0 {
        // vtf_copy_from_mmio records the byte count itself.
        vtf_copy_from_mmio(source, destination, dataLength);
    } else {
        std::ptr::copy_nonoverlapping(source, destination, dataLength);
        crate::runtime::vtf_record_data_copy(dataLength);
    }
    noErr
}

#[no_mangle]
pub unsafe extern "C" fn CMBlockBufferGetDataPointer(
    buffer_ref: CMBlockBufferRef,
    offset: usize,
    lengthAtOffsetOut: *mut usize,
    totalLengthOut: *mut usize,
    dataPointerOut: *mut *mut i8,
) -> OSStatus {
    if buffer_ref.is_null() || offset > (*(buffer_ref as *const vtf_cm_block_buffer)).length {
        return kCMFormatDescriptionError_InvalidParameter;
    }
    let bb = buffer_ref as *const vtf_cm_block_buffer;
    if !lengthAtOffsetOut.is_null() {
        *lengthAtOffsetOut = (*bb).length - offset;
    }
    if !totalLengthOut.is_null() {
        *totalLengthOut = (*bb).length;
    }
    if !dataPointerOut.is_null() {
        if !(*bb).owns_bytes && (*bb).output_id != 0 {
            return kCMFormatDescriptionError_InvalidParameter;
        }
        *dataPointerOut = (*bb).bytes.add(offset).cast::<i8>();
    }
    noErr
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetDataBuffer(
    sampleBuffer_ref: CMSampleBufferRef,
) -> CMBlockBufferRef {
    if sampleBuffer_ref.is_null() {
        return ptr::null();
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).data_buffer
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetFormatDescription(
    sampleBuffer_ref: CMSampleBufferRef,
) -> CMFormatDescriptionRef {
    if sampleBuffer_ref.is_null() {
        return ptr::null();
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).format_description
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetPresentationTimeStamp(
    sampleBuffer_ref: CMSampleBufferRef,
) -> CMTime {
    if sampleBuffer_ref.is_null() {
        return kCMTimeInvalid;
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).presentation_timestamp
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetDecodeTimeStamp(
    sampleBuffer_ref: CMSampleBufferRef,
) -> CMTime {
    if sampleBuffer_ref.is_null() {
        return kCMTimeInvalid;
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).decode_timestamp
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetSampleAttachmentsArray(
    sampleBuffer_ref: CMSampleBufferRef,
    _createIfNecessary: Boolean,
) -> CFTypeRef {
    if sampleBuffer_ref.is_null() {
        return ptr::null();
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).attachments
}

#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferGetTotalSampleSize(
    sampleBuffer_ref: CMSampleBufferRef,
) -> usize {
    if sampleBuffer_ref.is_null() {
        return 0;
    }
    let sb = sampleBuffer_ref as *const vtf_cm_sample_buffer;
    (*sb).total_sample_size
}

#[no_mangle]
pub unsafe extern "C" fn CMFormatDescriptionGetExtension(
    formatDescription: CMFormatDescriptionRef,
    _extensionKey: CFStringRef,
) -> CFTypeRef {
    if formatDescription.is_null() {
        return ptr::null();
    }
    let fd = formatDescription as *const vtf_cm_format_description;
    (*fd).extensions
}

/// `CMMediaType` constant for video — `'vide'` FourCC.
pub const VTF_CM_MEDIA_TYPE_VIDEO: u32 = 0x76_69_64_65;

/// Returns `kCMMediaType_Video` for any `vtf_cm_format_description`
/// — the only kind of format description we mint guest-side.
/// FFmpeg's videotoolbox decoder probes this to confirm it's
/// looking at a video format description.
#[no_mangle]
pub unsafe extern "C" fn CMFormatDescriptionGetMediaType(
    formatDescription: CMFormatDescriptionRef,
) -> u32 {
    if formatDescription.is_null() {
        return 0;
    }
    VTF_CM_MEDIA_TYPE_VIDEO
}

/// Returns the codec FourCC stored in the format description
/// (e.g. `'avc1'` / `'hvc1'`). FFmpeg uses this to choose the
/// decoder code path after format description construction.
#[no_mangle]
pub unsafe extern "C" fn CMFormatDescriptionGetMediaSubType(
    formatDescription: CMFormatDescriptionRef,
) -> u32 {
    if formatDescription.is_null() {
        return 0;
    }
    let fd = formatDescription as *const vtf_cm_format_description;
    (*fd).codec_type
}

/// `CMVideoDimensions` — `{ int32_t width; int32_t height; }`.
/// Apple's struct layout; passed by value where ABI permits and
/// by reference otherwise (8-byte struct fits in a register pair
/// on aarch64).
#[repr(C)]
pub struct CMVideoDimensions {
    pub width: i32,
    pub height: i32,
}

/// Returns dimensions stored on the format description. For
/// format descriptions built via
/// `CMVideoFormatDescriptionCreateFromH264/HEVCParameterSets`,
/// width / height are 0 (the worker derives them from SPS during
/// `OP_SET_DECODE_FORMAT`). FFmpeg already parses SPS itself
/// before calling our constructor so this 0,0 result generally
/// goes unused — but we expose the symbol so dynamic queries
/// don't fail at link/runtime.
#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionGetDimensions(
    videoDesc: CMFormatDescriptionRef,
) -> CMVideoDimensions {
    if videoDesc.is_null() {
        return CMVideoDimensions { width: 0, height: 0 };
    }
    let fd = videoDesc as *const vtf_cm_format_description;
    CMVideoDimensions {
        width: (*fd).width,
        height: (*fd).height,
    }
}

/// Shared body of the H.264/HEVC parameter-set accessors. The two
/// public entry points only differ in the codec FourCC they accept;
/// once the format-description is identified as the right codec, the
/// rest of the lookup is identical.
unsafe fn vtf_video_format_description_get_parameter_set_at_index(
    videoDesc: CMFormatDescriptionRef,
    expected_codec: u32,
    parameterSetIndex: usize,
    parameterSetPointerOut: *mut *const u8,
    parameterSetSizeOut: *mut usize,
    parameterSetCountOut: *mut usize,
    NALUnitHeaderLengthOut: *mut i32,
) -> OSStatus {
    if videoDesc.is_null() {
        return kCMFormatDescriptionError_InvalidParameter;
    }
    let fd = videoDesc as *const vtf_cm_format_description;
    if (*fd).codec_type != expected_codec {
        return kCMFormatDescriptionError_InvalidParameter;
    }

    if !parameterSetCountOut.is_null() {
        *parameterSetCountOut = (*fd).parameter_set_count;
    }
    if !NALUnitHeaderLengthOut.is_null() {
        *NALUnitHeaderLengthOut = (*fd).nal_header_length;
    }
    if parameterSetIndex >= (*fd).parameter_set_count {
        if !parameterSetPointerOut.is_null() {
            *parameterSetPointerOut = ptr::null();
        }
        if !parameterSetSizeOut.is_null() {
            *parameterSetSizeOut = 0;
        }
        return kCMFormatDescriptionError_InvalidParameter;
    }

    if !parameterSetPointerOut.is_null() {
        *parameterSetPointerOut = (*fd).parameter_sets[parameterSetIndex];
    }
    if !parameterSetSizeOut.is_null() {
        *parameterSetSizeOut = (*fd).parameter_set_sizes[parameterSetIndex];
    }

    noErr
}

#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
    videoDesc: CMFormatDescriptionRef,
    parameterSetIndex: usize,
    parameterSetPointerOut: *mut *const u8,
    parameterSetSizeOut: *mut usize,
    parameterSetCountOut: *mut usize,
    NALUnitHeaderLengthOut: *mut i32,
) -> OSStatus {
    vtf_video_format_description_get_parameter_set_at_index(
        videoDesc,
        VTF_VIDEO_CODEC_H264,
        parameterSetIndex,
        parameterSetPointerOut,
        parameterSetSizeOut,
        parameterSetCountOut,
        NALUnitHeaderLengthOut,
    )
}

#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
    videoDesc: CMFormatDescriptionRef,
    parameterSetIndex: usize,
    parameterSetPointerOut: *mut *const u8,
    parameterSetSizeOut: *mut usize,
    parameterSetCountOut: *mut usize,
    NALUnitHeaderLengthOut: *mut i32,
) -> OSStatus {
    vtf_video_format_description_get_parameter_set_at_index(
        videoDesc,
        VTF_VIDEO_CODEC_HEVC,
        parameterSetIndex,
        parameterSetPointerOut,
        parameterSetSizeOut,
        parameterSetCountOut,
        NALUnitHeaderLengthOut,
    )
}

pub unsafe fn vtf_create_block_buffer_from_bytes(bytes: &[u8]) -> CMBlockBufferRef {
    let mut out: CMBlockBufferRef = ptr::null();
    let status = CMBlockBufferCreateWithMemoryBlock(
        ptr::null(),
        bytes.as_ptr() as *mut c_void,
        bytes.len(),
        ptr::null(),
        ptr::null(),
        0,
        bytes.len(),
        0,
        &mut out,
    );
    if status != noErr {
        ptr::null()
    } else {
        out
    }
}

pub unsafe fn vtf_create_block_buffer_alias_from_bytes(
    bytes: &[u8],
    output_id: u64,
) -> CMBlockBufferRef {
    Box::into_raw(Box::new(vtf_cm_block_buffer {
        base: vtf_cf_object::init(VTF_TYPE_BLOCK_BUFFER, Some(vtf_finalize_block_buffer)),
        bytes: bytes.as_ptr() as *mut u8,
        length: bytes.len(),
        owns_bytes: false,
        output_id,
    })) as CMBlockBufferRef
}

/// FourCC for h264 (`'avc1'`).
pub const VTF_VIDEO_CODEC_H264: u32 = 0x61766331;
/// FourCC for HEVC (`'hvc1'`).
pub const VTF_VIDEO_CODEC_HEVC: u32 = 0x68766331;

pub unsafe fn vtf_create_video_format_description(
    codec_type: u32,
    width: i32,
    height: i32,
    parameter_set_count: usize,
    parameter_set_sizes: &[u32],
    parameter_set_data: &[u8],
    nal_header_length: i32,
) -> CMFormatDescriptionRef {
    let mut fd = Box::new(vtf_cm_format_description {
        base: vtf_cf_object::init(
            VTF_TYPE_FORMAT_DESCRIPTION,
            Some(vtf_finalize_format_description),
        ),
        codec_type,
        width,
        height,
        extensions: ptr::null(),
        nal_header_length,
        parameter_set_count: parameter_set_count.min(4),
        parameter_set_sizes: [0; 4],
        parameter_sets: [ptr::null_mut(); 4],
    });

    let mut offset = 0usize;
    for index in 0..fd.parameter_set_count {
        let size = parameter_set_sizes.get(index).copied().unwrap_or(0) as usize;
        if size == 0 || offset + size > parameter_set_data.len() {
            fd.parameter_set_count = index;
            break;
        }
        let mut data = vec![0u8; size];
        data.copy_from_slice(&parameter_set_data[offset..offset + size]);
        fd.parameter_set_sizes[index] = size;
        fd.parameter_sets[index] = Box::into_raw(data.into_boxed_slice()) as *mut u8;
        offset += size;
    }

    Box::into_raw(fd) as CMFormatDescriptionRef
}

/// Convenience wrapper for the H.264 case. Existing call sites kept
/// happy without wiring codec discrimination through.
pub unsafe fn vtf_create_h264_format_description(
    width: i32,
    height: i32,
    parameter_set_count: usize,
    parameter_set_sizes: &[u32],
    parameter_set_data: &[u8],
    nal_header_length: i32,
) -> CMFormatDescriptionRef {
    vtf_create_video_format_description(
        VTF_VIDEO_CODEC_H264,
        width,
        height,
        parameter_set_count,
        parameter_set_sizes,
        parameter_set_data,
        nal_header_length,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corefoundation::{
        CFArrayGetCount, CFArrayGetValueAtIndex, CFBooleanGetValue, CFDictionaryGetValue,
        kCFBooleanFalse, kCFBooleanTrue,
    };
    use std::ffi::c_void;

    #[test]
    fn cmtime_make_then_compare() {
        let a = CMTimeMake(100, 25);
        let b = CMTimeMake(200, 25);
        assert_eq!(a.flags & kCMTimeFlags_Valid, kCMTimeFlags_Valid);
        assert_eq!(CMTimeCompare(a, b), -1);
        assert_eq!(CMTimeCompare(b, a), 1);
        assert_eq!(CMTimeCompare(a, a), 0);
    }

    #[test]
    fn cmtime_invalid_when_timescale_zero() {
        let invalid = CMTimeMake(100, 0);
        assert!(is_cmtime_invalid(invalid));
        assert_eq!(CMTimeCompare(invalid, invalid), 0);
        // Invalid sorts strictly before any valid value.
        let valid = CMTimeMake(0, 25);
        assert_eq!(CMTimeCompare(invalid, valid), -1);
        assert_eq!(CMTimeCompare(valid, invalid), 1);
    }

    #[test]
    fn cmtime_kcmtimeinvalid_constant_is_invalid() {
        assert!(is_cmtime_invalid(kCMTimeInvalid));
    }

    #[test]
    fn block_buffer_data_pointer_aliases_input() {
        unsafe {
            let payload: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
            let bb = vtf_create_block_buffer_from_bytes(&payload);
            assert!(!bb.is_null());
            assert_eq!(CMBlockBufferGetDataLength(bb), payload.len());

            // Read via CMBlockBufferGetDataPointer — guest path for parameter sets.
            let mut data: *mut i8 = std::ptr::null_mut();
            let mut total: usize = 0;
            let mut at_offset: usize = 0;
            let status = CMBlockBufferGetDataPointer(
                bb,
                0,
                &mut at_offset as *mut usize,
                &mut total as *mut usize,
                &mut data as *mut *mut i8,
            );
            assert_eq!(status, noErr);
            assert_eq!(total, payload.len());
            assert_eq!(at_offset, payload.len());
            let view = std::slice::from_raw_parts(data as *const u8, payload.len());
            assert_eq!(view, &payload);

            // Read via CMBlockBufferCopyDataBytes — FFmpeg's preferred path.
            let mut copy = [0u8; 8];
            let status = CMBlockBufferCopyDataBytes(
                bb,
                0,
                payload.len(),
                copy.as_mut_ptr() as *mut c_void,
            );
            assert_eq!(status, noErr);
            assert_eq!(copy, payload);

            crate::runtime::CFRelease(bb);
        }
    }

    #[test]
    fn block_buffer_rejects_out_of_range_copy() {
        unsafe {
            let payload = [0u8; 4];
            let bb = vtf_create_block_buffer_from_bytes(&payload);
            let mut dst = [0u8; 8];
            let status =
                CMBlockBufferCopyDataBytes(bb, 0, 8, dst.as_mut_ptr() as *mut c_void);
            assert_ne!(status, noErr);
            crate::runtime::CFRelease(bb);
        }
    }

    /// Build a minimal sample buffer (no real format description / image
    /// buffer) so we can exercise the post-encode metadata path without a
    /// host worker. The data buffer is a guest-owned byte block.
    unsafe fn make_sample_buffer(pts: CMTime, dts: CMTime, sync: bool) -> CMSampleBufferRef {
        let payload = [0xAAu8; 4];
        let bb = vtf_create_block_buffer_from_bytes(&payload);
        let format = vtf_create_h264_format_description(
            128, 72, 0, &[], &[], 4,
        );
        let flags = if sync { VTF_ENCODE_OUTPUT_FLAG_SYNC } else { 0 };
        let sb = vtf_create_sample_buffer(
            bb,
            format,
            pts,
            dts,
            payload.len(),
            flags,
            0,
            std::ptr::null(),
        );
        crate::runtime::CFRelease(bb);
        crate::runtime::CFRelease(format);
        sb
    }

    #[test]
    fn sample_buffer_pts_round_trip() {
        unsafe {
            let pts = CMTimeMake(0x1234, 600);
            let dts = CMTimeMake(0x1230, 600);
            let sb = make_sample_buffer(pts, dts, true);

            let got_pts = CMSampleBufferGetPresentationTimeStamp(sb);
            let got_dts = CMSampleBufferGetDecodeTimeStamp(sb);
            assert_eq!(got_pts.value, pts.value);
            assert_eq!(got_pts.timescale, pts.timescale);
            assert_eq!(got_dts.value, dts.value);
            assert_eq!(got_dts.timescale, dts.timescale);

            crate::runtime::CFRelease(sb);
        }
    }

    #[test]
    fn sample_buffer_keyframe_attachment_indicates_sync_frame() {
        // FFmpeg looks up the kCMSampleAttachmentKey_NotSync attachment
        // and treats `false` (or missing) as "this is a keyframe". We
        // store the inverse of VTF_ENCODE_OUTPUT_FLAG_SYNC, so a sync
        // frame yields kCFBooleanFalse and a non-sync frame yields
        // kCFBooleanTrue.
        unsafe {
            let sync_sb = make_sample_buffer(CMTimeMake(0, 600), CMTimeMake(0, 600), true);
            let attachments = CMSampleBufferGetSampleAttachmentsArray(sync_sb, 0);
            assert!(!attachments.is_null());
            assert_eq!(CFArrayGetCount(attachments), 1);
            let dict = CFArrayGetValueAtIndex(attachments, 0);
            let value = CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync.0);
            assert_eq!(value, kCFBooleanFalse.0, "sync frame must report NotSync=false");
            assert_eq!(CFBooleanGetValue(value), 0);
            crate::runtime::CFRelease(sync_sb);

            let p_sb = make_sample_buffer(CMTimeMake(1, 600), CMTimeMake(1, 600), false);
            let attachments = CMSampleBufferGetSampleAttachmentsArray(p_sb, 0);
            let dict = CFArrayGetValueAtIndex(attachments, 0);
            let value = CFDictionaryGetValue(dict, kCMSampleAttachmentKey_NotSync.0);
            assert_eq!(
                value, kCFBooleanTrue.0,
                "non-sync (e.g. P/B) frame must report NotSync=true"
            );
            assert_eq!(CFBooleanGetValue(value), 1);
            crate::runtime::CFRelease(p_sb);
        }
    }

    #[test]
    fn sample_buffer_total_size_is_recoverable() {
        unsafe {
            let sb = make_sample_buffer(CMTimeMake(0, 30), CMTimeMake(0, 30), true);
            assert_eq!(CMSampleBufferGetTotalSampleSize(sb), 4);
            crate::runtime::CFRelease(sb);
        }
    }

    #[test]
    fn null_inputs_are_safe() {
        unsafe {
            assert_eq!(CMBlockBufferGetDataLength(std::ptr::null()), 0);
            assert!(is_cmtime_invalid(
                CMSampleBufferGetPresentationTimeStamp(std::ptr::null())
            ));
            assert!(is_cmtime_invalid(
                CMSampleBufferGetDecodeTimeStamp(std::ptr::null())
            ));
            assert!(CMSampleBufferGetDataBuffer(std::ptr::null()).is_null());
            assert!(CMSampleBufferGetFormatDescription(std::ptr::null()).is_null());
            assert!(CMSampleBufferGetSampleAttachmentsArray(std::ptr::null(), 0).is_null());
        }
    }

    /// `VTF_DATA_COPY_*` are process-globals; serialize the asserting
    /// tests so they don't race with each other or with the CM-path
    /// tests above (which also touch them).
    fn data_copy_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[test]
    fn data_copy_counter_ticks_on_block_buffer_create_with_owned_bytes() {
        let _guard = data_copy_lock().lock().unwrap();
        unsafe {
            let bytes_before = crate::runtime::vtf_data_copy_bytes();
            let events_before = crate::runtime::vtf_data_copy_events();

            let payload = [0xABu8; 16];
            let bb = vtf_create_block_buffer_from_bytes(&payload);
            assert!(!bb.is_null());

            assert_eq!(
                crate::runtime::vtf_data_copy_bytes() - bytes_before,
                payload.len() as u64,
                "construction must register one defensive copy of the source bytes"
            );
            assert_eq!(
                crate::runtime::vtf_data_copy_events() - events_before,
                1,
                "construction is one event"
            );

            crate::runtime::CFRelease(bb);
        }
    }

    #[test]
    fn data_copy_counter_ticks_on_copy_data_bytes() {
        let _guard = data_copy_lock().lock().unwrap();
        unsafe {
            let payload = [0u8; 32];
            let bb = vtf_create_block_buffer_from_bytes(&payload);
            // Drop construction's copy from the baseline so we measure
            // only the CopyDataBytes-driven copy below.
            let bytes_before = crate::runtime::vtf_data_copy_bytes();
            let events_before = crate::runtime::vtf_data_copy_events();

            let mut dst = [0u8; 16];
            let status = CMBlockBufferCopyDataBytes(
                bb,
                4,
                dst.len(),
                dst.as_mut_ptr() as *mut c_void,
            );
            assert_eq!(status, noErr);

            assert_eq!(
                crate::runtime::vtf_data_copy_bytes() - bytes_before,
                dst.len() as u64,
                "CopyDataBytes must register exactly the requested length"
            );
            assert_eq!(
                crate::runtime::vtf_data_copy_events() - events_before,
                1,
                "CopyDataBytes is one event"
            );

            crate::runtime::CFRelease(bb);
        }
    }

    #[test]
    fn data_copy_counter_does_not_tick_for_null_source_construction() {
        // CMBlockBufferCreateWithMemoryBlock with `memoryBlock == NULL`
        // allocates zeroed bytes and skips the source copy entirely.
        // The counter must reflect that.
        let _guard = data_copy_lock().lock().unwrap();
        unsafe {
            let bytes_before = crate::runtime::vtf_data_copy_bytes();
            let events_before = crate::runtime::vtf_data_copy_events();

            let mut out: CMBlockBufferRef = ptr::null();
            let status = CMBlockBufferCreateWithMemoryBlock(
                ptr::null(),
                ptr::null_mut(), // memoryBlock = NULL → zeroed allocation, no copy
                64,
                ptr::null(),
                ptr::null(),
                0,
                64,
                0,
                &mut out,
            );
            assert_eq!(status, noErr);
            assert!(!out.is_null());

            assert_eq!(
                crate::runtime::vtf_data_copy_bytes() - bytes_before,
                0,
                "NULL-source construction must not register a defensive copy"
            );
            assert_eq!(
                crate::runtime::vtf_data_copy_events() - events_before,
                0
            );

            crate::runtime::CFRelease(out);
        }
    }

    /// Phase 15: AVCC config-record parser. FFmpeg's
    /// `videotoolbox_decoder_config_create` packages SPS/PPS
    /// into a config record under the
    /// `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`
    /// → `"avcC"` extension. Without parsing it,
    /// `VTDecompressionSessionCreate` couldn't ship parameter
    /// sets to the worker and FFmpeg fell back to libavcodec.
    #[test]
    fn parse_avcc_extracts_sps_pps_in_order() {
        // Synthetic AVCC: configurationVersion=1,
        // profile/compat/level=0x42 0x00 0x1F (Constrained
        // Baseline 3.1), lengthSizeMinusOne=3 (so 4-byte
        // prefixes), 1 SPS + 1 PPS.
        let sps = [0x67u8, 0x42, 0xc0, 0x1f, 0x95, 0xa0, 0x14, 0x01];
        let pps = [0x68u8, 0xce, 0x06, 0xe2];
        let mut record = vec![1u8, 0x42, 0x00, 0x1f, 0xff, 0xe1];
        record.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        record.extend_from_slice(&sps);
        record.push(0x01); // numOfPictureParameterSets
        record.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        record.extend_from_slice(&pps);

        let (sets, length_size) =
            super::vtf_parse_avcc_config_record(&record).expect("parser should accept record");
        assert_eq!(length_size, 4);
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0], &sps);
        assert_eq!(sets[1], &pps);
    }

    #[test]
    fn parse_avcc_picks_first_when_multiple_sps() {
        // Some encoders pack multiple SPS / PPS — we accept
        // them but pick the first of each, matching what
        // `CMVideoFormatDescriptionCreateFromH264ParameterSets`
        // does internally with the first index of each.
        let sps_a = [0x67u8, 0xaa];
        let sps_b = [0x67u8, 0xbb, 0xbb];
        let pps_a = [0x68u8, 0xcc];
        let pps_b = [0x68u8, 0xdd, 0xdd];
        let mut record = vec![1u8, 0x42, 0x00, 0x1f, 0xff, 0xe2];
        record.extend_from_slice(&(sps_a.len() as u16).to_be_bytes());
        record.extend_from_slice(&sps_a);
        record.extend_from_slice(&(sps_b.len() as u16).to_be_bytes());
        record.extend_from_slice(&sps_b);
        record.push(0x02);
        record.extend_from_slice(&(pps_a.len() as u16).to_be_bytes());
        record.extend_from_slice(&pps_a);
        record.extend_from_slice(&(pps_b.len() as u16).to_be_bytes());
        record.extend_from_slice(&pps_b);

        let (sets, _) = super::vtf_parse_avcc_config_record(&record).unwrap();
        assert_eq!(sets[0], &sps_a);
        assert_eq!(sets[1], &pps_a);
    }

    #[test]
    fn parse_avcc_rejects_truncated_record() {
        let truncated = vec![1u8, 0x42, 0x00, 0x1f]; // 4 bytes, < 7 minimum
        assert!(super::vtf_parse_avcc_config_record(&truncated).is_none());
    }

    #[test]
    fn parse_avcc_rejects_wrong_version() {
        let wrong = vec![2u8, 0x42, 0x00, 0x1f, 0xff, 0xe1, 0x00, 0x00];
        assert!(super::vtf_parse_avcc_config_record(&wrong).is_none());
    }

    #[test]
    fn parse_hvcc_extracts_vps_sps_pps_in_order() {
        // Synthetic HVCC: 22-byte fixed header + 1-byte
        // numOfArrays, then 3 arrays (VPS=32, SPS=33, PPS=34).
        let vps = [0x40u8, 0x01, 0x0c, 0x01];
        let sps = [0x42u8, 0x01, 0x01, 0x01, 0x60];
        let pps = [0x44u8, 0x01, 0xc0];

        let mut record = vec![0u8; 22];
        record[0] = 1; // configurationVersion
        record[21] = 0xff; // lengthSizeMinusOne(2 bits) = 3 → 4-byte prefixes
        record.push(0x03); // numOfArrays

        for (nal_type, nal) in [(32u8, &vps[..]), (33u8, &sps[..]), (34u8, &pps[..])] {
            record.push(nal_type & 0x3F);
            record.extend_from_slice(&1u16.to_be_bytes()); // numNalus = 1
            record.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            record.extend_from_slice(nal);
        }

        let (sets, length_size) =
            super::vtf_parse_hvcc_config_record(&record).expect("hvcc parser should accept");
        assert_eq!(length_size, 4);
        assert_eq!(sets[0], &vps);
        assert_eq!(sets[1], &sps);
        assert_eq!(sets[2], &pps);
    }

    #[test]
    fn parse_hvcc_returns_none_when_missing_required_array() {
        // VPS + SPS but no PPS array → `?` on `pps?` returns None.
        let vps = [0x40u8, 0x01];
        let sps = [0x42u8, 0x01];
        let mut record = vec![0u8; 22];
        record[0] = 1;
        record[21] = 0xff;
        record.push(0x02);
        for (nal_type, nal) in [(32u8, &vps[..]), (33u8, &sps[..])] {
            record.push(nal_type & 0x3F);
            record.extend_from_slice(&1u16.to_be_bytes());
            record.extend_from_slice(&(nal.len() as u16).to_be_bytes());
            record.extend_from_slice(nal);
        }
        assert!(super::vtf_parse_hvcc_config_record(&record).is_none());
    }
}

pub unsafe fn vtf_create_sample_buffer(
    data_buffer: CMBlockBufferRef,
    format_description: CMFormatDescriptionRef,
    pts: CMTime,
    dts: CMTime,
    total_sample_size: usize,
    sample_flags: u32,
    output_id: u64,
    source_image_buffer: CFTypeRef,
) -> CMSampleBufferRef {
    let not_sync_value = if (sample_flags & VTF_ENCODE_OUTPUT_FLAG_SYNC) != 0 {
        kCFBooleanFalse.0
    } else {
        kCFBooleanTrue.0
    };
    let attachment_key = [kCMSampleAttachmentKey_NotSync.0];
    let attachment_value = [not_sync_value];
    let attachment = crate::corefoundation::CFDictionaryCreate(
        ptr::null(),
        attachment_key.as_ptr(),
        attachment_value.as_ptr(),
        1,
        &raw const kCFTypeDictionaryKeyCallBacks,
        &raw const kCFTypeDictionaryValueCallBacks,
    );
    let attachment_values = [attachment];
    let attachments: CFArrayRef = crate::corefoundation::CFArrayCreate(
        ptr::null(),
        attachment_values.as_ptr(),
        1,
        &raw const kCFTypeArrayCallBacks,
    );
    crate::runtime::CFRelease(attachment);

    Box::into_raw(Box::new(vtf_cm_sample_buffer {
        base: vtf_cf_object::init(VTF_TYPE_SAMPLE_BUFFER, Some(vtf_finalize_sample_buffer)),
        data_buffer: crate::runtime::CFRetain(data_buffer),
        format_description: crate::runtime::CFRetain(format_description),
        decode_timestamp: dts,
        presentation_timestamp: pts,
        total_sample_size,
        attachments,
        source_image_buffer: crate::runtime::CFRetain(source_image_buffer),
        output_id,
    })) as CMSampleBufferRef
}

/// Apple-named entrypoint FFmpeg's videotoolbox decoder uses to
/// build a `CMVideoFormatDescription` from parsed H.264 SPS/PPS
/// bytes. Without this symbol, FFmpeg can't reach
/// `VTDecompressionSessionCreate`.
///
/// The internal `vtf_create_video_format_description` helper
/// already handles the proxy construction; we just translate
/// Apple's array-of-pointers calling convention into the
/// concatenated-bytes shape that helper expects.
#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
    _allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes_ptr: *const usize,
    nal_unit_header_length: i32,
    format_description_out: *mut CMFormatDescriptionRef,
) -> i32 {
    vtf_format_description_from_parameter_sets(
        VTF_VIDEO_CODEC_H264,
        parameter_set_count,
        parameter_set_pointers,
        parameter_set_sizes_ptr,
        nal_unit_header_length,
        format_description_out,
    )
}

/// HEVC counterpart. Same calling convention; the Apple API just
/// adds an extensions dictionary parameter we currently ignore
/// (FFmpeg's hevc_videotoolbox decoder passes `NULL`).
#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
    _allocator: CFAllocatorRef,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes_ptr: *const usize,
    nal_unit_header_length: i32,
    _extensions: CFTypeRef,
    format_description_out: *mut CMFormatDescriptionRef,
) -> i32 {
    vtf_format_description_from_parameter_sets(
        VTF_VIDEO_CODEC_HEVC,
        parameter_set_count,
        parameter_set_pointers,
        parameter_set_sizes_ptr,
        nal_unit_header_length,
        format_description_out,
    )
}

unsafe fn vtf_format_description_from_parameter_sets(
    codec_type: u32,
    parameter_set_count: usize,
    parameter_set_pointers: *const *const u8,
    parameter_set_sizes_ptr: *const usize,
    nal_unit_header_length: i32,
    format_description_out: *mut CMFormatDescriptionRef,
) -> i32 {
    if format_description_out.is_null() {
        return -12710; // kCMFormatDescriptionError_InvalidParameter
    }
    *format_description_out = ptr::null();
    if parameter_set_pointers.is_null() || parameter_set_sizes_ptr.is_null() {
        return -12710;
    }
    if parameter_set_count == 0 || parameter_set_count > 4 {
        return -12710;
    }

    // Pack parameter sets into the concatenated-bytes shape that
    // vtf_create_video_format_description expects. Worker side
    // parses the parameter sets to derive width / height; we
    // leave guest-side dimensions at 0.
    let mut sizes_u32: [u32; 4] = [0; 4];
    let mut total_bytes = 0usize;
    for i in 0..parameter_set_count {
        let size = *parameter_set_sizes_ptr.add(i);
        if size == 0 || size > u32::MAX as usize {
            return -12710;
        }
        sizes_u32[i] = size as u32;
        total_bytes = match total_bytes.checked_add(size) {
            Some(t) => t,
            None => return -12710,
        };
    }
    let mut concatenated = Vec::with_capacity(total_bytes);
    for i in 0..parameter_set_count {
        let size = *parameter_set_sizes_ptr.add(i);
        let src = *parameter_set_pointers.add(i);
        if src.is_null() {
            return -12710;
        }
        let slice = std::slice::from_raw_parts(src, size);
        concatenated.extend_from_slice(slice);
    }

    *format_description_out = vtf_create_video_format_description(
        codec_type,
        0, // width — worker derives from SPS
        0, // height — worker derives from SPS
        parameter_set_count,
        &sizes_u32,
        &concatenated,
        nal_unit_header_length,
    );
    0
}

/// `CMSampleBufferCreate` — same shape as `CMSampleBufferCreateReady`
/// plus a `dataReady` flag and a make-data-ready callback. FFmpeg's
/// videotoolbox.c calls this for decode-input sample buffers with
/// `dataReady = TRUE` and no callback (data is already in the
/// block buffer). Just delegate to CreateReady — the
/// makeDataReady callback path isn't exercised on the vt-ferry
/// decode side.
#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferCreate(
    allocator: CFTypeRef,
    data_buffer: CMBlockBufferRef,
    _data_ready: i32,
    _make_data_ready_callback: *const c_void,
    _make_data_ready_refcon: *mut c_void,
    format_description: CMFormatDescriptionRef,
    num_samples: CMItemCount,
    num_sample_timing_entries: CMItemCount,
    sample_timing_array: *const CMSampleTimingInfo,
    num_sample_size_entries: CMItemCount,
    sample_size_array: *const usize,
    sample_buffer_out: *mut CMSampleBufferRef,
) -> i32 {
    CMSampleBufferCreateReady(
        allocator,
        data_buffer,
        format_description,
        num_samples,
        num_sample_timing_entries,
        sample_timing_array,
        num_sample_size_entries,
        sample_size_array,
        sample_buffer_out,
    )
}

/// `CMVideoFormatDescriptionCreate` — generic format description
/// constructor that takes width/height + extensions dictionary.
/// FFmpeg's videotoolbox.c calls this to build a format
/// description for codecs where parameter sets aren't extracted
/// (e.g. MPEG2). For our v1 decode path (h264 / hevc), the
/// parameter-set entrypoints are the primary path. FFmpeg's
/// `-hwaccel videotoolbox` path also lands here — but instead of
/// passing parameter sets explicitly, FFmpeg packages them in an
/// AVCC/HVCC config record under the conventional Apple
/// extensions dictionary key
/// `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`
/// → `"avcC"` / `"hvcC"` → `CFData`. We unpack that here so the
/// resulting format description carries real parameter sets and
/// downstream `VTDecompressionSessionCreate` can ship them
/// through `OP_SET_DECODE_FORMAT` exactly like the explicit-
/// parameter-set path does.
#[no_mangle]
pub unsafe extern "C" fn CMVideoFormatDescriptionCreate(
    _allocator: CFTypeRef,
    codec_type: u32,
    width: i32,
    height: i32,
    extensions: CFDictionaryRef,
    format_description_out: *mut CMFormatDescriptionRef,
) -> i32 {
    if format_description_out.is_null() {
        return -12710;
    }
    *format_description_out = ptr::null();

    // Pull SPS/PPS (or VPS+SPS+PPS for HEVC) out of
    // `extensions[SampleDescriptionExtensionAtoms]["avcC"|"hvcC"]`
    // if present. Failure here is non-fatal — callers passing
    // null extensions or an MPEG2-style format description still
    // get a usable placeholder.
    let mut nal_header_length: i32 = 0;
    let mut param_blobs: Vec<Vec<u8>> = Vec::new();
    if !extensions.is_null() {
        if let Some((extracted, lp)) =
            vtf_extract_param_sets_from_extensions(extensions, codec_type)
        {
            param_blobs = extracted;
            nal_header_length = lp;
        }
    }

    // Convert each parameter-set Vec<u8> into a Box<[u8]> heap
    // allocation so the format description owns the bytes
    // independently of the source CFData (which gets dropped on
    // CFRelease(extensions)). The finalizer
    // `vtf_finalize_format_description` walks `parameter_sets`
    // and reconstitutes each as a Box for proper teardown.
    let mut parameter_sets: [*mut u8; 4] = [ptr::null_mut(); 4];
    let mut parameter_set_sizes: [usize; 4] = [0; 4];
    let parameter_set_count = param_blobs.len().min(4);
    for (i, blob) in param_blobs.into_iter().take(4).enumerate() {
        let len = blob.len();
        let boxed: Box<[u8]> = blob.into_boxed_slice();
        parameter_sets[i] = Box::into_raw(boxed) as *mut u8;
        parameter_set_sizes[i] = len;
    }

    let fd = Box::new(vtf_cm_format_description {
        base: vtf_cf_object::init(
            VTF_TYPE_FORMAT_DESCRIPTION,
            Some(vtf_finalize_format_description),
        ),
        codec_type,
        width,
        height,
        extensions: if extensions.is_null() {
            ptr::null()
        } else {
            crate::runtime::CFRetain(extensions)
        },
        nal_header_length,
        parameter_set_count,
        parameter_set_sizes,
        parameter_sets,
    });
    *format_description_out = Box::into_raw(fd) as CMFormatDescriptionRef;
    0
}

/// Look up the AVCC/HVCC config record blob in the extensions
/// dictionary FFmpeg's hwaccel videotoolbox builds, parse it,
/// and return the SPS/PPS (H.264) or VPS+SPS+PPS (HEVC) bytes
/// in worker-expected order plus the AVCC/HVCC
/// `lengthSizeMinusOne + 1` (NAL length-prefix size).
///
/// Layout we walk:
///   `extensions: CFDictionary {
///        kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms:
///            CFDictionary { "avcC" | "hvcC": CFData(...) }
///    }`
///
/// AVCC config record (ISO/IEC 14496-15 § 5.2.4.1):
///   u8 configurationVersion (= 1)
///   u8 AVCProfileIndication
///   u8 profile_compatibility
///   u8 AVCLevelIndication
///   u8 reserved(6) | lengthSizeMinusOne(2)
///   u8 reserved(3) | numOfSequenceParameterSets(5)
///   for each SPS: u16_be sps_len + sps_bytes
///   u8 numOfPictureParameterSets
///   for each PPS: u16_be pps_len + pps_bytes
///
/// HVCC config record is more elaborate (ISO/IEC 14496-15 §
/// 8.3.3.1) but the head is fixed-shape and the parameter-set
/// arrays are at a known offset (23 bytes in).
unsafe fn vtf_extract_param_sets_from_extensions(
    extensions: CFDictionaryRef,
    codec_type: u32,
) -> Option<(Vec<Vec<u8>>, i32)> {
    const FOURCC_AVC1: u32 = 0x6176_6331;
    const FOURCC_HVC1: u32 = 0x6876_6331;

    let atoms = crate::corefoundation::CFDictionaryGetValue(
        extensions,
        &raw const vtf_kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms_storage
            as *const c_void,
    ) as CFDictionaryRef;
    if atoms.is_null() {
        return None;
    }

    let want_key: &[u8] = match codec_type {
        FOURCC_AVC1 => b"avcC",
        FOURCC_HVC1 => b"hvcC",
        _ => return None,
    };

    // Walk the inner dictionary's keys and find the one whose
    // bytes match the expected codec identifier. We iterate
    // explicitly because the keys come from `CFSTR(...)` calls
    // and the underlying byte slice is what we care about, not
    // pointer identity (different CFSTR call sites produce
    // distinct CFStringRefs even for the same literal).
    let inner = atoms as *const crate::corefoundation::vtf_cf_dictionary;
    let inner_ref = &*inner;
    let mut data_ref: CFTypeRef = ptr::null();
    for (i, key) in inner_ref.keys.iter().enumerate() {
        let key_ref = *key;
        if key_ref.is_null() || vtf_get_type_id(key_ref) != VTF_TYPE_STRING {
            continue;
        }
        let key_string = key_ref as *const vtf_cf_string;
        let key_bytes = std::slice::from_raw_parts(
            (*key_string).bytes,
            (*key_string).length,
        );
        if key_bytes == want_key {
            data_ref = inner_ref.values[i];
            break;
        }
    }
    if data_ref.is_null() || vtf_get_type_id(data_ref) != VTF_TYPE_DATA {
        return None;
    }
    let data = data_ref as *const crate::corefoundation::vtf_cf_data;
    let bytes: &[u8] = (*data).bytes.as_slice();

    if codec_type == FOURCC_AVC1 {
        vtf_parse_avcc_config_record(bytes)
    } else {
        vtf_parse_hvcc_config_record(bytes)
    }
}

/// Parse an AVCC config record into [SPS, PPS] (worker-expected
/// order) plus length-size. Tolerant of records with multiple
/// SPS/PPS entries — picks the first of each, which matches
/// what FFmpeg's avcc extradata writer emits and what
/// `CMVideoFormatDescriptionCreateFromH264ParameterSets` does
/// when given an extradata blob.
fn vtf_parse_avcc_config_record(bytes: &[u8]) -> Option<(Vec<Vec<u8>>, i32)> {
    if bytes.len() < 7 || bytes[0] != 1 {
        return None;
    }
    let length_size = (bytes[4] & 0x03) + 1; // lengthSizeMinusOne + 1
    let num_sps = (bytes[5] & 0x1F) as usize;
    if num_sps == 0 {
        return None;
    }
    let mut offset = 6usize;
    let mut sps: Option<Vec<u8>> = None;
    for _ in 0..num_sps {
        if offset + 2 > bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;
        if offset + len > bytes.len() {
            return None;
        }
        if sps.is_none() {
            sps = Some(bytes[offset..offset + len].to_vec());
        }
        offset += len;
    }
    if offset >= bytes.len() {
        return None;
    }
    let num_pps = bytes[offset] as usize;
    offset += 1;
    if num_pps == 0 {
        return None;
    }
    let mut pps: Option<Vec<u8>> = None;
    for _ in 0..num_pps {
        if offset + 2 > bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;
        if offset + len > bytes.len() {
            return None;
        }
        if pps.is_none() {
            pps = Some(bytes[offset..offset + len].to_vec());
        }
        offset += len;
    }
    Some((vec![sps?, pps?], length_size as i32))
}

/// Parse an HVCC config record into [VPS, SPS, PPS] (worker-
/// expected order) plus length-size. HVCC has a 22-byte header
/// followed by a `numOfArrays` byte and then per-array entries
/// keyed by NAL-type. We pull the first VPS / SPS / PPS we
/// encounter (NAL types 32, 33, 34).
fn vtf_parse_hvcc_config_record(bytes: &[u8]) -> Option<(Vec<Vec<u8>>, i32)> {
    if bytes.len() < 23 || bytes[0] != 1 {
        return None;
    }
    let length_size = (bytes[21] & 0x03) + 1;
    let num_arrays = bytes[22] as usize;
    let mut offset = 23usize;
    let mut vps: Option<Vec<u8>> = None;
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    for _ in 0..num_arrays {
        if offset + 3 > bytes.len() {
            return None;
        }
        let nal_unit_type = bytes[offset] & 0x3F;
        let num_nalus = u16::from_be_bytes([bytes[offset + 1], bytes[offset + 2]]) as usize;
        offset += 3;
        for _ in 0..num_nalus {
            if offset + 2 > bytes.len() {
                return None;
            }
            let len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
            offset += 2;
            if offset + len > bytes.len() {
                return None;
            }
            let blob = bytes[offset..offset + len].to_vec();
            offset += len;
            match nal_unit_type {
                32 if vps.is_none() => vps = Some(blob),
                33 if sps.is_none() => sps = Some(blob),
                34 if pps.is_none() => pps = Some(blob),
                _ => {}
            }
        }
    }
    Some((vec![vps?, sps?, pps?], length_size as i32))
}

/// Apple-named entrypoint that FFmpeg's videotoolbox decoder
/// calls to construct an input CMSampleBuffer wrapping a parsed
/// encoded packet. Builds an `vtf_cm_sample_buffer` proxy
/// retaining the data buffer + format description + timing info.
///
/// Multi-sample buffers (`numSamples > 1`) get treated as
/// single-sample for v1 — FFmpeg's decoder always passes
/// `numSamples = 1` for video. The first timing entry + first
/// size entry are used; trailing entries are ignored. If a real
/// caller needs multi-sample semantics, this fn returns
/// `kCMSampleBufferError_RequiredParameterMissing` (-12731) and
/// future work expands handling.
#[no_mangle]
pub unsafe extern "C" fn CMSampleBufferCreateReady(
    _allocator: CFTypeRef,
    data_buffer: CMBlockBufferRef,
    format_description: CMFormatDescriptionRef,
    num_samples: CMItemCount,
    num_sample_timing_entries: CMItemCount,
    sample_timing_array: *const CMSampleTimingInfo,
    num_sample_size_entries: CMItemCount,
    sample_size_array: *const usize,
    sample_buffer_out: *mut CMSampleBufferRef,
) -> i32 {
    if sample_buffer_out.is_null() {
        return -12731; // kCMSampleBufferError_RequiredParameterMissing
    }
    *sample_buffer_out = ptr::null();
    if data_buffer.is_null() || format_description.is_null() {
        return -12731;
    }
    if num_samples != 1 {
        // FFmpeg's video decode path always passes 1; multi-sample
        // is audio-shaped and not yet supported here.
        crate::runtime::vtf_guest_trace(&format!(
            "coremedia:CMSampleBufferCreateReady num_samples={} != 1 \
             — only single-sample buffers supported for v1",
            num_samples
        ));
        return -12731;
    }

    // Default to invalid CMTime + total_sample_size from the
    // block buffer's data length when timing/size arrays are
    // empty.
    let pts = if num_sample_timing_entries > 0 && !sample_timing_array.is_null() {
        (*sample_timing_array).presentationTimeStamp
    } else {
        CMTime {
            value: 0,
            timescale: 0,
            flags: 0,
            epoch: 0,
        }
    };
    let dts = if num_sample_timing_entries > 0 && !sample_timing_array.is_null() {
        (*sample_timing_array).decodeTimeStamp
    } else {
        CMTime {
            value: 0,
            timescale: 0,
            flags: 0,
            epoch: 0,
        }
    };
    let total_sample_size = if num_sample_size_entries > 0 && !sample_size_array.is_null() {
        *sample_size_array
    } else {
        // Fall back to the block buffer's reported length.
        let block = data_buffer as *const vtf_cm_block_buffer;
        (*block).length
    };

    *sample_buffer_out = vtf_create_sample_buffer(
        data_buffer,
        format_description,
        pts,
        dts,
        total_sample_size,
        VTF_ENCODE_OUTPUT_FLAG_SYNC, // assume sync; FFmpeg sets via attachments later
        0,
        ptr::null(),
    );
    0
}
