use crate::corefoundation::*;
use crate::runtime::*;
use bytemuck::Zeroable;
use std::ffi::c_void;
use std::slice;
use std::time::{Duration, Instant};

const VTF_PROPERTY_VALUE_KIND_BOOLEAN: u32 = 1;
const VTF_PROPERTY_VALUE_KIND_NUMBER: u32 = 2;
const VTF_PROPERTY_VALUE_KIND_STRING: u32 = 3;
const VTF_PROPERTY_VALUE_KIND_ARRAY_I64_PAIR: u32 = 4;
const VTF_PROPERTY_VALUE_KIND_DICT_I64_PAIR: u32 = 5;
const VTF_COMPLETE_FRAMES_WAIT_MS_DEFAULT: u64 = 5_000;
const VTF_COMPLETE_FRAMES_POLL_INTERVAL_MS: u64 = 10;
const VTF_OUTPUT_POLL_INTERVAL_DEFAULT: usize = 4;
const VTF_OUTPUT_BATCH_SIZE_DEFAULT: usize = vt_ferry_protocol::VTF_TRANSPORT_MAX_OUTPUT_BATCH;
// Default pool-pressure wait used by vtf_deliver_outputs_for_pool_pressure;
// the helper is currently gated dead-code-on-purpose, see the
// comment on that fn.
#[allow(dead_code)]
const VTF_POOL_PRESSURE_WAIT_MS_DEFAULT: u64 = 5_000;

macro_rules! define_string_key {
    ($name:ident, $literal:expr) => {
        static mut $name: vtf_cf_string = vtf_cf_string {
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
            bytes: $literal.as_ptr(),
            length: $literal.len() - 1,
            owns_bytes: false,
        };
    };
}

macro_rules! export_string_key {
    ($name:ident, $storage:ident) => {
        #[no_mangle]
        pub static $name: crate::coremedia::ExportedSymbol =
            crate::coremedia::ExportedSymbol(
                &raw const $storage as *const _ as *const c_void,
            );
    };
}

define_string_key!(
    vtf_kVTCompressionPropertyKey_RealTime_storage,
    b"RealTime\0"
);
export_string_key!(
    kVTCompressionPropertyKey_RealTime,
    vtf_kVTCompressionPropertyKey_RealTime_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_ProfileLevel_storage,
    b"ProfileLevel\0"
);
export_string_key!(
    kVTCompressionPropertyKey_ProfileLevel,
    vtf_kVTCompressionPropertyKey_ProfileLevel_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_AverageBitRate_storage,
    b"AverageBitRate\0"
);
export_string_key!(
    kVTCompressionPropertyKey_AverageBitRate,
    vtf_kVTCompressionPropertyKey_AverageBitRate_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_ColorPrimaries_storage,
    b"ColorPrimaries\0"
);
export_string_key!(
    kVTCompressionPropertyKey_ColorPrimaries,
    vtf_kVTCompressionPropertyKey_ColorPrimaries_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_ExpectedFrameRate_storage,
    b"ExpectedFrameRate\0"
);
export_string_key!(
    kVTCompressionPropertyKey_ExpectedFrameRate,
    vtf_kVTCompressionPropertyKey_ExpectedFrameRate_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MaxKeyFrameInterval_storage,
    b"MaxKeyFrameInterval\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MaxKeyFrameInterval,
    vtf_kVTCompressionPropertyKey_MaxKeyFrameInterval_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MoreFramesAfterEnd_storage,
    b"MoreFramesAfterEnd\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MoreFramesAfterEnd,
    vtf_kVTCompressionPropertyKey_MoreFramesAfterEnd_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MoreFramesBeforeStart_storage,
    b"MoreFramesBeforeStart\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MoreFramesBeforeStart,
    vtf_kVTCompressionPropertyKey_MoreFramesBeforeStart_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_AllowFrameReordering_storage,
    b"AllowFrameReordering\0"
);
export_string_key!(
    kVTCompressionPropertyKey_AllowFrameReordering,
    vtf_kVTCompressionPropertyKey_AllowFrameReordering_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_DataRateLimits_storage,
    b"DataRateLimits\0"
);
export_string_key!(
    kVTCompressionPropertyKey_DataRateLimits,
    vtf_kVTCompressionPropertyKey_DataRateLimits_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_H264EntropyMode_storage,
    b"H264EntropyMode\0"
);
export_string_key!(
    kVTCompressionPropertyKey_H264EntropyMode,
    vtf_kVTCompressionPropertyKey_H264EntropyMode_storage
);

define_string_key!(vtf_kVTH264EntropyMode_CABAC_storage, b"CABAC\0");
export_string_key!(
    kVTH264EntropyMode_CABAC,
    vtf_kVTH264EntropyMode_CABAC_storage
);

define_string_key!(vtf_kVTH264EntropyMode_CAVLC_storage, b"CAVLC\0");
export_string_key!(
    kVTH264EntropyMode_CAVLC,
    vtf_kVTH264EntropyMode_CAVLC_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_PixelAspectRatio_storage,
    b"PixelAspectRatio\0"
);
export_string_key!(
    kVTCompressionPropertyKey_PixelAspectRatio,
    vtf_kVTCompressionPropertyKey_PixelAspectRatio_storage
);

define_string_key!(vtf_kVTCompressionPropertyKey_Quality_storage, b"Quality\0");
export_string_key!(
    kVTCompressionPropertyKey_Quality,
    vtf_kVTCompressionPropertyKey_Quality_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_TransferFunction_storage,
    b"TransferFunction\0"
);
export_string_key!(
    kVTCompressionPropertyKey_TransferFunction,
    vtf_kVTCompressionPropertyKey_TransferFunction_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_YCbCrMatrix_storage,
    b"YCbCrMatrix\0"
);
export_string_key!(
    kVTCompressionPropertyKey_YCbCrMatrix,
    vtf_kVTCompressionPropertyKey_YCbCrMatrix_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_EncoderID_storage,
    b"EncoderID\0"
);
export_string_key!(
    kVTCompressionPropertyKey_EncoderID,
    vtf_kVTCompressionPropertyKey_EncoderID_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MaxH264SliceBytes_storage,
    b"MaxH264SliceBytes\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MaxH264SliceBytes,
    vtf_kVTCompressionPropertyKey_MaxH264SliceBytes_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_ConstantBitRate_storage,
    b"ConstantBitRate\0"
);
export_string_key!(
    kVTCompressionPropertyKey_ConstantBitRate,
    vtf_kVTCompressionPropertyKey_ConstantBitRate_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_TargetQualityForAlpha_storage,
    b"TargetQualityForAlpha\0"
);
export_string_key!(
    kVTCompressionPropertyKey_TargetQualityForAlpha,
    vtf_kVTCompressionPropertyKey_TargetQualityForAlpha_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality_storage,
    b"PrioritizeEncodingSpeedOverQuality\0"
);
export_string_key!(
    kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
    vtf_kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_AllowOpenGOP_storage,
    b"AllowOpenGOP\0"
);
export_string_key!(
    kVTCompressionPropertyKey_AllowOpenGOP,
    vtf_kVTCompressionPropertyKey_AllowOpenGOP_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MaximizePowerEfficiency_storage,
    b"MaximizePowerEfficiency\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MaximizePowerEfficiency,
    vtf_kVTCompressionPropertyKey_MaximizePowerEfficiency_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_SpatialAdaptiveQPLevel_storage,
    b"SpatialAdaptiveQPLevel\0"
);
export_string_key!(
    kVTCompressionPropertyKey_SpatialAdaptiveQPLevel,
    vtf_kVTCompressionPropertyKey_SpatialAdaptiveQPLevel_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_ReferenceBufferCount_storage,
    b"ReferenceBufferCount\0"
);
export_string_key!(
    kVTCompressionPropertyKey_ReferenceBufferCount,
    vtf_kVTCompressionPropertyKey_ReferenceBufferCount_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MaxAllowedFrameQP_storage,
    b"MaxAllowedFrameQP\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MaxAllowedFrameQP,
    vtf_kVTCompressionPropertyKey_MaxAllowedFrameQP_storage
);

define_string_key!(
    vtf_kVTCompressionPropertyKey_MinAllowedFrameQP_storage,
    b"MinAllowedFrameQP\0"
);
export_string_key!(
    kVTCompressionPropertyKey_MinAllowedFrameQP,
    vtf_kVTCompressionPropertyKey_MinAllowedFrameQP_storage
);

define_string_key!(
    vtf_kVTVideoEncoderSpecification_EncoderID_storage,
    b"EncoderID\0"
);
export_string_key!(
    kVTVideoEncoderSpecification_EncoderID,
    vtf_kVTVideoEncoderSpecification_EncoderID_storage
);

define_string_key!(
    vtf_kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder_storage,
    b"EnableHardwareAcceleratedVideoEncoder\0"
);
export_string_key!(
    kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder,
    vtf_kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder_storage
);

define_string_key!(
    vtf_kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder_storage,
    b"RequireHardwareAcceleratedVideoEncoder\0"
);
export_string_key!(
    kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder,
    vtf_kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder_storage
);

define_string_key!(
    vtf_kVTVideoEncoderSpecification_EnableLowLatencyRateControl_storage,
    b"EnableLowLatencyRateControl\0"
);
export_string_key!(
    kVTVideoEncoderSpecification_EnableLowLatencyRateControl,
    vtf_kVTVideoEncoderSpecification_EnableLowLatencyRateControl_storage
);

define_string_key!(
    vtf_kVTEncodeFrameOptionKey_ForceKeyFrame_storage,
    b"ForceKeyFrame\0"
);
export_string_key!(
    kVTEncodeFrameOptionKey_ForceKeyFrame,
    vtf_kVTEncodeFrameOptionKey_ForceKeyFrame_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_1_3_storage,
    b"H264_Baseline_1_3\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_1_3,
    vtf_kVTProfileLevel_H264_Baseline_1_3_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_3_0_storage,
    b"H264_Baseline_3_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_3_0,
    vtf_kVTProfileLevel_H264_Baseline_3_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_3_1_storage,
    b"H264_Baseline_3_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_3_1,
    vtf_kVTProfileLevel_H264_Baseline_3_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_3_2_storage,
    b"H264_Baseline_3_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_3_2,
    vtf_kVTProfileLevel_H264_Baseline_3_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_4_0_storage,
    b"H264_Baseline_4_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_4_0,
    vtf_kVTProfileLevel_H264_Baseline_4_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_4_1_storage,
    b"H264_Baseline_4_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_4_1,
    vtf_kVTProfileLevel_H264_Baseline_4_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_4_2_storage,
    b"H264_Baseline_4_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_4_2,
    vtf_kVTProfileLevel_H264_Baseline_4_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_5_0_storage,
    b"H264_Baseline_5_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_5_0,
    vtf_kVTProfileLevel_H264_Baseline_5_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_5_1_storage,
    b"H264_Baseline_5_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_5_1,
    vtf_kVTProfileLevel_H264_Baseline_5_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_5_2_storage,
    b"H264_Baseline_5_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_5_2,
    vtf_kVTProfileLevel_H264_Baseline_5_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Baseline_AutoLevel_storage,
    b"H264_Baseline_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_Baseline_AutoLevel,
    vtf_kVTProfileLevel_H264_Baseline_AutoLevel_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_3_0_storage,
    b"H264_Main_3_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_3_0,
    vtf_kVTProfileLevel_H264_Main_3_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_3_1_storage,
    b"H264_Main_3_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_3_1,
    vtf_kVTProfileLevel_H264_Main_3_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_3_2_storage,
    b"H264_Main_3_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_3_2,
    vtf_kVTProfileLevel_H264_Main_3_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_4_0_storage,
    b"H264_Main_4_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_4_0,
    vtf_kVTProfileLevel_H264_Main_4_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_4_1_storage,
    b"H264_Main_4_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_4_1,
    vtf_kVTProfileLevel_H264_Main_4_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_4_2_storage,
    b"H264_Main_4_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_4_2,
    vtf_kVTProfileLevel_H264_Main_4_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_5_0_storage,
    b"H264_Main_5_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_5_0,
    vtf_kVTProfileLevel_H264_Main_5_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_5_1_storage,
    b"H264_Main_5_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_5_1,
    vtf_kVTProfileLevel_H264_Main_5_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_5_2_storage,
    b"H264_Main_5_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_5_2,
    vtf_kVTProfileLevel_H264_Main_5_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Main_AutoLevel_storage,
    b"H264_Main_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_Main_AutoLevel,
    vtf_kVTProfileLevel_H264_Main_AutoLevel_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_3_0_storage,
    b"H264_High_3_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_3_0,
    vtf_kVTProfileLevel_H264_High_3_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_3_1_storage,
    b"H264_High_3_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_3_1,
    vtf_kVTProfileLevel_H264_High_3_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_3_2_storage,
    b"H264_High_3_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_3_2,
    vtf_kVTProfileLevel_H264_High_3_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_4_0_storage,
    b"H264_High_4_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_4_0,
    vtf_kVTProfileLevel_H264_High_4_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_4_1_storage,
    b"H264_High_4_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_4_1,
    vtf_kVTProfileLevel_H264_High_4_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_4_2_storage,
    b"H264_High_4_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_4_2,
    vtf_kVTProfileLevel_H264_High_4_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_5_0_storage,
    b"H264_High_5_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_5_0,
    vtf_kVTProfileLevel_H264_High_5_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_5_1_storage,
    b"H264_High_5_1\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_5_1,
    vtf_kVTProfileLevel_H264_High_5_1_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_5_2_storage,
    b"H264_High_5_2\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_5_2,
    vtf_kVTProfileLevel_H264_High_5_2_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_High_AutoLevel_storage,
    b"H264_High_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_High_AutoLevel,
    vtf_kVTProfileLevel_H264_High_AutoLevel_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Extended_5_0_storage,
    b"H264_Extended_5_0\0"
);
export_string_key!(
    kVTProfileLevel_H264_Extended_5_0,
    vtf_kVTProfileLevel_H264_Extended_5_0_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_Extended_AutoLevel_storage,
    b"H264_Extended_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_Extended_AutoLevel,
    vtf_kVTProfileLevel_H264_Extended_AutoLevel_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel_storage,
    b"H264_ConstrainedBaseline_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel,
    vtf_kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel_storage
);

define_string_key!(
    vtf_kVTProfileLevel_H264_ConstrainedHigh_AutoLevel_storage,
    b"H264_ConstrainedHigh_AutoLevel\0"
);
export_string_key!(
    kVTProfileLevel_H264_ConstrainedHigh_AutoLevel,
    vtf_kVTProfileLevel_H264_ConstrainedHigh_AutoLevel_storage
);

#[repr(C)]
pub struct vtf_vt_session {
    pub base: vtf_cf_object,
    pub width: i32,
    pub height: i32,
    pub codec_type: u32,
    pub pixel_format: u32,
    pub properties: CFDictionaryRef,
    pub source_image_buffer_attributes: CFDictionaryRef,
    pub output_callback:
        Option<unsafe extern "C" fn(*mut c_void, *mut c_void, i32, u32, *mut c_void)>,
    pub output_refcon: *mut c_void,
    pub pixel_buffer_pool: *mut super::corevideo::vtf_cv_pixel_buffer_pool,
    pub prepared: bool,
    pub invalidated: bool,
    pub pending_count: usize,
    pub pending_capacity: usize,
    pub pending_image_buffers: *mut *mut super::corevideo::vtf_cv_pixel_buffer,
    pub pending_source_frame_refcons: *mut *mut c_void,
    pub pending_encode_count: usize,
    pub pending_encode_payloads:
        [vt_ferry_protocol::EncodeFramePayload; vt_ferry_protocol::VTF_TRANSPORT_MAX_ENCODE_BATCH],
    pub stored_property_count: usize,
    pub stored_properties: [vtf_vt_session_property; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct vtf_vt_session_property {
    pub key: [u8; 64],
    pub value: CFTypeRef,
}

fn cfstring_to_string(value: CFStringRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let string = unsafe { &*(value as *const vtf_cf_string) };
    if string.bytes.is_null() {
        return None;
    }
    let bytes = unsafe { slice::from_raw_parts(string.bytes, string.length) };
    String::from_utf8(bytes.to_vec()).ok()
}

fn fill_key_bytes(destination: &mut [u8; 64], key: &str) {
    destination.fill(0);
    let raw = key.as_bytes();
    let len = raw.len().min(destination.len().saturating_sub(1));
    destination[..len].copy_from_slice(&raw[..len]);
}

fn fill_small_key_bytes(destination: &mut [u8; 32], key: &str) {
    destination.fill(0);
    let raw = key.as_bytes();
    let len = raw.len().min(destination.len().saturating_sub(1));
    destination[..len].copy_from_slice(&raw[..len]);
}

unsafe fn vtf_fill_set_property_payload(
    session: *mut vtf_vt_session,
    property_key_ref: CFStringRef,
    property_key: &str,
    property_value: CFTypeRef,
) -> Result<vt_ferry_protocol::SetPropertyPayload, i32> {
    let mut payload = vt_ferry_protocol::SetPropertyPayload::zeroed();
    payload.session_id = vtf_get_host_id(session as CFTypeRef);
    payload.property_key_proxy_id = vtf_get_proxy_id(property_key_ref);
    payload.property_value_proxy_id = vtf_get_proxy_id(property_value);
    fill_key_bytes(&mut payload.property_key, property_key);

    if property_value == kCFBooleanTrue.0 || property_value == kCFBooleanFalse.0 {
        payload.property_value_kind = VTF_PROPERTY_VALUE_KIND_BOOLEAN;
        payload.property_bool = u32::from(property_value == kCFBooleanTrue.0);
        return Ok(payload);
    }

    if CFGetTypeID(property_value) == CFNumberGetTypeID() {
        let number_type = CFNumberGetType(property_value);
        let mut number_value: i64 = 0;
        if CFNumberGetValue(
            property_value,
            K_CF_NUMBER_SINT64_TYPE,
            &mut number_value as *mut i64 as *mut c_void,
        ) == 0
        {
            return Err(-12902);
        }
        payload.property_value_kind = VTF_PROPERTY_VALUE_KIND_NUMBER;
        payload.property_number_type = match number_type {
            K_CF_NUMBER_SINT32_TYPE => K_CF_NUMBER_SINT32_TYPE as u32,
            K_CF_NUMBER_SINT64_TYPE | K_CF_NUMBER_INT_TYPE => K_CF_NUMBER_SINT64_TYPE as u32,
            _ => return Err(-12902),
        };
        payload.property_sint64 = number_value;
        return Ok(payload);
    }

    if CFGetTypeID(property_value) == CFArrayGetTypeID() {
        let array = property_value as CFArrayRef;
        if CFArrayGetCount(array) != 2 {
            return Err(-12902);
        }
        for index in 0..2 {
            let item = CFArrayGetValueAtIndex(array, index as CFIndex);
            if item.is_null() || CFGetTypeID(item) != CFNumberGetTypeID() {
                return Err(-12902);
            }
            let mut number_value: i64 = 0;
            if CFNumberGetValue(
                item,
                K_CF_NUMBER_SINT64_TYPE,
                &mut number_value as *mut i64 as *mut c_void,
            ) == 0
            {
                return Err(-12902);
            }
            payload.property_array_i64[index] = number_value;
        }
        payload.property_value_kind = VTF_PROPERTY_VALUE_KIND_ARRAY_I64_PAIR;
        payload.property_array_count = 2;
        return Ok(payload);
    }

    if CFGetTypeID(property_value) == CFDictionaryGetTypeID() {
        let dictionary = property_value as CFDictionaryRef;
        let pair_keys = [
            (
                crate::coremedia::kCMFormatDescriptionKey_PixelAspectRatioHorizontalSpacing.0
                    as *const c_void,
                "HorizontalSpacing",
            ),
            (
                crate::coremedia::kCMFormatDescriptionKey_PixelAspectRatioVerticalSpacing.0
                    as *const c_void,
                "VerticalSpacing",
            ),
        ];
        for (index, (lookup_key, encoded_key)) in pair_keys.iter().enumerate() {
            let mut value: *const c_void = std::ptr::null();
            if CFDictionaryGetValueIfPresent(dictionary, *lookup_key, &mut value) == 0
                || value.is_null()
                || CFGetTypeID(value) != CFNumberGetTypeID()
            {
                return Err(-12902);
            }
            let mut number_value: i64 = 0;
            if CFNumberGetValue(
                value,
                K_CF_NUMBER_SINT64_TYPE,
                &mut number_value as *mut i64 as *mut c_void,
            ) == 0
            {
                return Err(-12902);
            }
            fill_small_key_bytes(&mut payload.property_dict_keys[index], encoded_key);
            payload.property_dict_sint64[index] = number_value;
        }
        payload.property_value_kind = VTF_PROPERTY_VALUE_KIND_DICT_I64_PAIR;
        payload.property_dict_pair_count = 2;
        return Ok(payload);
    }

    let Some(property_string) = cfstring_to_string(property_value as CFStringRef) else {
        return Err(-12902);
    };
    payload.property_value_kind = VTF_PROPERTY_VALUE_KIND_STRING;
    fill_key_bytes(&mut payload.property_string, &property_string);
    Ok(payload)
}

unsafe fn vtf_source_pixel_format_from_attributes(attributes: CFDictionaryRef) -> u32 {
    if attributes.is_null() {
        return 0x3432_3076;
    }
    let value = CFDictionaryGetValue(
        attributes,
        crate::corevideo::kCVPixelBufferPixelFormatTypeKey.0 as *const c_void,
    );
    if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
        return 0x3432_3076;
    }
    let mut pixel_format: i32 = 0;
    if CFNumberGetValue(
        value,
        K_CF_NUMBER_SINT32_TYPE,
        &mut pixel_format as *mut i32 as *mut c_void,
    ) == 0
    {
        return 0x3432_3076;
    }
    pixel_format as u32
}

unsafe fn vtf_push_pending_image_buffer(
    session: *mut vtf_vt_session,
    image_buffer: *mut super::corevideo::vtf_cv_pixel_buffer,
    source_frame_refcon: *mut c_void,
) -> bool {
    if (*session).pending_count == (*session).pending_capacity {
        let new_capacity = if (*session).pending_capacity == 0 {
            4
        } else {
            (*session).pending_capacity * 2
        };
        let bytes =
            new_capacity * std::mem::size_of::<*mut super::corevideo::vtf_cv_pixel_buffer>();
        let refcon_bytes = new_capacity * std::mem::size_of::<*mut c_void>();
        let new_ptr = libc::malloc(bytes);
        let new_refcon_ptr = libc::malloc(refcon_bytes);
        if new_ptr.is_null() || new_refcon_ptr.is_null() {
            if !new_ptr.is_null() {
                libc::free(new_ptr);
            }
            if !new_refcon_ptr.is_null() {
                libc::free(new_refcon_ptr);
            }
            return false;
        }
        if !(*session).pending_image_buffers.is_null() && (*session).pending_count > 0 {
            std::ptr::copy_nonoverlapping(
                (*session).pending_image_buffers,
                new_ptr.cast(),
                (*session).pending_count,
            );
            libc::free((*session).pending_image_buffers.cast());
        }
        if !(*session).pending_source_frame_refcons.is_null() && (*session).pending_count > 0 {
            std::ptr::copy_nonoverlapping(
                (*session).pending_source_frame_refcons,
                new_refcon_ptr.cast(),
                (*session).pending_count,
            );
            libc::free((*session).pending_source_frame_refcons.cast());
        }
        (*session).pending_image_buffers = new_ptr.cast();
        (*session).pending_source_frame_refcons = new_refcon_ptr.cast();
        (*session).pending_capacity = new_capacity;
    }
    *(*session)
        .pending_image_buffers
        .add((*session).pending_count) = crate::runtime::CFRetain(image_buffer as CFTypeRef)
        .cast_mut()
        .cast();
    *(*session)
        .pending_source_frame_refcons
        .add((*session).pending_count) = source_frame_refcon;
    (*session).pending_count += 1;
    true
}

unsafe fn vtf_pop_pending_image_buffer(
    session: *mut vtf_vt_session,
) -> (*mut super::corevideo::vtf_cv_pixel_buffer, *mut c_void) {
    if (*session).pending_count == 0 || (*session).pending_image_buffers.is_null() {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    let first = *(*session).pending_image_buffers;
    let first_refcon = if (*session).pending_source_frame_refcons.is_null() {
        std::ptr::null_mut()
    } else {
        *(*session).pending_source_frame_refcons
    };
    if (*session).pending_count > 1 {
        std::ptr::copy(
            (*session).pending_image_buffers.add(1),
            (*session).pending_image_buffers,
            (*session).pending_count - 1,
        );
        if !(*session).pending_source_frame_refcons.is_null() {
            std::ptr::copy(
                (*session).pending_source_frame_refcons.add(1),
                (*session).pending_source_frame_refcons,
                (*session).pending_count - 1,
            );
        }
    }
    (*session).pending_count -= 1;
    (first, first_refcon)
}

fn vtf_output_poll_interval() -> usize {
    std::env::var("VT_FERRY_OUTPUT_POLL_INTERVAL")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(VTF_OUTPUT_POLL_INTERVAL_DEFAULT)
}

fn vtf_output_batch_size() -> usize {
    std::env::var("VT_FERRY_OUTPUT_BATCH_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(VTF_OUTPUT_BATCH_SIZE_DEFAULT)
        .min(vt_ferry_protocol::VTF_TRANSPORT_MAX_OUTPUT_BATCH)
}

// Batch-encoding scaffolding — the current encode path is
// single-frame (the batch size always returns 1), but the queue +
// flush machinery is kept so future tuning can re-enable batched
// EncodeFrameBatchPayload sends without rebuilding from scratch.
// `vtf_flush_pending_encodes` is reachable from `vtf_drain_session`
// so it stays live; the helpers below are gated on a batch size > 1
// that the current build never produces.
#[allow(dead_code)]
fn vtf_encode_batch_size() -> usize {
    1
}

unsafe fn vtf_flush_pending_encodes(session: *mut vtf_vt_session) -> Result<(), i32> {
    let count = (*session).pending_encode_count;
    if count == 0 {
        return Ok(());
    }

    if count == 1 {
        crate::transport::encode_frame(&(*session).pending_encode_payloads[0])?;
    } else {
        let mut payload = vt_ferry_protocol::EncodeFrameBatchPayload::zeroed();
        payload.session_id = (*session).base.proxy_id;
        payload.frame_count = count as u32;
        payload.frames[..count].copy_from_slice(&(&(*session).pending_encode_payloads)[..count]);
        crate::transport::encode_frame_batch(&payload)?;
    }

    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:encode_frame flush session_id={} frame_count={count}",
        (*session).base.proxy_id
    ));
    (*session).pending_encode_count = 0;
    Ok(())
}

#[allow(dead_code)]
unsafe fn vtf_queue_pending_encode(
    session: *mut vtf_vt_session,
    payload: vt_ferry_protocol::EncodeFramePayload,
) -> Result<(), i32> {
    if (*session).pending_encode_count >= vt_ferry_protocol::VTF_TRANSPORT_MAX_ENCODE_BATCH {
        vtf_flush_pending_encodes(session)?;
    }

    let index = (*session).pending_encode_count;
    (*session).pending_encode_payloads[index] = payload;
    (*session).pending_encode_count += 1;

    if (*session).pending_encode_count >= vtf_encode_batch_size() {
        vtf_flush_pending_encodes(session)?;
    }

    Ok(())
}

unsafe fn vtf_finalize_vt_session(obj: *mut vtf_cf_object) {
    crate::runtime::vtf_record_vt_session_destroyed();
    let session = obj as *mut vtf_vt_session;
    // Belt-and-braces DESTROY_SESSION on CFRelease teardown.
    // ffmpeg's videotoolbox encoder/decoder calls
    // `VTCompressionSessionInvalidate` (which sends DESTROY) for
    // an explicit close, but some lifecycle paths (probe encoder
    // teardown via CFRelease, error-path cleanup) drop the
    // session without an Invalidate. Without this fall-through,
    // the worker keeps the session and any pool it owns alive
    // until the connection drops — which leaks
    // `IOSurfacePoolDirectory` entries for the rest of the
    // process lifetime, blocking concurrent guests competing
    // for the same shape.
    if !(*session).invalidated {
        let _ = crate::transport::destroy_session((*session).base.proxy_id);
        (*session).invalidated = true;
    }
    if !(*session).pixel_buffer_pool.is_null() {
        crate::runtime::CFRelease((*session).pixel_buffer_pool as CFTypeRef);
    }
    for index in 0..(*session).stored_property_count {
        crate::runtime::CFRelease((*session).stored_properties[index].value);
    }
    if !(*session).pending_image_buffers.is_null() {
        for index in 0..(*session).pending_count {
            let pending = *(*session).pending_image_buffers.add(index);
            crate::runtime::CFRelease(pending as CFTypeRef);
        }
    }
    if !(*session).pending_image_buffers.is_null() {
        libc::free((*session).pending_image_buffers.cast());
    }
    if !(*session).pending_source_frame_refcons.is_null() {
        libc::free((*session).pending_source_frame_refcons.cast());
    }
    crate::runtime::CFRelease((*session).source_image_buffer_attributes);
    crate::runtime::CFRelease((*session).properties);
    let _ = Box::from_raw(session);
}

unsafe fn vtf_prepare_session_if_needed(session: *mut vtf_vt_session) -> i32 {
    if (*session).prepared {
        return 0;
    }
    let payload = vt_ferry_protocol::PrepareSessionPayload {
        session_id: (*session).base.proxy_id,
    };
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:prepare session_id={}",
        payload.session_id
    ));
    match crate::transport::send_request_dynamic(vt_ferry_protocol::OP_PREPARE_SESSION, &payload) {
        Ok(_) => {
            (*session).prepared = true;
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:prepare ok session_id={}",
                payload.session_id
            ));
            0
        }
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:prepare failed session_id={} status={status}",
                payload.session_id
            ));
            -12902
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionCreate(
    _allocator: CFTypeRef,
    width: i32,
    height: i32,
    codecType: u32,
    _encoderSpecification: CFDictionaryRef,
    _sourceImageBufferAttributes: CFDictionaryRef,
    _compressedDataAllocator: CFTypeRef,
    outputCallback: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, i32, u32, *mut c_void)>,
    outputCallbackRefCon: *mut c_void,
    compressionSessionOut: *mut *mut vtf_vt_session,
) -> i32 {
    if compressionSessionOut.is_null() || width <= 0 || height <= 0 {
        return -12902; // kVTParameterErr
    }

    *compressionSessionOut = std::ptr::null_mut();

    if !crate::transport::is_enabled() {
        return -12902;
    }

    let source_pixel_format =
        vtf_source_pixel_format_from_attributes(_sourceImageBufferAttributes);
    let payload = vt_ferry_protocol::CreateSessionPayload {
        kind: 1, // VTF_SESSION_KIND_ENCODE
        codec: codecType,
        width: width as u32,
        height: height as u32,
        pixel_format: source_pixel_format,
        fps_num: 0,
        fps_den: 0,
        bitrate: 0,
        gop_size: 0,
    };
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:create_session width={} height={} codec=0x{:08x} pixel_format=0x{:08x}",
        width, height, codecType, source_pixel_format
    ));

    match crate::transport::create_session(&payload) {
        Ok(session_id) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:create_session ok session_id={session_id}"
            ));
            let session = Box::into_raw(Box::new(vtf_vt_session {
                base: crate::runtime::vtf_cf_object::with_host_id(
                    crate::runtime::VTF_TYPE_VT_SESSION,
                    Some(vtf_finalize_vt_session),
                    session_id,
                    session_id,
                    1,
                ),
                width,
                height,
                codec_type: codecType,
                pixel_format: source_pixel_format,
                properties: std::ptr::null(), // Optional mock for testing
                source_image_buffer_attributes: crate::runtime::CFRetain(
                    _sourceImageBufferAttributes,
                ),
                output_callback: outputCallback,
                output_refcon: outputCallbackRefCon,
                pixel_buffer_pool: std::ptr::null_mut(),
                prepared: false,
                invalidated: false,
                pending_count: 0,
                pending_capacity: 0,
                pending_image_buffers: std::ptr::null_mut(),
                pending_source_frame_refcons: std::ptr::null_mut(),
                pending_encode_count: 0,
                pending_encode_payloads: [vt_ferry_protocol::EncodeFramePayload::zeroed();
                    vt_ferry_protocol::VTF_TRANSPORT_MAX_ENCODE_BATCH],
                stored_property_count: 0,
                stored_properties: [vtf_vt_session_property {
                    key: [0u8; 64],
                    value: std::ptr::null(),
                }; 16],
            }));

            *compressionSessionOut = session;
            crate::runtime::vtf_record_vt_session_created();
            return 0; // noErr
        }
        Err(e) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:create_session failed status={e}"
            ));
            eprintln!("create_session failed: {}", e);
            return -12902;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionInvalidate(session: *mut vtf_vt_session) {
    if session.is_null() || (*session).invalidated {
        return;
    }
    let _ = crate::transport::destroy_session((*session).base.proxy_id);
    (*session).invalidated = true;
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionGetPixelBufferPool(
    session: *mut vtf_vt_session,
) -> *mut super::corevideo::vtf_cv_pixel_buffer_pool {
    if session.is_null() || (*session).invalidated {
        return std::ptr::null_mut();
    }
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:get_pool session_id={} existing_pool={}",
        (*session).base.proxy_id,
        !(*session).pixel_buffer_pool.is_null()
    ));
    if (*session).pixel_buffer_pool.is_null() {
        match super::corevideo::vtf_create_pixel_buffer_pool_for_session(
            session,
            (*session).base.proxy_id,
            (*session).width as u32,
            (*session).height as u32,
            (*session).pixel_format,
            (*session).source_image_buffer_attributes,
        ) {
            Ok(pool) => {
                (*session).pixel_buffer_pool = pool;
                crate::runtime::vtf_guest_trace(&format!(
                    "videotoolbox:get_pool created session_id={} pool_id={}",
                    (*session).base.proxy_id,
                    (*pool).base.proxy_id
                ));
            }
            Err(_) => {
                crate::runtime::vtf_guest_trace(&format!(
                    "videotoolbox:get_pool failed session_id={}",
                    (*session).base.proxy_id
                ));
                return std::ptr::null_mut();
            }
        }
    }
    (*session).pixel_buffer_pool
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionPrepareToEncodeFrames(
    session: *mut vtf_vt_session,
) -> i32 {
    if session.is_null() {
        return -12902;
    }
    if (*session).invalidated {
        return -12903;
    }
    vtf_prepare_session_if_needed(session)
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionEncodeFrame(
    session: *mut vtf_vt_session,
    imageBuffer: *mut super::corevideo::vtf_cv_pixel_buffer,
    presentationTimeStamp: crate::coremedia::CMTime,
    duration: crate::coremedia::CMTime,
    _frameProperties: CFDictionaryRef,
    _sourceFrameRefcon: *mut c_void,
    infoFlagsOut: *mut u32,
) -> i32 {
    if !infoFlagsOut.is_null() {
        *infoFlagsOut = 0;
    }
    if session.is_null() || imageBuffer.is_null() {
        return if crate::transport::is_enabled() {
            -12902
        } else {
            -12915
        };
    }
    if (*session).invalidated {
        return -12903;
    }
    if (*imageBuffer).state != super::corevideo::VTF_CV_BUFFER_STATE_GUEST_WRITABLE {
        return -12902;
    }
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:encode_frame session_id={} buffer_id={} generation={} slot_index={} slot_offset={} total_size={}",
        (*session).base.proxy_id,
        (*imageBuffer).base.proxy_id,
        (*imageBuffer).host_generation,
        (*imageBuffer).slot_index,
        (*imageBuffer).slot_offset,
        (*imageBuffer).total_size
    ));
    let prepare_status = vtf_prepare_session_if_needed(session);
    if prepare_status != 0 {
        return prepare_status;
    }

    if !(*imageBuffer).mapped_backing_active {
        if !super::corevideo::vtf_ensure_backing_store(imageBuffer) {
            return -12902;
        }
        let bytes = std::slice::from_raw_parts(
            (*imageBuffer).backing_store as *const u8,
            (*imageBuffer).total_size,
        );
        if let Err(status) = crate::transport::write_buffer(
            (*imageBuffer).base.proxy_id,
            (*imageBuffer).host_generation,
            bytes,
        ) {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:encode_frame write_buffer_failed session_id={} buffer_id={} status={status}",
                (*session).base.proxy_id,
                (*imageBuffer).base.proxy_id
            ));
            return -12902;
        }
    }

    let payload = vt_ferry_protocol::EncodeFramePayload {
        session_id: (*session).base.proxy_id,
        image_buffer_proxy_id: (*imageBuffer).base.proxy_id,
        image_buffer_host_id: (*imageBuffer).base.host_id,
        image_buffer_generation: (*imageBuffer).host_generation,
        pts_value: presentationTimeStamp.value,
        pts_timescale: presentationTimeStamp.timescale,
        duration_timescale: duration.timescale,
        duration_value: duration.value,
    };

    if !vtf_push_pending_image_buffer(session, imageBuffer, _sourceFrameRefcon) {
        return -12904;
    }
    (*imageBuffer).state = super::corevideo::VTF_CV_BUFFER_STATE_QUEUED_TO_HOST;

    let encode_result = crate::transport::encode_frame(&payload);

    match encode_result {
        Ok(()) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:encode_frame queued session_id={} buffer_id={}",
                (*session).base.proxy_id,
                (*imageBuffer).base.proxy_id,
            ));
        }
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:encode_frame failed session_id={} buffer_id={} status={status}",
                (*session).base.proxy_id,
                (*imageBuffer).base.proxy_id
            ));
            return -12902;
        }
    }

    let poll_interval = vtf_output_poll_interval();
    if poll_interval != 0 && (*session).pending_count % poll_interval == 0 {
        match vtf_deliver_session_outputs(session, None) {
            Ok(_) => 0,
            Err(status) => status,
        }
    } else {
        0
    }
}

unsafe fn vtf_deliver_session_outputs(
    session: *mut vtf_vt_session,
    wait_deadline: Option<Instant>,
) -> Result<usize, i32> {
    vtf_flush_pending_encodes(session)?;

    let Some(callback) = (*session).output_callback else {
        return Ok(0);
    };

    let mut delivered_outputs = 0usize;
    let batch_size = vtf_output_batch_size();
    loop {
        let dequeues = vtf_dequeue_ready_outputs(session, batch_size);
        let dequeues = match dequeues {
            Ok(dequeues) if !dequeues.is_empty() => dequeues,
            Ok(_) => break,
            Err(status) => {
                if vtf_handle_dequeue_status(session, wait_deadline, delivered_outputs, status)? {
                    continue;
                }
                break;
            }
        };

        for dequeue in dequeues {
            delivered_outputs += 1;
            vtf_deliver_one_output(session, callback, dequeue, delivered_outputs)?;
        }
    }

    Ok(delivered_outputs)
}

// Pool-pressure draining helper — used to be invoked by the
// CVPixelBufferPool fast path when the host pool ran low; the
// fast path no longer needs explicit pressure handling now that
// recycle/dequeue stays in lock-step. Kept so a future
// regression where the pool can starve has the recovery
// scaffolding ready, gated by VT_FERRY_POOL_PRESSURE_WAIT_MS.
#[allow(dead_code)]
pub(crate) unsafe fn vtf_deliver_outputs_for_pool_pressure(
    session: *mut vtf_vt_session,
) -> Result<usize, i32> {
    if session.is_null() {
        return Ok(0);
    }
    if (*session).invalidated {
        return Err(-12903);
    }

    let wait_ms = std::env::var("VT_FERRY_POOL_PRESSURE_WAIT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(VTF_POOL_PRESSURE_WAIT_MS_DEFAULT);
    let deadline = if wait_ms == 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_millis(wait_ms))
    };
    vtf_deliver_session_outputs(session, deadline)
}

unsafe fn vtf_dequeue_ready_outputs(
    session: *mut vtf_vt_session,
    batch_size: usize,
) -> Result<Vec<vt_ferry_protocol::DequeueOutputReply>, i32> {
    if batch_size > 1 {
        match crate::transport::dequeue_output_batch(&vt_ferry_protocol::DequeueOutputBatchPayload {
            session_id: (*session).base.proxy_id,
            max_outputs: batch_size as u32,
            reserved: 0,
        }) {
            Ok(reply) => {
                let count = (reply.output_count as usize)
                    .min(vt_ferry_protocol::VTF_TRANSPORT_MAX_OUTPUT_BATCH);
                return Ok(reply.outputs[..count].to_vec());
            }
            Err(status) if status != vt_ferry_protocol::STATUS_UNSUPPORTED_OPCODE as i32 => {
                return Err(status);
            }
            Err(_) => {}
        }
    }

    crate::transport::dequeue_output(&vt_ferry_protocol::DequeueOutputPayload {
        session_id: (*session).base.proxy_id,
    })
    .map(|reply| vec![reply])
}

unsafe fn vtf_handle_dequeue_status(
    session: *mut vtf_vt_session,
    wait_deadline: Option<Instant>,
    delivered_outputs: usize,
    status: i32,
) -> Result<bool, i32> {
    if status == vt_ferry_protocol::STATUS_TIMEOUT as i32 {
        if let Some(deadline) = wait_deadline {
            if (*session).pending_count != 0 {
                if Instant::now() >= deadline {
                    crate::runtime::vtf_guest_trace(&format!(
                        "videotoolbox:deliver_outputs timeout session_id={} pending_buffers={} delivered_outputs={}",
                        (*session).base.proxy_id,
                        (*session).pending_count,
                        delivered_outputs
                    ));
                    return Err(-12902);
                }
                std::thread::sleep(Duration::from_millis(VTF_COMPLETE_FRAMES_POLL_INTERVAL_MS));
                return Ok(true);
            }
        }
        return Ok(false);
    }

    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:deliver_outputs dequeue_failed session_id={} status={status} pending_buffers={}",
        (*session).base.proxy_id,
        (*session).pending_count
    ));
    Err(-12902)
}

unsafe fn vtf_deliver_one_output(
    session: *mut vtf_vt_session,
    callback: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, u32, *mut c_void),
    dequeue: vt_ferry_protocol::DequeueOutputReply,
    delivered_outputs: usize,
) -> Result<(), i32> {
    crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:deliver_outputs dequeue session_id={} output_id={} sample_size={} delivered_outputs={} pending_buffers_before_pop={}",
            (*session).base.proxy_id,
            dequeue.output_id,
            dequeue.sample_size,
            delivered_outputs,
            (*session).pending_count
        ));

    let read_bytes = match crate::transport::read_output(&vt_ferry_protocol::ReadOutputPayload {
        output_id: dequeue.output_id,
    }) {
        Ok(bytes) => bytes,
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                        "videotoolbox:deliver_outputs read_output_failed session_id={} output_id={} status={status}",
                        (*session).base.proxy_id,
                        dequeue.output_id
                    ));
            return Err(-12902);
        }
    };
    if read_bytes.len() < std::mem::size_of::<vt_ferry_protocol::ReadOutputReply>() {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:deliver_outputs read_output_short session_id={} output_id={} len={}",
            (*session).base.proxy_id,
            dequeue.output_id,
            read_bytes.len()
        ));
        return Err(-12902);
    }
    let read_reply: vt_ferry_protocol::ReadOutputReply = bytemuck::pod_read_unaligned(
        &read_bytes[..std::mem::size_of::<vt_ferry_protocol::ReadOutputReply>()],
    );
    let sample_size = read_reply.sample_size as usize;
    let sample_data_owned: Vec<u8> =
        read_bytes[std::mem::size_of::<vt_ferry_protocol::ReadOutputReply>()..].to_vec();
    let sample_data: &[u8] = sample_data_owned.as_slice();
    let (source_image_buffer, source_frame_refcon) = vtf_pop_pending_image_buffer(session);

    let block = {
        super::coremedia::vtf_create_block_buffer_from_bytes(sample_data)
    };
    if block.is_null() {
        crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:deliver_outputs block_buffer_failed session_id={} output_id={} sample_size={sample_size}",
                (*session).base.proxy_id,
                dequeue.output_id
            ));
        return Err(-12902);
    }
    // Pick the codec-specific FourCC for the format description so
    // CMSampleBufferGetFormatDescription on the guest reports the
    // matching codec_type. Fall back to the session-declared codec
    // if the worker reply's `codec` field is 0 (older replies) — the
    // session is the source of truth for the encode kind.
    let codec_type = if dequeue.codec != 0 {
        dequeue.codec
    } else {
        (*session).codec_type
    };
    let format_description = super::coremedia::vtf_create_video_format_description(
        codec_type,
        dequeue.width as i32,
        dequeue.height as i32,
        dequeue.parameter_set_count as usize,
        &dequeue.parameter_set_sizes,
        &dequeue.parameter_set_data,
        dequeue.nal_header_length as i32,
    );
    if format_description.is_null() {
        crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:deliver_outputs format_description_failed session_id={} output_id={} parameter_set_count={} nal_header_length={}",
                (*session).base.proxy_id,
                dequeue.output_id,
                dequeue.parameter_set_count,
                dequeue.nal_header_length
            ));
        crate::runtime::CFRelease(block);
        return Err(-12902);
    }
    let sample = super::coremedia::vtf_create_sample_buffer(
        block,
        format_description,
        crate::coremedia::CMTime {
            value: dequeue.pts_value,
            timescale: dequeue.pts_timescale,
            flags: crate::coremedia::kCMTimeFlags_Valid,
            epoch: 0,
        },
        crate::coremedia::kCMTimeInvalid,
        sample_size,
        dequeue.sample_flags,
        dequeue.output_id,
        source_image_buffer as CFTypeRef,
    );
    if !source_image_buffer.is_null() {
        crate::runtime::CFRelease(source_image_buffer as CFTypeRef);
    }
    crate::runtime::CFRelease(block);
    crate::runtime::CFRelease(format_description);
    if sample.is_null() {
        crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:deliver_outputs sample_buffer_failed session_id={} output_id={} sample_size={sample_size}",
                (*session).base.proxy_id,
                dequeue.output_id
            ));
        return Err(-12902);
    }

    callback(
        (*session).output_refcon,
        source_frame_refcon,
        0,
        0,
        sample as *mut c_void,
    );
    if !source_image_buffer.is_null() {
        super::corevideo::vtf_recycle_pixel_buffer(
            source_image_buffer as *mut super::corevideo::vtf_cv_pixel_buffer,
        );
        (*(source_image_buffer as *mut super::corevideo::vtf_cv_pixel_buffer)).state =
            super::corevideo::VTF_CV_BUFFER_STATE_GUEST_WRITABLE;
    }
    crate::runtime::CFRelease(sample);
    Ok(())
}

#[no_mangle]
pub unsafe extern "C" fn VTCompressionSessionCompleteFrames(
    _session: *mut vtf_vt_session,
    _completeUntilPresentationTimeStamp: u64,
) -> i32 {
    if _session.is_null() {
        crate::runtime::vtf_guest_trace("videotoolbox:complete_frames null_session");
        return -12902;
    }
    if (*_session).invalidated {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:complete_frames invalidated session_id={}",
            (*_session).base.proxy_id
        ));
        return -12903;
    }

    if let Err(status) = vtf_flush_pending_encodes(_session) {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:complete_frames encode_flush_failed session_id={} status={status}",
            (*_session).base.proxy_id
        ));
        return -12902;
    }

    let drain_reply = match crate::transport::drain((*_session).base.proxy_id) {
        Ok(reply) => reply,
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:complete_frames drain_failed session_id={} status={status}",
                (*_session).base.proxy_id
            ));
            return -12902;
        }
    };
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:complete_frames drain session_id={} pending_outputs={} pending_buffers={}",
        (*_session).base.proxy_id,
        drain_reply.pending_outputs,
        (*_session).pending_count
    ));

    if (*_session).output_callback.is_none() {
        return if drain_reply.session_id == (*_session).base.proxy_id {
            0
        } else {
            -12902
        };
    }

    // Output delivery goes through vtf_deliver_session_outputs which
    // pulls the callback off the session itself; no need to extract
    // it here.
    let wait_deadline = Instant::now()
        + Duration::from_millis(
            std::env::var("VT_FERRY_COMPLETE_FRAMES_WAIT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(VTF_COMPLETE_FRAMES_WAIT_MS_DEFAULT),
        );
    let delivered_outputs = match vtf_deliver_session_outputs(_session, Some(wait_deadline)) {
        Ok(count) => count,
        Err(status) => return status,
    };
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:complete_frames done session_id={} delivered_outputs={} pending_buffers={}",
        (*_session).base.proxy_id,
        delivered_outputs,
        (*_session).pending_count
    ));
    0
}

#[no_mangle]
pub unsafe extern "C" fn VTSessionSetProperty(
    _session: *mut vtf_vt_session,
    _propertyKey: CFStringRef,
    _propertyValue: CFTypeRef,
) -> i32 {
    if _session.is_null() || _propertyKey.is_null() || _propertyValue.is_null() {
        return -12902;
    }
    // VTSessionRef is opaque on Apple's side — works on both
    // compression and decompression sessions. Our two struct
    // types differ in layout past the vtf_cf_object base, so
    // discriminate on type_id before casting through to the
    // compression-specific fields. Decode sessions silently
    // accept property sets (return 0 / noErr) since the worker-
    // side decode path doesn't yet honor them; the call is
    // advisory in most decoder use cases.
    let base = _session as *const vtf_cf_object;
    if (*base).type_id == crate::runtime::VTF_TYPE_VT_DECOMPRESSION_SESSION {
        crate::runtime::vtf_guest_trace(
            "videotoolbox:VTSessionSetProperty on decode session — \
             silently accepted (decode property routing not yet wired)",
        );
        return 0;
    }
    if (*_session).invalidated {
        return -12903;
    }

    let Some(key) = cfstring_to_string(_propertyKey) else {
        return -12902;
    };

    let payload = match vtf_fill_set_property_payload(_session, _propertyKey, &key, _propertyValue)
    {
        Ok(payload) => payload,
        Err(status) => {
            eprintln!("VTSessionSetProperty payload failed key={key} status={status}");
            return status;
        }
    };

    if let Err(status) = crate::transport::set_property(&payload) {
        eprintln!("VTSessionSetProperty transport failed key={key} status={status}");
        return -12902;
    }

    for index in 0..(*_session).stored_property_count {
        let stored = &mut (*_session).stored_properties[index];
        let stored_len = stored
            .key
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(stored.key.len());
        if stored.key[..stored_len] == *key.as_bytes() {
            crate::runtime::CFRelease(stored.value);
            stored.value = crate::runtime::CFRetain(_propertyValue);
            return 0;
        }
    }

    if (*_session).stored_property_count >= (*_session).stored_properties.len() {
        return -12900;
    }

    let index = (*_session).stored_property_count;
    fill_key_bytes(&mut (*_session).stored_properties[index].key, &key);
    (*_session).stored_properties[index].value = crate::runtime::CFRetain(_propertyValue);
    (*_session).stored_property_count += 1;
    0
}

#[no_mangle]
pub unsafe extern "C" fn VTSessionCopyProperty(
    _session: *mut vtf_vt_session,
    _propertyKey: CFStringRef,
    _allocator: CFTypeRef,
    _propertyValueOut: *mut *mut c_void,
) -> i32 {
    if _session.is_null() || _propertyKey.is_null() || _propertyValueOut.is_null() {
        return -12902;
    }
    // Same opaque-VTSessionRef discrimination as SetProperty —
    // see comment there. Decode sessions return null property
    // (the caller's `*propertyValueOut` stays null) and noErr
    // status, treating the request as "no value stored" which is
    // benign for most callers.
    let base = _session as *const vtf_cf_object;
    if (*base).type_id == crate::runtime::VTF_TYPE_VT_DECOMPRESSION_SESSION {
        *_propertyValueOut = std::ptr::null_mut();
        return 0;
    }
    if (*_session).invalidated {
        return -12903;
    }

    *_propertyValueOut = std::ptr::null_mut();
    let Some(key) = cfstring_to_string(_propertyKey) else {
        return -12902;
    };

    for index in 0..(*_session).stored_property_count {
        let stored = &(*_session).stored_properties[index];
        let stored_len = stored
            .key
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(stored.key.len());
        if stored.key[..stored_len] == *key.as_bytes() {
            *_propertyValueOut = crate::runtime::CFRetain(stored.value) as *mut c_void;
            return 0;
        }
    }

    -12900
}

#[no_mangle]
pub unsafe extern "C" fn VTCopySupportedPropertyDictionaryForEncoder(
    _width: i32,
    _height: i32,
    _codecType: u32,
    _encoderSpecification: CFDictionaryRef,
    _encoderIDOut: *mut CFStringRef,
    _supportedPropertiesOut: *mut CFDictionaryRef,
) -> i32 {
    if _supportedPropertiesOut.is_null() {
        return -12902;
    }

    if !_encoderIDOut.is_null() {
        *_encoderIDOut = std::ptr::null();
    }

    let empty_value = crate::corefoundation::CFDictionaryCreate(
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
        0,
        &raw const kCFTypeDictionaryKeyCallBacks,
        &raw const kCFTypeDictionaryValueCallBacks,
    );
    let property_keys = [
        kVTCompressionPropertyKey_AverageBitRate.0,
        kVTCompressionPropertyKey_AllowFrameReordering.0,
        kVTCompressionPropertyKey_AllowOpenGOP.0,
        kVTCompressionPropertyKey_ColorPrimaries.0,
        kVTCompressionPropertyKey_DataRateLimits.0,
        kVTCompressionPropertyKey_H264EntropyMode.0,
        kVTCompressionPropertyKey_MaxAllowedFrameQP.0,
        kVTCompressionPropertyKey_MaxKeyFrameInterval.0,
        kVTCompressionPropertyKey_MaxH264SliceBytes.0,
        kVTCompressionPropertyKey_MinAllowedFrameQP.0,
        kVTCompressionPropertyKey_MoreFramesBeforeStart.0,
        kVTCompressionPropertyKey_MoreFramesAfterEnd.0,
        kVTCompressionPropertyKey_PixelAspectRatio.0,
        kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality.0,
        kVTCompressionPropertyKey_RealTime.0,
        kVTCompressionPropertyKey_ProfileLevel.0,
        kVTCompressionPropertyKey_TransferFunction.0,
        kVTCompressionPropertyKey_YCbCrMatrix.0,
    ];
    let property_values = [
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
        empty_value,
    ];
    let supported = crate::corefoundation::CFDictionaryCreate(
        std::ptr::null(),
        property_keys.as_ptr(),
        property_values.as_ptr(),
        property_keys.len() as i64,
        &raw const kCFTypeDictionaryKeyCallBacks,
        &raw const kCFTypeDictionaryValueCallBacks,
    );
    crate::runtime::CFRelease(empty_value);
    *_supportedPropertiesOut = supported;
    if supported.is_null() {
        return -12900;
    }
    0
}

// ===========================================================
//  VTDecompressionSession surface (Phase 10 decode bring-up)
// ===========================================================
//
// Mirror of the encode-side `vtf_vt_session` + entry points, but
// inverted: encoded sample buffers flow IN via DecodeFrame, decoded
// pixel buffers come OUT via the user's output callback.
//
// Two-phase commit at create time: VTDecompressionSessionCreate
// kicks off OP_CREATE_SESSION (decode), then extracts parameter
// sets from the format description and ships them via
// OP_SET_DECODE_FORMAT. The worker can't build the underlying
// VTDecompressionSession until both halves arrive.

/// VTDecompressionOutputCallback signature. Matches Apple's:
///   (refcon, sourceFrameRefCon, status, infoFlags,
///    imageBuffer, presentationTimeStamp, presentationDuration)
/// CMTime is passed by value (24 bytes). FFmpeg's videotoolbox
/// decoder calls this signature via its compiled callback; we
/// must match exactly or the C-side ABI will mis-marshal args.
pub type VTDecompressionOutputCallback = unsafe extern "C" fn(
    refcon: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: i32,
    info_flags: u32,
    image_buffer: *mut c_void,
    presentation_time_stamp: super::coremedia::CMTime,
    presentation_duration: super::coremedia::CMTime,
);

#[repr(C)]
pub struct vtf_vt_decompression_session {
    pub base: vtf_cf_object,
    pub codec_type: u32,
    pub width: i32,
    pub height: i32,
    /// Caller-supplied output callback fired when decoded frames
    /// are dequeued. Held until VTDecompressionSessionInvalidate.
    pub output_callback: Option<VTDecompressionOutputCallback>,
    pub output_refcon: *mut c_void,
    pub format_description: CFTypeRef,
    pub format_set: bool,
    pub invalidated: bool,
    /// `true` once `OP_BIND_DECODE_OUTPUT_POOL` has switched the
    /// session to the zero-copy chunked output path. Set when the
    /// decoded NV12 frame size exceeds the inline
    /// `OP_READ_DECODED_FRAME` budget
    /// (`VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES`); the drain loop
    /// then fetches pixel bytes via chunked
    /// `OP_READ_DECODED_FRAME_CHUNK` against VT's CVImageBuffer
    /// directly (no intermediate worker-side slot copy).
    pub chunked_output: bool,
}

unsafe fn vtf_finalize_vt_decompression_session(obj: *mut vtf_cf_object) {
    let session = obj as *mut vtf_vt_decompression_session;
    if session.is_null() {
        return;
    }
    // Mirror the compression-session belt-and-braces: if the
    // caller dropped the decompression session via CFRelease
    // without an explicit `VTDecompressionSessionInvalidate`,
    // synthesize the DESTROY_SESSION here so the worker frees
    // its VTDecompressionSession + format description instead
    // of holding them until the connection drops.
    if !(*session).invalidated {
        let _ = crate::transport::destroy_session((*session).base.proxy_id);
        (*session).invalidated = true;
    }
    if !(*session).format_description.is_null() {
        crate::runtime::CFRelease((*session).format_description);
    }
    drop(Box::from_raw(session));
    crate::runtime::vtf_record_vt_session_destroyed();
}

/// Extract H.264 / HEVC parameter sets from a guest-side
/// `CMVideoFormatDescription` proxy and pack them into a
/// `SetDecodeFormatPayload`. The proxy is a host-backed object
/// — the existing parameter-set accessors round-trip through
/// the worker.
///
/// The functions below (`vtf_split_avcc_bitstream`,
/// `vtf_nal_unit_type`, `vtf_pack_decode_format_payload_from_inband`)
/// were prototyped during Phase 15 as a fallback for hwaccel
/// paths that deliver SPS/PPS in-band rather than via the
/// `extensions` dictionary. FFmpeg's actual hwaccel videotoolbox
/// path uses the `extensions` route — see
/// `coremedia::CMVideoFormatDescriptionCreate`'s extraction
/// logic — so these helpers aren't on the live path. Kept
/// behind `#[allow(dead_code)]` (with their tests still
/// running) so a future caller that DOES embed parameter sets
/// in the bitstream can flip them on without re-deriving the
/// parsing logic.
#[allow(dead_code)]
fn vtf_split_avcc_bitstream(data: &[u8], length_prefix_bytes: usize) -> Vec<&[u8]> {
    let mut units: Vec<&[u8]> = Vec::new();
    if length_prefix_bytes == 0 || length_prefix_bytes > 4 {
        return units;
    }
    let mut offset = 0usize;
    while offset + length_prefix_bytes <= data.len() {
        let mut nal_len: usize = 0;
        for i in 0..length_prefix_bytes {
            nal_len = (nal_len << 8) | data[offset + i] as usize;
        }
        offset += length_prefix_bytes;
        if nal_len == 0 || offset + nal_len > data.len() {
            // Empty or truncated NAL — stop rather than panic.
            break;
        }
        units.push(&data[offset..offset + nal_len]);
        offset += nal_len;
    }
    units
}

/// Identify a NAL unit as SPS / PPS / VPS for H.264 (`'avc1'`)
/// or HEVC (`'hvc1'`). Returns the NAL type byte the caller can
/// match against:
///   H.264: 7=SPS, 8=PPS
///   HEVC:  32=VPS, 33=SPS, 34=PPS
#[allow(dead_code)]
fn vtf_nal_unit_type(nal: &[u8], codec: u32) -> Option<u8> {
    const FOURCC_AVC1: u32 = 0x6176_6331;
    if nal.is_empty() {
        return None;
    }
    if codec == FOURCC_AVC1 {
        // First byte: forbidden_zero_bit | nal_ref_idc(2) | nal_unit_type(5)
        Some(nal[0] & 0x1F)
    } else {
        // HEVC: 2-byte header. type = (first_byte >> 1) & 0x3F.
        if nal.len() < 2 {
            return None;
        }
        Some((nal[0] >> 1) & 0x3F)
    }
}

/// Walk the AVCC-framed encoded bitstream FFmpeg's hwaccel
/// videotoolbox path delivers in each `CMSampleBuffer`'s data
/// buffer, extract SPS/PPS (H.264) or VPS+SPS+PPS (HEVC), and
/// pack them into a `SetDecodeFormatPayload` ready to ship via
/// `OP_SET_DECODE_FORMAT`. Used when the format description
/// passed to `VTDecompressionSessionCreate` was a placeholder
/// with no parameter sets — see Phase 15 in the
/// IMPLEMENTATION-BACKLOG for the why.
///
/// Returns `Err(-12710)` if no parameter sets are present (the
/// caller hasn't seen an IDR yet), `Err(-12902)` if the byte
/// budget would overflow, `Ok(payload)` otherwise.
#[allow(dead_code)]
fn vtf_pack_decode_format_payload_from_inband(
    session_id: u64,
    codec: u32,
    width: i32,
    height: i32,
    encoded_data: &[u8],
) -> Result<vt_ferry_protocol::SetDecodeFormatPayload, i32> {
    const FOURCC_AVC1: u32 = 0x6176_6331;
    const FOURCC_HVC1: u32 = 0x6876_6331;
    // FFmpeg's hwaccel videotoolbox always emits 4-byte length
    // prefixes; both `videotoolbox_h264_end_frame` and
    // `videotoolbox_hevc_end_frame` hardcode the AVCC length
    // size. Hardcoding 4 here matches the wire format we
    // actually receive on this path.
    let length_prefix_bytes = 4usize;
    let nals = vtf_split_avcc_bitstream(encoded_data, length_prefix_bytes);
    let (sps_type, pps_type, vps_type) = if codec == FOURCC_HVC1 {
        // HEVC: VPS=32, SPS=33, PPS=34
        (33u8, 34u8, Some(32u8))
    } else if codec == FOURCC_AVC1 {
        // H.264: SPS=7, PPS=8 (no VPS)
        (7u8, 8u8, None)
    } else {
        return Err(-12902);
    };

    let mut sps_bytes: Option<&[u8]> = None;
    let mut pps_bytes: Option<&[u8]> = None;
    let mut vps_bytes: Option<&[u8]> = None;
    for nal in &nals {
        match vtf_nal_unit_type(nal, codec) {
            Some(t) if t == sps_type && sps_bytes.is_none() => sps_bytes = Some(nal),
            Some(t) if t == pps_type && pps_bytes.is_none() => pps_bytes = Some(nal),
            Some(t) if vps_type == Some(t) && vps_bytes.is_none() => vps_bytes = Some(nal),
            _ => {}
        }
    }

    // Order matters for the worker's
    // `CMVideoFormatDescriptionCreateFromH264ParameterSets` /
    // ...HEVC... call: H.264 wants [SPS, PPS]; HEVC wants
    // [VPS, SPS, PPS]. We package in that order.
    let parameter_sets: Vec<&[u8]> = match (vps_bytes, sps_bytes, pps_bytes) {
        (Some(vps), Some(sps), Some(pps)) if codec == FOURCC_HVC1 => vec![vps, sps, pps],
        (None, Some(sps), Some(pps)) if codec == FOURCC_AVC1 => vec![sps, pps],
        _ => return Err(-12710), // kCMFormatDescriptionError_InvalidParameter — caller hasn't seen an IDR yet
    };

    let total_bytes: usize = parameter_sets.iter().map(|s| s.len()).sum();
    if total_bytes > vt_ferry_protocol::VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES {
        return Err(-12902);
    }
    if parameter_sets.len() > vt_ferry_protocol::VTF_TRANSPORT_MAX_PARAMETER_SETS {
        return Err(-12902);
    }

    let mut payload = vt_ferry_protocol::SetDecodeFormatPayload::zeroed();
    payload.session_id = session_id;
    payload.codec = codec;
    payload.width = width as u32;
    payload.height = height as u32;
    payload.nal_header_length = length_prefix_bytes as u32;
    payload.parameter_set_count = parameter_sets.len() as u32;

    let mut data_offset = 0usize;
    for (i, ps) in parameter_sets.iter().enumerate() {
        payload.parameter_set_sizes[i] = ps.len() as u32;
        payload.parameter_set_data[data_offset..data_offset + ps.len()].copy_from_slice(ps);
        data_offset += ps.len();
    }
    Ok(payload)
}

unsafe fn vtf_pack_decode_format_payload(
    session_id: u64,
    codec: u32,
    width: i32,
    height: i32,
    format_description: CFTypeRef,
) -> Result<vt_ferry_protocol::SetDecodeFormatPayload, i32> {
    let mut payload = vt_ferry_protocol::SetDecodeFormatPayload::zeroed();
    payload.session_id = session_id;
    payload.codec = codec;
    payload.width = width as u32;
    payload.height = height as u32;

    // VTDecompressionSessionCreate expects an AVCC-shaped format
    // description (length-prefix framing, typically 4 bytes for
    // both H.264 and HEVC). The accessors return the
    // nal_header_length the format description was built with;
    // we trust whatever the caller passed.
    // FOURCC_AVC1 = 0x6176_6331 ('avc1'); we only need the HEVC
    // discriminator since H.264 is the default branch.
    const FOURCC_HVC1: u32 = 0x6876_6331;
    let mut parameter_set_count: usize = 0;
    let mut nal_header_length: i32 = 0;
    let probe_status = if codec == FOURCC_HVC1 {
        super::coremedia::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
            format_description as *const _,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut parameter_set_count,
            &mut nal_header_length,
        )
    } else {
        super::coremedia::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_description as *const _,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut parameter_set_count,
            &mut nal_header_length,
        )
    };
    if probe_status != 0 {
        return Err(probe_status);
    }
    if parameter_set_count == 0
        || parameter_set_count > vt_ferry_protocol::VTF_TRANSPORT_MAX_PARAMETER_SETS
    {
        return Err(-12902);
    }

    payload.nal_header_length = nal_header_length as u32;
    payload.parameter_set_count = parameter_set_count as u32;

    // Pull the actual parameter-set bytes one at a time and pack
    // them into the inline buffer.
    let mut data_offset: usize = 0;
    for index in 0..parameter_set_count {
        let mut param_ptr: *const u8 = std::ptr::null();
        let mut param_size: usize = 0;
        let status = if codec == FOURCC_HVC1 {
            super::coremedia::CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                format_description as *const _,
                index,
                &mut param_ptr,
                &mut param_size,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } else {
            super::coremedia::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format_description as *const _,
                index,
                &mut param_ptr,
                &mut param_size,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != 0 || param_ptr.is_null() {
            return Err(status);
        }
        if data_offset + param_size > payload.parameter_set_data.len() {
            return Err(-12902);
        }
        let dest = &mut payload.parameter_set_data[data_offset..data_offset + param_size];
        std::ptr::copy_nonoverlapping(param_ptr, dest.as_mut_ptr(), param_size);
        payload.parameter_set_sizes[index] = param_size as u32;
        data_offset += param_size;
    }
    Ok(payload)
}

/// VTDecompressionSessionCreate proxy entrypoint. FFmpeg's
/// `videotoolbox` decoder calls this once per decode session
/// after building a `CMVideoFormatDescription` from parsed
/// parameter sets.
///
/// Two-phase commit:
///   1. OP_CREATE_SESSION (decode) reserves the session id.
///   2. Parameter sets get extracted from the format description
///      and shipped via OP_SET_DECODE_FORMAT, which actually
///      creates the underlying VTDecompressionSession on the
///      host.
#[no_mangle]
pub unsafe extern "C" fn VTDecompressionSessionCreate(
    _allocator: CFTypeRef,
    video_format_description: CFTypeRef,
    _video_decoder_specification: CFDictionaryRef,
    _destination_image_buffer_attributes: CFDictionaryRef,
    output_callback: *const VTDecompressionOutputCallbackRecord,
    decompression_session_out: *mut *mut vtf_vt_decompression_session,
) -> i32 {
    if decompression_session_out.is_null() || video_format_description.is_null() {
        return -12902;
    }
    *decompression_session_out = std::ptr::null_mut();

    if !crate::transport::is_enabled() {
        return -12902;
    }

    // Pull codec / dimensions directly from the
    // vtf_cm_format_description proxy. Both live in the same
    // crate so the layout is stable; this avoids a
    // CMFormatDescriptionGetMediaSubType /
    // CMVideoFormatDescriptionGetDimensions round trip we'd
    // otherwise have to add.
    let format_description_ref =
        video_format_description as *const super::coremedia::vtf_cm_format_description;
    let codec = (*format_description_ref).codec_type;
    let width = (*format_description_ref).width;
    let height = (*format_description_ref).height;

    let create_payload = vt_ferry_protocol::CreateSessionPayload {
        kind: vt_ferry_protocol::VTF_SESSION_KIND_DECODE,
        codec,
        width: width as u32,
        height: height as u32,
        pixel_format: 0, // worker ignores for decode
        fps_num: 0,
        fps_den: 0,
        bitrate: 0,
        gop_size: 0,
    };
    crate::runtime::vtf_guest_trace(&format!(
        "videotoolbox:decompression_create width={} height={} codec=0x{:08x}",
        width, height, codec
    ));

    let session_id = match crate::transport::create_session(&create_payload) {
        Ok(id) => id,
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:decompression_create CREATE_SESSION failed status={status}"
            ));
            return -12902;
        }
    };

    // Pack + ship the parameter sets via OP_SET_DECODE_FORMAT.
    // Both creation paths populate the format description's
    // `parameter_set_*` fields by the time we get here:
    //
    //   1. `CMVideoFormatDescriptionCreateFromH264ParameterSets`
    //      (or HEVC equivalent) writes them directly.
    //   2. FFmpeg's `-hwaccel videotoolbox` path — uses the
    //      generic `CMVideoFormatDescriptionCreate` but supplies
    //      an extensions dict containing an AVCC/HVCC config
    //      record under
    //      `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`
    //      → `"avcC"` / `"hvcC"`. Our shim's
    //      `CMVideoFormatDescriptionCreate` parses that record
    //      and populates `parameter_sets` for us — see
    //      `coremedia::vtf_extract_param_sets_from_extensions`.
    //
    // If pack still fails, the format description is genuinely
    // empty (caller built it with neither parameter sets nor an
    // AVCC/HVCC blob). Surface that as -12902 so the caller
    // falls back instead of decoding garbage.
    let format_payload = match vtf_pack_decode_format_payload(
        session_id,
        codec,
        width,
        height,
        video_format_description,
    ) {
        Ok(p) => p,
        Err(status) => {
            let _ = crate::transport::destroy_session(session_id);
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:decompression_create pack_format failed status={status} \
                 — format description has neither explicit parameter sets nor an \
                 AVCC/HVCC config record in extensions"
            ));
            return -12902;
        }
    };
    if let Err(status) = crate::transport::set_decode_format(&format_payload) {
        let _ = crate::transport::destroy_session(session_id);
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:decompression_create SET_DECODE_FORMAT failed status={status}"
        ));
        return -12902;
    }
    let format_set = true;

    let (callback, refcon) = if !output_callback.is_null() {
        (
            (*output_callback).decompression_output_callback,
            (*output_callback).decompression_output_ref_con,
        )
    } else {
        (None, std::ptr::null_mut())
    };

    // Decide whether the decoded-frame size exceeds the inline
    // OP_READ_DECODED_FRAME budget. If so, switch the session to
    // chunked-zero-copy output: the dequeue reply will signal it
    // via nonzero `buffer_host_id` and the drain loop fetches
    // pixels via repeated OP_READ_DECODED_FRAME_CHUNK calls.
    //
    // The size threshold uses width × height × 3/2 (NV12 lower
    // bound; the canonical layout adds 64-byte stride alignment
    // which only matters at very small widths). Worker validates
    // the pool_id-must-be-zero contract, so no pool allocation
    // happens here — the BIND op is purely a session-mode toggle.
    let nominal_frame_bytes = (width as usize) * (height as usize) * 3 / 2;
    let needs_chunked = nominal_frame_bytes
        > vt_ferry_protocol::VTF_TRANSPORT_MAX_DECODED_FRAME_BYTES as usize;

    if needs_chunked {
        let bind_payload = vt_ferry_protocol::BindDecodeOutputPoolPayload {
            session_id,
            pool_id: 0,
            reserved: [0; 16],
        };
        if let Err(status) = crate::transport::bind_decode_output_pool(&bind_payload) {
            let _ = crate::transport::destroy_session(session_id);
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:decompression_create BIND_DECODE_OUTPUT_POOL \
                 (chunked-mode toggle) failed status={status}"
            ));
            return -12902;
        }
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:decompression_create chunked-mode session_id={} \
             nominal_frame_bytes={}",
            session_id, nominal_frame_bytes
        ));
    }

    let session = Box::into_raw(Box::new(vtf_vt_decompression_session {
        base: crate::runtime::vtf_cf_object::with_host_id(
            crate::runtime::VTF_TYPE_VT_DECOMPRESSION_SESSION,
            Some(vtf_finalize_vt_decompression_session),
            session_id,
            session_id,
            1,
        ),
        codec_type: codec,
        width,
        height,
        output_callback: callback,
        output_refcon: refcon,
        format_description: crate::runtime::CFRetain(video_format_description),
        format_set,
        invalidated: false,
        chunked_output: needs_chunked,
    }));

    *decompression_session_out = session;
    crate::runtime::vtf_record_vt_session_created();
    0
}

#[no_mangle]
pub unsafe extern "C" fn VTDecompressionSessionInvalidate(
    session: *mut vtf_vt_decompression_session,
) {
    if session.is_null() || (*session).invalidated {
        return;
    }
    let _ = crate::transport::destroy_session((*session).base.proxy_id);
    (*session).invalidated = true;
}

/// VTDecompressionOutputCallbackRecord — passed by FFmpeg to
/// `VTDecompressionSessionCreate` to register the per-session
/// output callback. Layout matches Apple's
/// `VTDecompressionOutputCallbackRecord`: `{callback, refcon}`.
#[repr(C)]
pub struct VTDecompressionOutputCallbackRecord {
    pub decompression_output_callback: Option<VTDecompressionOutputCallback>,
    pub decompression_output_ref_con: *mut c_void,
}

/// Drain any decoded frames the worker has queued for this
/// session. Called after `enqueue_encoded_frame` (the synchronous
/// drain pattern: most VT decode pipelines emit decoded frames
/// quickly enough that draining after each submission keeps the
/// queue depth bounded). Each decoded frame fires the user's
/// output callback with a guest-side CVPixelBuffer proxy that
/// wraps the inline pixel bytes returned by `OP_READ_DECODED_FRAME`.
unsafe fn vtf_drain_decoded_frames(session: *mut vtf_vt_decompression_session) -> i32 {
    if session.is_null() {
        return -12902;
    }
    let session_id = (*session).base.proxy_id;
    let callback_opt = (*session).output_callback;
    let refcon = (*session).output_refcon;
    // No-callback path: still drain the worker queue (releasing
    // each slot) so we don't leak. Without releases, the worker's
    // decoded_output_state would grow unbounded across DecodeFrame
    // calls — every successful decode would burn a slot.
    let callback = match callback_opt {
        Some(cb) => Some(cb),
        None => None,
    };

    loop {
        let dequeue_payload = vt_ferry_protocol::DequeueDecodedFramePayload {
            session_id,
        };
        let reply = match crate::transport::dequeue_decoded_frame(&dequeue_payload) {
            Ok(reply) => reply,
            Err(status) if status == vt_ferry_protocol::STATUS_TIMEOUT as i32 => {
                // Queue is empty — done draining.
                return 0;
            }
            Err(status) => {
                crate::runtime::vtf_guest_trace(&format!(
                    "videotoolbox:decompression_drain DEQUEUE failed status={status}"
                ));
                return status;
            }
        };

        // Error sentinel: worker normal output_ids start at 70_000,
        // so output_id == 0 is the wire-level marker that a VT
        // decode callback fired with non-zero status (e.g. -12909
        // = kVTVideoDecoderBadDataErr) and there's no real frame
        // to fetch. `reply.status` carries the VT OSStatus directly;
        // surface it as the function's return value so FFmpeg's
        // videotoolbox glue can propagate a real per-frame error
        // instead of silently producing fewer frames than expected.
        if reply.output_id == 0 {
            let vt_status = reply.status as i32;
            crate::runtime::vtf_guest_trace(&format!(
                "videotoolbox:decompression_drain VT decode error \
                 surfaced session={session_id} vt_status={vt_status}"
            ));
            return vt_status;
        }

        // No-callback fast path: skip the (potentially large)
        // pixel-data fetch entirely. Just release the slot so the
        // worker can recycle it.
        let cb = match callback {
            Some(cb) => cb,
            None => {
                let _ = crate::transport::release_decoded_frame(
                    &vt_ferry_protocol::ReleaseDecodedFramePayload {
                        session_id,
                        output_id: reply.output_id,
                    },
                );
                continue;
            }
        };

        // Branch on inline vs chunked-zero-copy delivery. Worker
        // populates buffer_host_id with the output_id when the
        // session has been switched to chunked mode via
        // OP_BIND_DECODE_OUTPUT_POOL; otherwise the inline READ
        // path applies (≤720p).
        let (pixel_bytes_owned, sample_size, status_code) = if reply.buffer_host_id != 0 {
            // Chunked-zero-copy path: fetch via repeated
            // OP_READ_DECODED_FRAME_CHUNK against the held
            // CVImageBuffer. Total bytes derived from the
            // canonical layout (mirrors the worker's
            // vtf_fill_buffer_layout) so guest and worker agree
            // on per-frame size without round-tripping a
            // sample_size field.
            let mut layout = vt_ferry_protocol::BufferLayoutReply::zeroed();
            vtf_decode_layout_for_format(
                reply.width,
                reply.height,
                reply.pixel_format,
                &mut layout,
            );
            let total_bytes = layout.total_size as usize;
            match crate::transport::read_decoded_frame_chunk(
                session_id,
                reply.output_id,
                total_bytes,
            ) {
                Ok(bytes) => (bytes, total_bytes as u32, reply.status),
                Err(status) => {
                    let _ = crate::transport::release_decoded_frame(
                        &vt_ferry_protocol::ReleaseDecodedFramePayload {
                            session_id,
                            output_id: reply.output_id,
                        },
                    );
                    crate::runtime::vtf_guest_trace(&format!(
                        "videotoolbox:decompression_drain READ_DECODED_FRAME_CHUNK \
                         failed status={status} output_id={}",
                        reply.output_id
                    ));
                    return status;
                }
            }
        } else {
            // Inline (≤720p): single OP_READ_DECODED_FRAME call.
            let read_payload = vt_ferry_protocol::ReadDecodedFramePayload {
                session_id,
                output_id: reply.output_id,
            };
            let read_bytes = match crate::transport::read_decoded_frame(&read_payload) {
                Ok(b) => b,
                Err(status) => {
                    let _ = crate::transport::release_decoded_frame(
                        &vt_ferry_protocol::ReleaseDecodedFramePayload {
                            session_id,
                            output_id: reply.output_id,
                        },
                    );
                    crate::runtime::vtf_guest_trace(&format!(
                        "videotoolbox:decompression_drain READ failed status={status}"
                    ));
                    return status;
                }
            };
            let read_reply: vt_ferry_protocol::ReadDecodedFrameReply =
                bytemuck::pod_read_unaligned(
                    &read_bytes[..std::mem::size_of::<vt_ferry_protocol::ReadDecodedFrameReply>()],
                );
            let header_size = std::mem::size_of::<vt_ferry_protocol::ReadDecodedFrameReply>();
            let expected_total = header_size + read_reply.sample_size as usize;
            if read_bytes.len() < expected_total {
                crate::runtime::vtf_guest_trace(&format!(
                    "videotoolbox:decompression_drain READ truncated — \
                     got {} bytes, expected {} (header={} + sample_size={})",
                    read_bytes.len(),
                    expected_total,
                    header_size,
                    read_reply.sample_size,
                ));
                let _ = crate::transport::release_decoded_frame(
                    &vt_ferry_protocol::ReleaseDecodedFramePayload {
                        session_id,
                        output_id: reply.output_id,
                    },
                );
                return -12902;
            }
            (
                read_bytes[header_size..expected_total].to_vec(),
                read_reply.sample_size,
                read_reply.status,
            )
        };

        let _ = sample_size; // currently informational; layout drives plane sizing

        // Build a guest-side CVPixelBuffer proxy wrapping the
        // pixel bytes. The buffer owns its backing store;
        // vtf_finalize_pixel_buffer will free it when the user's
        // callback returns and CFRelease drops the last retain.
        let mut owned_bytes = pixel_bytes_owned;
        let backing_ptr = owned_bytes.as_mut_ptr();
        let backing_len = owned_bytes.len();
        std::mem::forget(owned_bytes); // ownership moves into the pixel buffer

        let mut layout = vt_ferry_protocol::BufferLayoutReply::zeroed();
        vtf_decode_layout_for_format(
            reply.width,
            reply.height,
            reply.pixel_format,
            &mut layout,
        );

        let mut plane_offsets = [0usize; 4];
        let mut plane_widths = [0usize; 4];
        let mut plane_heights = [0usize; 4];
        let mut plane_bytes_per_row = [0usize; 4];
        let mut plane_data = [std::ptr::null_mut::<u8>(); 4];
        let plane_count = layout.plane_count.min(4) as usize;
        for i in 0..plane_count {
            plane_offsets[i] = layout.plane_offsets[i] as usize;
            plane_widths[i] = layout.plane_widths[i] as usize;
            plane_heights[i] = layout.plane_heights[i] as usize;
            plane_bytes_per_row[i] = layout.plane_bytes_per_row[i] as usize;
            plane_data[i] = backing_ptr.add(plane_offsets[i]);
        }

        let pixel_buffer = Box::into_raw(Box::new(super::corevideo::vtf_cv_pixel_buffer {
            base: vtf_cf_object {
                magic: 0x534d5654,
                type_id: crate::runtime::VTF_TYPE_PIXEL_BUFFER,
                refcount: std::sync::atomic::AtomicI32::new(1),
                flags: 0,
                proxy_id: 0,
                generation: 0,
                host_id: 0,
                finalize: Some(super::corevideo::vtf_finalize_pixel_buffer),
            },
            pool_ref: std::ptr::null_mut(),
            pool_host_id: 0,
            host_generation: 0,
            host_backing_kind: 0,
            recycled: true, // already released worker-side after READ
            cache_valid: false,
            backing_store_owned: true, // free in finalize
            mapped_backing_active: false,
            state: super::corevideo::VTF_CV_BUFFER_STATE_GUEST_WRITABLE,
            slot_index: 0,
            slot_offset: 0,
            slot_region_size: backing_len as u32,
            lock_flags: 0,
            lock_snapshot: std::ptr::null_mut(),
            lock_snapshot_size: 0,
            backing_store: backing_ptr,
            backing_store_size: backing_len,
            mapped_region_base: std::ptr::null_mut(),
            mapped_region_size: 0,
            total_size: backing_len,
            width: reply.width as usize,
            height: reply.height as usize,
            pixel_format: reply.pixel_format,
            plane_count,
            plane_offsets,
            plane_widths,
            plane_heights,
            plane_bytes_per_row,
            plane_data,
            attachments: std::ptr::null_mut(),
            locked: false,
        }));
        crate::runtime::vtf_record_proxy_alive_increment();

        let pts = super::coremedia::CMTime {
            value: reply.pts_value,
            timescale: reply.pts_timescale,
            flags: super::coremedia::kCMTimeFlags_Valid,
            epoch: 0,
        };
        let duration = super::coremedia::CMTime {
            value: reply.duration_value,
            timescale: reply.duration_timescale,
            flags: super::coremedia::kCMTimeFlags_Valid,
            epoch: 0,
        };

        cb(
            refcon,
            std::ptr::null_mut(), // source_frame_ref_con — VT-only feature, not surfaced
            status_code as i32,
            0, // info_flags
            pixel_buffer as *mut c_void,
            pts,
            duration,
        );

        // The user callback may have retained the pixel buffer.
        // Release our +1 retain regardless; if the user retained,
        // their reference keeps the buffer alive.
        crate::runtime::CFRelease(pixel_buffer as CFTypeRef);

        // Worker recycles its slot.
        let _ = crate::transport::release_decoded_frame(
            &vt_ferry_protocol::ReleaseDecodedFramePayload {
                session_id,
                output_id: reply.output_id,
            },
        );
    }
}

/// Compute a buffer layout for a decoded pixel format. Mirrors
/// the worker's `vtf_fill_buffer_layout` — same stride math, so
/// guest and worker agree on plane offsets within the inline
/// pixel-bytes payload returned by `OP_READ_DECODED_FRAME`.
fn vtf_decode_layout_for_format(
    width: u32,
    height: u32,
    pixel_format: u32,
    layout: &mut vt_ferry_protocol::BufferLayoutReply,
) {
    *layout = vt_ferry_protocol::BufferLayoutReply::zeroed();
    let align64 = |v: u32| (v + 63) & !63;
    match pixel_format {
        0x34323076 | 0x34323066 => {
            // NV12 video / full range — Y + interleaved CbCr at
            // half resolution, 1 byte per sample.
            let stride = align64(width);
            layout.plane_count = 2;
            layout.plane_widths = [width, width / 2, 0, 0];
            layout.plane_heights = [height, height / 2, 0, 0];
            layout.plane_bytes_per_row = [stride, stride, 0, 0];
            layout.plane_offsets = [0, stride * height, 0, 0];
            layout.total_size = layout.plane_offsets[1] + stride * (height / 2);
        }
        0x78343230 | 0x78663230 => {
            // P010 — 10-bit bi-planar, 2 bytes per sample.
            let stride = align64(width * 2);
            layout.plane_count = 2;
            layout.plane_widths = [width, width / 2, 0, 0];
            layout.plane_heights = [height, height / 2, 0, 0];
            layout.plane_bytes_per_row = [stride, stride, 0, 0];
            layout.plane_offsets = [0, stride * height, 0, 0];
            layout.total_size = layout.plane_offsets[1] + stride * (height / 2);
        }
        _ => {
            // BGRA / packed RGB fall-through — 4 bytes per pixel.
            let stride = align64(width * 4);
            layout.plane_count = 1;
            layout.plane_widths = [width, 0, 0, 0];
            layout.plane_heights = [height, 0, 0, 0];
            layout.plane_bytes_per_row = [stride, 0, 0, 0];
            layout.total_size = stride * height;
        }
    }
}

/// VTDecompressionSessionDecodeFrame proxy entrypoint.
///
/// Submits the encoded frame to the worker via
/// `OP_ENQUEUE_ENCODED_FRAME`, then synchronously drains any
/// decoded outputs the callback has queued. Each decoded frame
/// fires the user's output callback with a guest-side
/// CVPixelBuffer proxy wrapping the inline pixel bytes.
#[no_mangle]
pub unsafe extern "C" fn VTDecompressionSessionDecodeFrame(
    session: *mut vtf_vt_decompression_session,
    sample_buffer: *mut c_void,
    _decode_flags: u32,
    _source_frame_ref_con: *mut c_void,
    info_flags_out: *mut u32,
) -> i32 {
    if session.is_null() || sample_buffer.is_null() {
        return -12902;
    }
    if (*session).invalidated {
        return -12903; // kVTInvalidSessionErr
    }
    if !info_flags_out.is_null() {
        *info_flags_out = 0;
    }

    // Extract encoded bytes from the sample buffer's data buffer.
    let sample = sample_buffer as *const super::coremedia::vtf_cm_sample_buffer;
    let block_buffer_ref = (*sample).data_buffer;
    if block_buffer_ref.is_null() {
        return -12902;
    }
    let block = block_buffer_ref as *const super::coremedia::vtf_cm_block_buffer;
    if (*block).bytes.is_null() || (*block).length == 0 {
        return -12902;
    }
    let encoded_bytes = std::slice::from_raw_parts((*block).bytes, (*block).length);

    // Phase 15: deferred SET_DECODE_FORMAT. If the session was
    // created with a placeholder format description (FFmpeg's
    // hwaccel videotoolbox path), `format_set` is still false
    // here. Walk the AVCC-framed bitstream the caller is about
    // to send and pull SPS/PPS (or VPS+SPS+PPS for HEVC) out of
    // it. Ship `OP_SET_DECODE_FORMAT` so the worker can build
    // the real `VTDecompressionSession`, then continue with the
    // normal `OP_ENQUEUE_ENCODED_FRAME` path. Frames before the
    // first IDR have no parameter sets — those get rejected
    // with -12903, which matches what Apple's VT does for the
    // same condition (decoder waits for a keyframe).
    if !(*session).format_set {
        return -12903; // kVTInvalidSessionErr
    }

    // Pick inline vs chunked based on encoded frame size. Frames
    // ≤ VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES (4 MiB) take the
    // existing single-shot OP_ENQUEUE_ENCODED_FRAME path; frames
    // larger than that go through OP_ENQUEUE_ENCODED_FRAME_CHUNK
    // (added in the post-Phase-16 follow-up). 4 MiB covers every
    // 4K H.264 / HEVC IDR we've seen in v1 content; the chunked
    // path only fires for 8K or extreme-bitrate 4K content beyond
    // the v1 scope, but its presence means a future content
    // surprise can't strand the IDR like the original 256 KiB cap
    // did (see Phase 16 commit message).
    if encoded_bytes.len()
        <= vt_ferry_protocol::VTF_TRANSPORT_MAX_ENCODED_FRAME_BYTES as usize
    {
        return vtf_decompression_enqueue_inline(session, sample, encoded_bytes);
    }
    vtf_decompression_enqueue_chunked(session, sample, encoded_bytes)
}

unsafe fn vtf_decompression_enqueue_inline(
    session: *mut vtf_vt_decompression_session,
    sample: *const super::coremedia::vtf_cm_sample_buffer,
    encoded_bytes: &[u8],
) -> i32 {
    // Build the enqueue payload with PTS / duration from the
    // sample buffer.
    let mut payload = vt_ferry_protocol::EnqueueEncodedFramePayload::zeroed();
    payload.session_id = (*session).base.proxy_id;
    payload.encoded_size = encoded_bytes.len() as u32;
    payload.pts_value = (*sample).presentation_timestamp.value;
    payload.pts_timescale = (*sample).presentation_timestamp.timescale;
    payload.duration_value = 0;
    payload.duration_timescale = 0;

    if let Err(status) = crate::transport::enqueue_encoded_frame(&payload, encoded_bytes) {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:decompression_decode_frame ENQUEUE failed status={status}"
        ));
        return status;
    }

    // Synchronous drain: pull any decoded frames the worker has
    // ready and fire callbacks. Decoupled from the encode-side
    // callback model; matches what FFmpeg's videotoolbox decoder
    // expects (it doesn't rely on async delivery for v1).
    vtf_drain_decoded_frames(session)
}

unsafe fn vtf_decompression_enqueue_chunked(
    session: *mut vtf_vt_decompression_session,
    sample: *const super::coremedia::vtf_cm_sample_buffer,
    encoded_bytes: &[u8],
) -> i32 {
    let mut head_payload =
        vt_ferry_protocol::EnqueueEncodedFrameChunkPayload::zeroed();
    head_payload.session_id = (*session).base.proxy_id;
    head_payload.pts_value = (*sample).presentation_timestamp.value;
    head_payload.pts_timescale = (*sample).presentation_timestamp.timescale;
    head_payload.duration_value = 0;
    head_payload.duration_timescale = 0;

    if let Err(status) =
        crate::transport::enqueue_encoded_frame_chunked(&head_payload, encoded_bytes)
    {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:decompression_decode_frame chunked ENQUEUE failed \
             status={status} (frame size {} bytes)",
            encoded_bytes.len()
        ));
        return status;
    }

    vtf_drain_decoded_frames(session)
}

/// VTDecompressionSessionWaitForAsynchronousFrames — flushes any
/// in-flight VT decode work and then drains the decoded queue
/// into the user's output callback.
///
/// Two-step:
///   1. OP_DRAIN tells the worker to call
///      VTDecompressionSessionWaitForAsynchronousFrames host-side.
///      That blocks until all frames VT was processing have
///      either landed in the DecodedOutputQueue or been
///      definitively dropped. Without this, our local drain
///      could return before VT has emitted the tail of the
///      stream and FFmpeg would lose those frames.
///   2. Local drain pulls everything the worker queued and fires
///      the user's output callback.
#[no_mangle]
pub unsafe extern "C" fn VTDecompressionSessionWaitForAsynchronousFrames(
    session: *mut vtf_vt_decompression_session,
) -> i32 {
    if session.is_null() || (*session).invalidated {
        return 0;
    }
    if let Err(status) = crate::transport::drain((*session).base.proxy_id) {
        crate::runtime::vtf_guest_trace(&format!(
            "videotoolbox:wait_for_async DRAIN failed status={status}"
        ));
        return -12902;
    }
    vtf_drain_decoded_frames(session)
}

/// VTDecompressionSessionFinishDelayedFrames — same as
/// WaitForAsynchronousFrames in our synchronous drain model.
/// FFmpeg calls one or the other depending on the code path.
#[no_mangle]
pub unsafe extern "C" fn VTDecompressionSessionFinishDelayedFrames(
    session: *mut vtf_vt_decompression_session,
) -> i32 {
    VTDecompressionSessionWaitForAsynchronousFrames(session)
}

#[cfg(test)]
mod inband_parameter_set_tests {
    //! Phase 15: pull SPS/PPS (or VPS+SPS+PPS for HEVC) out of
    //! the AVCC-framed bitstream FFmpeg's `-hwaccel videotoolbox`
    //! delivers in each `CMSampleBuffer`'s data buffer. Without
    //! this, the placeholder format description created by
    //! FFmpeg's `videotoolbox_format_desc_create` carries no
    //! parameter sets and `OP_SET_DECODE_FORMAT` can't go out.
    use super::*;

    const FOURCC_AVC1: u32 = 0x6176_6331;
    const FOURCC_HVC1: u32 = 0x6876_6331;

    /// Build an AVCC-framed bitstream by length-prefixing each
    /// NAL with 4 big-endian bytes. Mirrors what FFmpeg's
    /// `videotoolbox_h264_end_frame` constructs.
    fn build_avcc(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            let len = nal.len() as u32;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(nal);
        }
        out
    }

    #[test]
    fn split_avcc_recovers_each_nal() {
        let bs = build_avcc(&[&[0x67, 0x42], &[0x68, 0x99], &[0x65, 0x88, 0x84]]);
        let parsed = vtf_split_avcc_bitstream(&bs, 4);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], &[0x67, 0x42]);
        assert_eq!(parsed[1], &[0x68, 0x99]);
        assert_eq!(parsed[2], &[0x65, 0x88, 0x84]);
    }

    #[test]
    fn split_avcc_stops_on_truncation() {
        // Length field promises 64 bytes but only 4 follow → stop
        // gracefully instead of panicking.
        let mut bs = vec![];
        bs.extend_from_slice(&64u32.to_be_bytes());
        bs.extend_from_slice(&[0x67, 0x42, 0xc0, 0x1f]);
        let parsed = vtf_split_avcc_bitstream(&bs, 4);
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn nal_unit_type_classifies_h264() {
        // SPS NAL header: 0x67 = forbidden(0) + nal_ref_idc(11) +
        //                   nal_unit_type(7) → type 7
        assert_eq!(vtf_nal_unit_type(&[0x67, 0x42], FOURCC_AVC1), Some(7));
        // PPS: 0x68 → type 8
        assert_eq!(vtf_nal_unit_type(&[0x68, 0x99], FOURCC_AVC1), Some(8));
        // IDR slice: 0x65 → type 5
        assert_eq!(vtf_nal_unit_type(&[0x65, 0x88, 0x84], FOURCC_AVC1), Some(5));
    }

    #[test]
    fn nal_unit_type_classifies_hevc() {
        // HEVC NAL header is 2 bytes: type = (first_byte >> 1) & 0x3F
        // VPS=32 → first_byte = 32 << 1 = 0x40
        assert_eq!(vtf_nal_unit_type(&[0x40, 0x01, 0x0c], FOURCC_HVC1), Some(32));
        // SPS=33 → first_byte = 33 << 1 = 0x42
        assert_eq!(vtf_nal_unit_type(&[0x42, 0x01, 0x01], FOURCC_HVC1), Some(33));
        // PPS=34 → first_byte = 34 << 1 = 0x44
        assert_eq!(vtf_nal_unit_type(&[0x44, 0x01, 0xc0], FOURCC_HVC1), Some(34));
    }

    #[test]
    fn pack_inband_extracts_h264_sps_pps_from_idr_frame() {
        // Realistic IDR access unit: SPS + PPS + IDR slice. Bytes
        // are abbreviated — the helper only inspects NAL type bits.
        let sps = [0x67u8, 0x42, 0xc0, 0x1f, 0x95, 0xa0, 0x14, 0x01, 0x6e, 0xc0];
        let pps = [0x68u8, 0xce, 0x06, 0xe2];
        let idr = [0x65u8, 0x88, 0x84, 0x00, 0x00, 0x00];
        let bs = build_avcc(&[&sps, &pps, &idr]);

        let payload = vtf_pack_decode_format_payload_from_inband(
            42,
            FOURCC_AVC1,
            1920,
            1080,
            &bs,
        )
        .expect("should extract SPS+PPS");

        assert_eq!(payload.session_id, 42);
        assert_eq!(payload.codec, FOURCC_AVC1);
        assert_eq!(payload.parameter_set_count, 2);
        assert_eq!(payload.nal_header_length, 4);
        assert_eq!(payload.parameter_set_sizes[0] as usize, sps.len());
        assert_eq!(payload.parameter_set_sizes[1] as usize, pps.len());
        // SPS bytes appear first in the packed buffer.
        assert_eq!(&payload.parameter_set_data[..sps.len()], &sps);
        // PPS follows immediately after SPS.
        assert_eq!(
            &payload.parameter_set_data[sps.len()..sps.len() + pps.len()],
            &pps
        );
    }

    #[test]
    fn pack_inband_extracts_hevc_vps_sps_pps_in_correct_order() {
        // HEVC IDR: VPS + SPS + PPS + IDR_W_RADL slice.
        // (NAL types: 32, 33, 34, 19)
        let vps = [0x40u8, 0x01, 0x0c, 0x01, 0xff, 0xff];
        let sps = [0x42u8, 0x01, 0x01, 0x01, 0x60, 0x00];
        let pps = [0x44u8, 0x01, 0xc0, 0xf3, 0xc0];
        let idr = [0x26u8, 0x01, 0xaf, 0x66]; // type 19 << 1 = 0x26
        let bs = build_avcc(&[&vps, &sps, &pps, &idr]);

        let payload = vtf_pack_decode_format_payload_from_inband(
            7,
            FOURCC_HVC1,
            3840,
            2160,
            &bs,
        )
        .expect("should extract VPS+SPS+PPS");

        assert_eq!(payload.parameter_set_count, 3);
        assert_eq!(payload.parameter_set_sizes[0] as usize, vps.len());
        assert_eq!(payload.parameter_set_sizes[1] as usize, sps.len());
        assert_eq!(payload.parameter_set_sizes[2] as usize, pps.len());
        // Order matters: worker's
        // CMVideoFormatDescriptionCreateFromHEVCParameterSets
        // expects [VPS, SPS, PPS].
        let mut o = 0;
        assert_eq!(&payload.parameter_set_data[o..o + vps.len()], &vps);
        o += vps.len();
        assert_eq!(&payload.parameter_set_data[o..o + sps.len()], &sps);
        o += sps.len();
        assert_eq!(&payload.parameter_set_data[o..o + pps.len()], &pps);
    }

    #[test]
    fn pack_inband_returns_minus_12710_when_no_sps_yet() {
        // Slice-only access unit (no SPS/PPS) — caller hasn't seen
        // an IDR yet. Returns kCMFormatDescriptionError_InvalidParameter
        // so VTDecompressionSessionDecodeFrame can surface
        // -12903 to FFmpeg, which the hwaccel framework treats
        // as a per-frame error rather than a fatal session
        // failure.
        let slice = [0x41u8, 0xe0, 0x00, 0x00, 0x00]; // type 1 (non-IDR)
        let bs = build_avcc(&[&slice]);
        let result = vtf_pack_decode_format_payload_from_inband(
            1,
            FOURCC_AVC1,
            640,
            480,
            &bs,
        );
        assert!(matches!(result, Err(-12710)));
    }

    #[test]
    fn pack_inband_rejects_unknown_codec() {
        let bs = build_avcc(&[&[0x00]]);
        let result = vtf_pack_decode_format_payload_from_inband(
            1, 0x4d4a5045, // 'MJPE' — not avc1 or hvc1
            1920, 1080, &bs,
        );
        assert!(matches!(result, Err(-12902)));
    }

    #[test]
    fn pack_inband_rejects_oversized_parameter_sets() {
        // Parameter sets that together exceed
        // VTF_TRANSPORT_MAX_PARAMETER_SET_BYTES (128) get
        // rejected — caller would have to chunk or signal
        // unsupported codec back to FFmpeg.
        let big = vec![0x67u8; 100];
        let big2 = vec![0x68u8; 100];
        let bs = build_avcc(&[&big, &big2]);
        let result = vtf_pack_decode_format_payload_from_inband(
            1, FOURCC_AVC1, 1920, 1080, &bs,
        );
        assert!(matches!(result, Err(-12902)));
    }
}

#[cfg(test)]
mod session_discrimination_tests {
    //! Coverage for the VTSessionRef opaque-pointer type
    //! discrimination in VTSessionSet/CopyProperty. FFmpeg's
    //! videotoolbox decoder may pass a decompression session
    //! pointer to these encode-typed fns; the dispatch must read
    //! type_id off the vtf_cf_object base and route correctly.
    //! Without this, casting through the wrong struct layout
    //! would either misread fields or corrupt them.
    use super::*;

    /// Build a minimal vtf_vt_decompression_session for tests.
    /// Just enough to satisfy the type_id check + null pointer
    /// fields — bypasses the actual VT/transport setup that the
    /// real Create entrypoint does.
    unsafe fn synthetic_decompression_session() -> *mut vtf_vt_decompression_session {
        Box::into_raw(Box::new(vtf_vt_decompression_session {
            base: vtf_cf_object {
                magic: 0x534d5654,
                type_id: crate::runtime::VTF_TYPE_VT_DECOMPRESSION_SESSION,
                refcount: std::sync::atomic::AtomicI32::new(1),
                flags: 0,
                proxy_id: 7777, // arbitrary; never sent over wire in this test
                generation: 1,
                host_id: 0,
                finalize: None, // skip recursion through finalize on free
            },
            codec_type: 0x6176_6331,
            width: 1280,
            height: 720,
            output_callback: None,
            output_refcon: std::ptr::null_mut(),
            format_description: std::ptr::null(),
            format_set: false,
            invalidated: false,
            chunked_output: false,
        }))
    }

    #[test]
    fn vt_session_set_property_silently_accepts_decode_session() {
        // Decode session passed to the encode-typed VTSessionSetProperty
        // must return 0 (noErr) and NOT touch encode-struct fields.
        unsafe {
            let session = synthetic_decompression_session();
            // Build a fake key/value via static booleans (which exist
            // unconditionally — no transport involvement).
            let key = crate::corefoundation::kCFBooleanTrue.0 as CFStringRef;
            let value = crate::corefoundation::kCFBooleanTrue.0;
            let result = VTSessionSetProperty(
                session as *mut vtf_vt_session,
                key,
                value,
            );
            assert_eq!(result, 0, "decode session must accept SetProperty as no-op");
            // The decode session struct must be unchanged.
            assert!(!(*session).invalidated);
            assert!(!(*session).format_set);
            // Tear down without invoking the (None) finalize.
            let _ = Box::from_raw(session);
        }
    }

    #[test]
    fn vt_session_copy_property_returns_null_on_decode_session() {
        unsafe {
            let session = synthetic_decompression_session();
            let key = crate::corefoundation::kCFBooleanTrue.0 as CFStringRef;
            let mut value_out: *mut c_void = 0xdeadbeef as *mut c_void;
            let result = VTSessionCopyProperty(
                session as *mut vtf_vt_session,
                key,
                std::ptr::null(),
                &mut value_out,
            );
            assert_eq!(result, 0);
            assert!(value_out.is_null(), "decode session must zero out the result");
            let _ = Box::from_raw(session);
        }
    }
}

#[cfg(test)]
mod key_helper_tests {
    //! Pure-function coverage for the fixed-buffer key copy helpers.
    //! These are the call sites that build SetPropertyPayload's
    //! `property_key` field — silent truncation or missing null
    //! termination here would produce malformed protocol messages.
    use super::*;

    #[test]
    fn fill_key_bytes_zeros_buffer_and_copies_short_key() {
        let mut buf = [0xffu8; 64];
        fill_key_bytes(&mut buf, "RealTime");
        assert_eq!(&buf[..8], b"RealTime");
        // Everything past the key must be zeroed (null-terminator
        // and beyond) because the consumer reads up to the first 0.
        for (i, b) in buf.iter().enumerate().skip(8) {
            assert_eq!(*b, 0, "byte {i} must be zero");
        }
    }

    #[test]
    fn fill_key_bytes_handles_empty_key() {
        let mut buf = [0xffu8; 64];
        fill_key_bytes(&mut buf, "");
        // All-zero buffer; consumer reads "no key" because there's
        // nothing before the null terminator.
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn fill_key_bytes_truncates_oversized_key_and_keeps_null() {
        // 100-byte key exceeds the 64-byte buffer. Helper must
        // truncate to 63 bytes (saving one for the null terminator)
        // so the consumer always has a sentinel to stop at.
        let oversized: String = "k".repeat(100);
        let mut buf = [0xffu8; 64];
        fill_key_bytes(&mut buf, &oversized);
        assert_eq!(buf[..63], [b'k'; 63]);
        assert_eq!(buf[63], 0, "last byte must be the null terminator");
    }

    #[test]
    fn fill_key_bytes_at_capacity_minus_one_fits_with_null() {
        // 63-byte key — the largest that fits without truncation.
        let key: String = "x".repeat(63);
        let mut buf = [0xffu8; 64];
        fill_key_bytes(&mut buf, &key);
        assert_eq!(&buf[..63], key.as_bytes());
        assert_eq!(buf[63], 0);
    }

    #[test]
    fn fill_small_key_bytes_uses_32_byte_buffer_with_same_semantics() {
        let mut buf = [0xffu8; 32];
        fill_small_key_bytes(&mut buf, "Hello");
        assert_eq!(&buf[..5], b"Hello");
        for (i, b) in buf.iter().enumerate().skip(5) {
            assert_eq!(*b, 0, "byte {i} must be zero");
        }
    }

    #[test]
    fn fill_small_key_bytes_truncates_at_31() {
        let oversized: String = "y".repeat(50);
        let mut buf = [0xffu8; 32];
        fill_small_key_bytes(&mut buf, &oversized);
        assert_eq!(buf[..31], [b'y'; 31]);
        assert_eq!(buf[31], 0, "last byte must be the null terminator");
    }
}
