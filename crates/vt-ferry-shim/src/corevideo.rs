#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::corefoundation::{vtf_cf_string, CFDictionaryRef};
use crate::coremedia::ExportedSymbol;
use crate::runtime::*;
use std::ffi::c_void;

pub type CVReturn = i32;
pub const kCVReturnSuccess: i32 = 0;
pub const kCVReturnInvalidArgument: i32 = -6661;
pub const kCVReturnError: i32 = -6660;
pub const VTF_GUEST_POOL_DEFAULT_BUFFER_COUNT: u32 = 4;
pub const VTF_CV_BUFFER_STATE_GUEST_WRITABLE: u32 = 0;
pub const VTF_CV_BUFFER_STATE_QUEUED_TO_HOST: u32 = 1;
pub const VTF_CV_BUFFER_STATE_RECYCLED: u32 = 2;

pub type CVPixelBufferRef = CFTypeRef;
pub type CVPixelBufferPoolRef = CFTypeRef;
pub type CVOptionFlags = u32;
pub type CVAttachmentMode = u32;

pub const kCVAttachmentMode_ShouldNotPropagate: CVAttachmentMode = 0;
pub const kCVAttachmentMode_ShouldPropagate: CVAttachmentMode = 1;
pub const kCVPixelBufferLock_ReadOnly: CVOptionFlags = 0x00000001u32;

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
            length: $literal.len() - 1, // minus null terminator
            owns_bytes: false,
        };
    };
}

macro_rules! export_string_key {
    ($name:ident, $storage:ident) => {
        #[no_mangle]
        pub static $name: ExportedSymbol =
            ExportedSymbol(&raw const $storage as *const _ as *const c_void);
    };
}

define_string_key!(
    vtf_kCVPixelBufferPixelFormatTypeKey_storage,
    b"PixelFormatType\0"
);
export_string_key!(
    kCVPixelBufferPixelFormatTypeKey,
    vtf_kCVPixelBufferPixelFormatTypeKey_storage
);

define_string_key!(vtf_kCVPixelBufferWidthKey_storage, b"Width\0");
export_string_key!(kCVPixelBufferWidthKey, vtf_kCVPixelBufferWidthKey_storage);

define_string_key!(vtf_kCVPixelBufferHeightKey_storage, b"Height\0");
export_string_key!(
    kCVPixelBufferHeightKey,
    vtf_kCVPixelBufferHeightKey_storage
);

define_string_key!(
    vtf_kCVPixelBufferBytesPerRowAlignmentKey_storage,
    b"BytesPerRowAlignment\0"
);
export_string_key!(
    kCVPixelBufferBytesPerRowAlignmentKey,
    vtf_kCVPixelBufferBytesPerRowAlignmentKey_storage
);

define_string_key!(
    vtf_kCVPixelBufferIOSurfacePropertiesKey_storage,
    b"IOSurfaceProperties\0"
);
export_string_key!(
    kCVPixelBufferIOSurfacePropertiesKey,
    vtf_kCVPixelBufferIOSurfacePropertiesKey_storage
);

define_string_key!(
    vtf_kCVPixelBufferOpenGLESCompatibilityKey_storage,
    b"OpenGLESCompatibility\0"
);
export_string_key!(
    kCVPixelBufferOpenGLESCompatibilityKey,
    vtf_kCVPixelBufferOpenGLESCompatibilityKey_storage
);

define_string_key!(
    vtf_kCVPixelBufferIOSurfaceOpenGLTextureCompatibilityKey_storage,
    b"IOSurfaceOpenGLTextureCompatibility\0"
);
export_string_key!(
    kCVPixelBufferIOSurfaceOpenGLTextureCompatibilityKey,
    vtf_kCVPixelBufferIOSurfaceOpenGLTextureCompatibilityKey_storage
);

define_string_key!(
    vtf_kCVImageBufferPixelAspectRatioKey_storage,
    b"PixelAspectRatio\0"
);
export_string_key!(
    kCVImageBufferPixelAspectRatioKey,
    vtf_kCVImageBufferPixelAspectRatioKey_storage
);

define_string_key!(
    vtf_kCVImageBufferPixelAspectRatioHorizontalSpacingKey_storage,
    b"PixelAspectRatioHorizontalSpacing\0"
);
export_string_key!(
    kCVImageBufferPixelAspectRatioHorizontalSpacingKey,
    vtf_kCVImageBufferPixelAspectRatioHorizontalSpacingKey_storage
);

define_string_key!(
    vtf_kCVImageBufferPixelAspectRatioVerticalSpacingKey_storage,
    b"PixelAspectRatioVerticalSpacing\0"
);
export_string_key!(
    kCVImageBufferPixelAspectRatioVerticalSpacingKey,
    vtf_kCVImageBufferPixelAspectRatioVerticalSpacingKey_storage
);

define_string_key!(vtf_kCVImageBufferYCbCrMatrixKey_storage, b"YCbCrMatrix\0");
export_string_key!(
    kCVImageBufferYCbCrMatrixKey,
    vtf_kCVImageBufferYCbCrMatrixKey_storage
);

define_string_key!(
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_709_2_storage,
    b"ITU_R_709_2\0"
);
export_string_key!(
    kCVImageBufferYCbCrMatrix_ITU_R_709_2,
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_709_2_storage
);

define_string_key!(
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_601_4_storage,
    b"ITU_R_601_4\0"
);
export_string_key!(
    kCVImageBufferYCbCrMatrix_ITU_R_601_4,
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_601_4_storage
);

define_string_key!(
    vtf_kCVImageBufferYCbCrMatrix_SMPTE_240M_1995_storage,
    b"SMPTE_240M_1995\0"
);
export_string_key!(
    kCVImageBufferYCbCrMatrix_SMPTE_240M_1995,
    vtf_kCVImageBufferYCbCrMatrix_SMPTE_240M_1995_storage
);

define_string_key!(
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_2020_storage,
    b"ITU_R_2020\0"
);
export_string_key!(
    kCVImageBufferYCbCrMatrix_ITU_R_2020,
    vtf_kCVImageBufferYCbCrMatrix_ITU_R_2020_storage
);

define_string_key!(
    vtf_kCVImageBufferColorPrimariesKey_storage,
    b"ColorPrimaries\0"
);
export_string_key!(
    kCVImageBufferColorPrimariesKey,
    vtf_kCVImageBufferColorPrimariesKey_storage
);

define_string_key!(
    vtf_kCVImageBufferColorPrimaries_ITU_R_709_2_storage,
    b"ITU_R_709_2\0"
);
export_string_key!(
    kCVImageBufferColorPrimaries_ITU_R_709_2,
    vtf_kCVImageBufferColorPrimaries_ITU_R_709_2_storage
);

define_string_key!(
    vtf_kCVImageBufferColorPrimaries_SMPTE_C_storage,
    b"SMPTE_C\0"
);
export_string_key!(
    kCVImageBufferColorPrimaries_SMPTE_C,
    vtf_kCVImageBufferColorPrimaries_SMPTE_C_storage
);

define_string_key!(
    vtf_kCVImageBufferColorPrimaries_EBU_3213_storage,
    b"EBU_3213\0"
);
export_string_key!(
    kCVImageBufferColorPrimaries_EBU_3213,
    vtf_kCVImageBufferColorPrimaries_EBU_3213_storage
);

define_string_key!(
    vtf_kCVImageBufferColorPrimaries_ITU_R_2020_storage,
    b"ITU_R_2020\0"
);
export_string_key!(
    kCVImageBufferColorPrimaries_ITU_R_2020,
    vtf_kCVImageBufferColorPrimaries_ITU_R_2020_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunctionKey_storage,
    b"TransferFunction\0"
);
export_string_key!(
    kCVImageBufferTransferFunctionKey,
    vtf_kCVImageBufferTransferFunctionKey_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_ITU_R_709_2_storage,
    b"ITU_R_709_2\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_ITU_R_709_2,
    vtf_kCVImageBufferTransferFunction_ITU_R_709_2_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_SMPTE_240M_1995_storage,
    b"SMPTE_240M_1995\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_SMPTE_240M_1995,
    vtf_kCVImageBufferTransferFunction_SMPTE_240M_1995_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_UseGamma_storage,
    b"UseGamma\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_UseGamma,
    vtf_kCVImageBufferTransferFunction_UseGamma_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_ITU_R_2020_storage,
    b"ITU_R_2020\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_ITU_R_2020,
    vtf_kCVImageBufferTransferFunction_ITU_R_2020_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_SMPTE_ST_428_1_storage,
    b"SMPTE_ST_428_1\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_SMPTE_ST_428_1,
    vtf_kCVImageBufferTransferFunction_SMPTE_ST_428_1_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ_storage,
    b"SMPTE_ST_2084_PQ\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ,
    vtf_kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ_storage
);

define_string_key!(
    vtf_kCVImageBufferTransferFunction_ITU_R_2100_HLG_storage,
    b"ITU_R_2100_HLG\0"
);
export_string_key!(
    kCVImageBufferTransferFunction_ITU_R_2100_HLG,
    vtf_kCVImageBufferTransferFunction_ITU_R_2100_HLG_storage
);

define_string_key!(vtf_kCVImageBufferGammaLevelKey_storage, b"GammaLevel\0");
export_string_key!(
    kCVImageBufferGammaLevelKey,
    vtf_kCVImageBufferGammaLevelKey_storage
);

define_string_key!(
    vtf_kCVImageBufferCGColorSpaceKey_storage,
    b"CGColorSpace\0"
);
export_string_key!(
    kCVImageBufferCGColorSpaceKey,
    vtf_kCVImageBufferCGColorSpaceKey_storage
);

define_string_key!(vtf_kCVImageBufferChromaLocation_Left_storage, b"Left\0");
export_string_key!(
    kCVImageBufferChromaLocation_Left,
    vtf_kCVImageBufferChromaLocation_Left_storage
);

define_string_key!(
    vtf_kCVImageBufferChromaLocation_Center_storage,
    b"Center\0"
);
export_string_key!(
    kCVImageBufferChromaLocation_Center,
    vtf_kCVImageBufferChromaLocation_Center_storage
);

define_string_key!(vtf_kCVImageBufferChromaLocation_Top_storage, b"Top\0");
export_string_key!(
    kCVImageBufferChromaLocation_Top,
    vtf_kCVImageBufferChromaLocation_Top_storage
);

define_string_key!(
    vtf_kCVImageBufferChromaLocation_Bottom_storage,
    b"Bottom\0"
);
export_string_key!(
    kCVImageBufferChromaLocation_Bottom,
    vtf_kCVImageBufferChromaLocation_Bottom_storage
);

define_string_key!(
    vtf_kCVImageBufferChromaLocation_TopLeft_storage,
    b"TopLeft\0"
);
export_string_key!(
    kCVImageBufferChromaLocation_TopLeft,
    vtf_kCVImageBufferChromaLocation_TopLeft_storage
);

define_string_key!(
    vtf_kCVImageBufferChromaLocation_BottomLeft_storage,
    b"BottomLeft\0"
);
export_string_key!(
    kCVImageBufferChromaLocation_BottomLeft,
    vtf_kCVImageBufferChromaLocation_BottomLeft_storage
);

define_string_key!(
    vtf_kCVImageBufferChromaLocationTopFieldKey_storage,
    b"ChromaLocationTopField\0"
);
export_string_key!(
    kCVImageBufferChromaLocationTopFieldKey,
    vtf_kCVImageBufferChromaLocationTopFieldKey_storage
);

/// Read a single CFNumber-valued attribute out of a pool's
/// `pixelBufferAttributes` dictionary. The keys in the dict are
/// the `kCVPixelBuffer*Key` symbols we export — `ExportedSymbol`'s
/// inner pointer is what the caller dropped in via
/// `CFDictionarySetValue`, so `key_ptr` is just `<key>.0`.
///
/// Returns `Some(value)` if the key is present and CFNumber-typed,
/// `None` otherwise. Callers fall back to a default rather than
/// failing the whole pool create — `pixelBufferAttributes` is
/// nominally optional, but every real caller (FFmpeg's hwaccel
/// code in particular) populates width/height/pixel-format.
unsafe fn read_pool_attribute_int(
    attrs: CFDictionaryRef,
    key_ptr: *const c_void,
) -> Option<i64> {
    if attrs.is_null() || key_ptr.is_null() {
        return None;
    }
    let value = crate::corefoundation::CFDictionaryGetValue(attrs, key_ptr);
    if value.is_null() {
        return None;
    }
    let mut out: i64 = 0;
    let ok = crate::corefoundation::CFNumberGetValue(
        value as crate::corefoundation::CFNumberRef,
        crate::corefoundation::K_CF_NUMBER_SINT64_TYPE,
        &mut out as *mut _ as *mut c_void,
    );
    if ok != 0 {
        Some(out)
    } else {
        // Some callers (e.g. FFmpeg via av_dict_set_int) end up
        // creating CFNumbers as SInt32; retry that type on
        // failure rather than wedging.
        let mut out32: i32 = 0;
        let ok32 = crate::corefoundation::CFNumberGetValue(
            value as crate::corefoundation::CFNumberRef,
            crate::corefoundation::K_CF_NUMBER_SINT32_TYPE,
            &mut out32 as *mut _ as *mut c_void,
        );
        if ok32 != 0 {
            Some(out32 as i64)
        } else {
            None
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferPoolCreate(
    _allocator: CFTypeRef,
    _poolAttributes: CFDictionaryRef,
    pixelBufferAttributes: CFDictionaryRef,
    poolOut: *mut CVPixelBufferPoolRef,
) -> CVReturn {
    if !crate::transport::is_enabled() {
        return kCVReturnError;
    }

    // Read width / height / pixel-format from the
    // `pixelBufferAttributes` dictionary. The previous stub
    // hardcoded 640×360 NV12, which made `-hwaccel videotoolbox`
    // unconditionally fail (`AVHWFramesContext` couldn't get a
    // matching pool from the worker, so FFmpeg silently
    // fell back to libavcodec native decode and our "VT decode"
    // claim was a transparent fiction). Reading the real
    // dimensions from `pixelBufferAttributes` is what FFmpeg's
    // VT hwaccel actually configures via
    // `av_hwframe_ctx_init` → `cv_pixbuf_pool_create`.
    let width_attr = read_pool_attribute_int(
        pixelBufferAttributes,
        crate::corevideo::kCVPixelBufferWidthKey.0,
    );
    let height_attr = read_pool_attribute_int(
        pixelBufferAttributes,
        crate::corevideo::kCVPixelBufferHeightKey.0,
    );
    let pf_attr = read_pool_attribute_int(
        pixelBufferAttributes,
        crate::corevideo::kCVPixelBufferPixelFormatTypeKey.0,
    );

    let mut payload: vt_ferry_protocol::CreateBufferPoolPayload = unsafe { std::mem::zeroed() };
    payload.session_id = 0;
    // Defaults match the historical stub for callers who pass
    // a null `pixelBufferAttributes` — keeps the protocol shape
    // observable from the worker side identical to the pre-fix
    // path when no attributes are supplied. Real callers always
    // set all three.
    payload.width = width_attr.unwrap_or(640) as u32;
    payload.height = height_attr.unwrap_or(360) as u32;
    payload.pixel_format = pf_attr.unwrap_or(0x34323076) as u32; // '420v' (NV12)

    crate::runtime::vtf_guest_trace(&format!(
        "corevideo:CVPixelBufferPoolCreate width={} height={} pixel_format=0x{:08x} \
         attrs_present={}",
        payload.width,
        payload.height,
        payload.pixel_format,
        !pixelBufferAttributes.is_null(),
    ));

    match crate::transport::create_buffer_pool(&payload) {
        Ok(reply) => {
            // Wire the finalizer so CFRelease at refcount 0
            // sends OP_DESTROY_BUFFER_POOL to the worker. Without
            // this, FFmpeg's `-hwaccel videotoolbox` would leak
            // the pool's IOSurfacePoolDirectory entry for the
            // rest of the connection's lifetime — starving the
            // encoder's pool create when both decoder and encoder
            // are active in the same process (i.e. transcode).
            let pool = Box::into_raw(Box::new(vtf_cv_pixel_buffer_pool {
                base: vtf_cf_object::with_host_id(
                    crate::runtime::VTF_TYPE_PIXEL_BUFFER_POOL,
                    Some(vtf_finalize_pixel_buffer_pool),
                    reply.pool_id,
                    reply.pool_id,
                    1,
                ),
                width: payload.width as usize,
                height: payload.height as usize,
                pixel_format: payload.pixel_format,
                host_backing_kind: vt_ferry_protocol::VTF_HOST_BACKING_KIND_UNKNOWN,
                slot_count: reply.slot_count as usize,
                buffer_region_size: reply.buffer_region_size as usize,
                layout: reply.layout,
                owner_session: std::ptr::null_mut(),
                pixel_buffer_attributes: std::ptr::null(),
            }));

            if !poolOut.is_null() {
                *poolOut = pool as CVPixelBufferPoolRef;
            }
            return kCVReturnSuccess;
        }
        Err(e) => {
            eprintln!(
                "create_buffer_pool failed for {}x{} pf=0x{:08x}: status {}",
                payload.width, payload.height, payload.pixel_format, e
            );
            kCVReturnError
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferPoolCreatePixelBuffer(
    _allocator: CFTypeRef,
    pixelBufferPool: CVPixelBufferPoolRef,
    pixelBufferOut: *mut CVPixelBufferRef,
) -> CVReturn {
    if pixelBufferPool.is_null() || pixelBufferOut.is_null() {
        return kCVReturnError;
    }
    let pool = pixelBufferPool as *mut vtf_cv_pixel_buffer_pool;

    let payload = vt_ferry_protocol::AllocBufferPayload {
        pool_id: (*pool).base.proxy_id,
    };
    crate::runtime::vtf_guest_trace(&format!(
        "corevideo:alloc_buffer pool_id={}",
        payload.pool_id
    ));
    match crate::transport::alloc_buffer(&payload) {
        Ok(reply) => {
            crate::runtime::vtf_guest_trace(&format!(
                "corevideo:alloc_buffer ok buffer_id={} generation={} slot_index={} slot_offset={} total_size={}",
                reply.buffer_id,
                reply.generation,
                reply.slot_index,
                reply.slot_offset,
                reply.layout.total_size
            ));
            let lease = vt_ferry_protocol::PoolBufferLeaseReply {
                buffer_id: reply.buffer_id,
                generation: reply.generation,
                slot_index: reply.slot_index,
                slot_offset: reply.slot_offset,
                host_backing_kind: reply.host_backing_kind,
                flags: 0,
            };
            let buffer = vtf_create_pixel_buffer_from_lease(pool, lease, reply.layout, false);
            if buffer.is_null() {
                return kCVReturnError;
            }

            *pixelBufferOut = buffer as CVPixelBufferRef;
            kCVReturnSuccess
        }
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "corevideo:alloc_buffer failed status={status}"
            ));
            kCVReturnError
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferRelease(buffer: CVPixelBufferRef) {
    if !buffer.is_null() {
        crate::runtime::CFRelease(buffer);
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferRetain(buffer: CVPixelBufferRef) -> CVPixelBufferRef {
    crate::runtime::CFRetain(buffer)
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferPoolRelease(pool: CVPixelBufferPoolRef) {
    if !pool.is_null() {
        crate::runtime::CFRelease(pool);
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferPoolGetPixelBufferAttributes(
    pool: CVPixelBufferPoolRef,
) -> CFDictionaryRef {
    if pool.is_null() {
        return std::ptr::null();
    }
    let pool = pool as *mut vtf_cv_pixel_buffer_pool;
    crate::runtime::CFRetain((*pool).pixel_buffer_attributes)
}

#[repr(C)]
pub struct vtf_cv_pixel_buffer {
    pub base: vtf_cf_object,
    pub pool_ref: *mut vtf_cv_pixel_buffer_pool,
    pub pool_host_id: u64,
    pub host_generation: u64,
    pub host_backing_kind: u32,
    pub recycled: bool,
    pub cache_valid: bool,
    pub backing_store_owned: bool,
    pub mapped_backing_active: bool,
    pub state: u32,
    pub slot_index: u32,
    pub slot_offset: u32,
    pub slot_region_size: u32,
    pub lock_flags: u64, // CVOptionFlags
    pub lock_snapshot: *mut u8,
    pub lock_snapshot_size: usize,
    pub backing_store: *mut u8,
    pub backing_store_size: usize,
    pub mapped_region_base: *mut u8,
    pub mapped_region_size: usize,
    pub total_size: usize,
    pub width: usize,
    pub height: usize,
    pub pixel_format: u32,
    pub plane_count: usize,
    pub plane_offsets: [usize; 4],
    pub plane_widths: [usize; 4],
    pub plane_heights: [usize; 4],
    pub plane_bytes_per_row: [usize; 4],
    pub plane_data: [*mut u8; 4],
    pub attachments: CFTypeRef, // CFMutableDictionaryRef
    pub locked: bool,
}

pub(crate) unsafe fn vtf_finalize_pixel_buffer(obj: *mut vtf_cf_object) {
    let buffer = obj as *mut vtf_cv_pixel_buffer;
    vtf_recycle_pixel_buffer(buffer);
    if !(*buffer).pool_ref.is_null() {
        crate::runtime::CFRelease((*buffer).pool_ref as CFTypeRef);
        (*buffer).pool_ref = std::ptr::null_mut();
    }
    if !(*buffer).attachments.is_null() {
        crate::runtime::CFRelease((*buffer).attachments);
    }
    if (*buffer).backing_store_owned && !(*buffer).backing_store.is_null() {
        let _ = Vec::from_raw_parts(
            (*buffer).backing_store,
            (*buffer).backing_store_size,
            (*buffer).backing_store_size,
        );
    }
    if !(*buffer).lock_snapshot.is_null() && (*buffer).lock_snapshot_size > 0 {
        let _ = Vec::from_raw_parts(
            (*buffer).lock_snapshot,
            (*buffer).lock_snapshot_size,
            (*buffer).lock_snapshot_size,
        );
    }
    let _ = Box::from_raw(buffer);
}

pub unsafe fn vtf_recycle_pixel_buffer(buffer: *mut vtf_cv_pixel_buffer) {
    if buffer.is_null() || (*buffer).recycled {
        return;
    }

    let payload = vt_ferry_protocol::RecycleBufferPayload {
        pool_id: (*buffer).pool_host_id,
        buffer_id: (*buffer).base.proxy_id,
        generation: (*buffer).host_generation,
    };

    match crate::transport::recycle_buffer(&payload) {
        Ok(()) => {
            (*buffer).recycled = true;
            crate::runtime::vtf_guest_trace(&format!(
                "corevideo:recycle_buffer ok pool_id={} buffer_id={} generation={}",
                payload.pool_id, payload.buffer_id, payload.generation
            ));
        }
        Err(status) => {
            crate::runtime::vtf_guest_trace(&format!(
                "corevideo:recycle_buffer failed pool_id={} buffer_id={} generation={} status={status}",
                payload.pool_id,
                payload.buffer_id,
                payload.generation
            ));
        }
    }
}


unsafe fn vtf_is_planar_pixel_format(pixel_format: u32) -> bool {
    matches!(
        pixel_format,
        0x34323076u32
            | 0x34323066u32
            | 0x79343230u32
            | 0x66343230u32
            | 0x78343230u32
            | 0x78663230u32
            | 0x34323276u32
            | 0x34343476u32
    )
}

pub(crate) unsafe fn vtf_ensure_backing_store(buffer: *mut vtf_cv_pixel_buffer) -> bool {
    if (*buffer).mapped_backing_active && !(*buffer).mapped_region_base.is_null() {
        // `mapped_region_base` already points at the beginning of the leased
        // slot. The transport/broker mapping helpers apply the slot offset when
        // constructing the mapping, so adding `slot_offset` again would walk
        // past the actual slot backing for every non-zero slot index.
        let base = (*buffer).mapped_region_base;
        (*buffer).backing_store = base;
        (*buffer).backing_store_size = (*buffer).total_size.max(1);
        for plane in 0..(*buffer).plane_count.min(4) {
            (*buffer).plane_data[plane] = base.add((*buffer).plane_offsets[plane]);
        }
        return true;
    }
    if !(*buffer).backing_store.is_null() {
        return true;
    }
    let size = (*buffer).total_size.max(1);
    let mut storage = vec![0u8; size];
    let base = storage.as_mut_ptr();
    (*buffer).backing_store = base;
    (*buffer).backing_store_size = size;
    (*buffer).backing_store_owned = true;
    for plane in 0..(*buffer).plane_count.min(4) {
        (*buffer).plane_data[plane] = base.add((*buffer).plane_offsets[plane]);
    }
    std::mem::forget(storage);
    true
}

unsafe fn vtf_attachment_dictionary(buffer: *mut vtf_cv_pixel_buffer) -> CFDictionaryRef {
    if (*buffer).attachments.is_null() {
        (*buffer).attachments = crate::corefoundation::CFDictionaryCreateMutable(
            std::ptr::null(),
            0,
            &crate::corefoundation::kCFTypeDictionaryKeyCallBacks,
            &crate::corefoundation::kCFTypeDictionaryValueCallBacks,
        );
    }
    (*buffer).attachments
}

#[repr(C)]
pub struct vtf_cv_pixel_buffer_pool {
    pub base: vtf_cf_object,
    pub width: usize,
    pub height: usize,
    pub pixel_format: u32,
    pub host_backing_kind: u32,
    pub slot_count: usize,
    pub buffer_region_size: usize,
    pub layout: vt_ferry_protocol::BufferLayoutReply,
    pub owner_session: *mut crate::videotoolbox::vtf_vt_session,
    pub pixel_buffer_attributes: CFDictionaryRef,
}

unsafe fn vtf_finalize_pixel_buffer_pool(obj: *mut vtf_cf_object) {
    let pool = obj as *mut vtf_cv_pixel_buffer_pool;
    // Tell the worker to release this pool's record + the
    // `IOSurfacePoolDirectory` entry it claimed. Without this
    // signal a connection that creates a pool, drops it, and
    // immediately creates another at the same shape would starve
    // the second pool — the worker has no other lifecycle hook
    // for session-less pools (those created by FFmpeg's
    // `-hwaccel videotoolbox` `AVHWFramesContext`, in particular).
    // Idempotent on the worker side — destroy on an unknown id
    // is a no-op — so we don't need to track double-release here.
    if (*pool).base.proxy_id != 0 {
        let _ = crate::transport::destroy_buffer_pool((*pool).base.proxy_id);
    }
    crate::runtime::CFRelease((*pool).pixel_buffer_attributes);
    let _ = Box::from_raw(pool);
}

fn vtf_requested_pool_buffer_count() -> u32 {
    let max_count = vt_ferry_protocol::VTF_TRANSPORT_MAX_POOL_SLOTS as u32;
    if let Ok(value) = std::env::var("VT_FERRY_GUEST_POOL_BUFFER_COUNT") {
        if let Ok(parsed) = value.parse::<u32>() {
            return parsed.clamp(1, max_count);
        }
    }
    VTF_GUEST_POOL_DEFAULT_BUFFER_COUNT
}

unsafe fn vtf_create_pixel_buffer_from_lease(
    pool: *mut vtf_cv_pixel_buffer_pool,
    lease: vt_ferry_protocol::PoolBufferLeaseReply,
    layout: vt_ferry_protocol::BufferLayoutReply,
    require_mapped_backing: bool,
) -> *mut vtf_cv_pixel_buffer {
    let mut plane_offsets = [0usize; 4];
    let mut plane_widths = [0usize; 4];
    let mut plane_heights = [0usize; 4];
    let mut plane_bytes_per_row = [0usize; 4];

    for i in 0..(layout.plane_count as usize).min(4) {
        plane_offsets[i] = layout.plane_offsets[i] as usize;
        plane_widths[i] = layout.plane_widths[i] as usize;
        plane_heights[i] = layout.plane_heights[i] as usize;
        plane_bytes_per_row[i] = layout.plane_bytes_per_row[i] as usize;
    }

    if require_mapped_backing {
        return std::ptr::null_mut();
    }

    let buffer = Box::into_raw(Box::new(vtf_cv_pixel_buffer {
        base: vtf_cf_object::with_host_id(
            crate::runtime::VTF_TYPE_PIXEL_BUFFER,
            Some(vtf_finalize_pixel_buffer),
            lease.buffer_id,
            lease.buffer_id,
            lease.generation,
        ),
        pool_ref: crate::runtime::CFRetain(pool as CFTypeRef)
            .cast_mut()
            .cast(),
        pool_host_id: (*pool).base.host_id,
        host_generation: lease.generation,
        host_backing_kind: lease.host_backing_kind,
        recycled: false,
        cache_valid: false,
        backing_store_owned: false,
        mapped_backing_active: false,
        state: 0,
        slot_index: lease.slot_index,
        slot_offset: lease.slot_offset,
        slot_region_size: layout.total_size,
        lock_flags: 0,
        lock_snapshot: std::ptr::null_mut(),
        lock_snapshot_size: 0,
        backing_store: std::ptr::null_mut(),
        backing_store_size: 0,
        mapped_region_base: std::ptr::null_mut(),
        mapped_region_size: 0,
        total_size: layout.total_size as usize,
        width: (*pool).width,
        height: (*pool).height,
        pixel_format: (*pool).pixel_format,
        plane_count: layout.plane_count as usize,
        plane_offsets,
        plane_widths,
        plane_heights,
        plane_bytes_per_row,
        plane_data: [std::ptr::null_mut(); 4],
        attachments: std::ptr::null_mut(),
        locked: false,
    }));

    buffer
}

pub unsafe fn vtf_create_pixel_buffer_pool_for_session(
    session: *mut crate::videotoolbox::vtf_vt_session,
    session_id: u64,
    width: u32,
    height: u32,
    pixel_format: u32,
    pixel_buffer_attributes: CFDictionaryRef,
) -> Result<*mut vtf_cv_pixel_buffer_pool, CVReturn> {
    if !crate::transport::is_enabled() {
        return Err(kCVReturnError);
    }
    crate::runtime::vtf_guest_trace(&format!(
        "corevideo:create_pool session_id={session_id} width={width} height={height} pixel_format=0x{pixel_format:08x}"
    ));

    let payload = vt_ferry_protocol::CreateBufferPoolPayload {
        session_id,
        buffer_count: vtf_requested_pool_buffer_count(),
        pixel_format,
        width,
        height,
        usage_flags: 0,
        _padding: 0,
    };

    let reply = crate::transport::create_buffer_pool(&payload).map_err(|status| {
        crate::runtime::vtf_guest_trace(&format!("corevideo:create_pool failed status={status}"));
        kCVReturnError
    })?;
    crate::runtime::vtf_guest_trace(&format!(
        "corevideo:create_pool ok pool_id={} slot_count={} region_size={} host_backing_kind={}",
        reply.pool_id, reply.slot_count, reply.buffer_region_size, reply.host_backing_kind
    ));
    let retained_attributes = crate::runtime::CFRetain(pixel_buffer_attributes);
    let pool = Box::into_raw(Box::new(vtf_cv_pixel_buffer_pool {
        base: vtf_cf_object::with_host_id(
            crate::runtime::VTF_TYPE_PIXEL_BUFFER_POOL,
            Some(vtf_finalize_pixel_buffer_pool),
            reply.pool_id,
            reply.pool_id,
            1,
        ),
        width: width as usize,
        height: height as usize,
        pixel_format,
        host_backing_kind: reply.host_backing_kind,
        slot_count: reply.slot_count as usize,
        buffer_region_size: reply.buffer_region_size as usize,
        layout: reply.layout,
        owner_session: session,
        pixel_buffer_attributes: retained_attributes,
    }));

    Ok(pool)
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferLockBaseAddress(
    pixelBuffer: CVPixelBufferRef,
    flags: CVOptionFlags,
) -> CVReturn {
    if pixelBuffer.is_null() {
        return kCVReturnInvalidArgument;
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    if !vtf_ensure_backing_store(buffer) {
        return kCVReturnError;
    }
    (*buffer).lock_flags = flags as u64;
    (*buffer).locked = true;
    kCVReturnSuccess
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferUnlockBaseAddress(
    pixelBuffer: CVPixelBufferRef,
    _flags: CVOptionFlags,
) -> CVReturn {
    if pixelBuffer.is_null() {
        return kCVReturnInvalidArgument;
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    (*buffer).locked = false;
    kCVReturnSuccess
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetBaseAddress(pixelBuffer: CVPixelBufferRef) -> *mut c_void {
    if pixelBuffer.is_null() {
        return std::ptr::null_mut();
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    if !vtf_ensure_backing_store(buffer) {
        return std::ptr::null_mut();
    }
    (*buffer).backing_store as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetBaseAddressOfPlane(
    pixelBuffer: CVPixelBufferRef,
    planeIndex: usize,
) -> *mut c_void {
    if pixelBuffer.is_null() {
        return std::ptr::null_mut();
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    if planeIndex >= (*buffer).plane_count.min(4) || !vtf_ensure_backing_store(buffer) {
        return std::ptr::null_mut();
    }
    (*buffer).plane_data[planeIndex] as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetBytesPerRow(pixelBuffer: CVPixelBufferRef) -> usize {
    if pixelBuffer.is_null() {
        return 0;
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    if (*buffer).plane_count == 0 {
        0
    } else {
        (*buffer).plane_bytes_per_row[0]
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetBytesPerRowOfPlane(
    pixelBuffer: CVPixelBufferRef,
    planeIndex: usize,
) -> usize {
    if pixelBuffer.is_null() {
        return 0;
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    if planeIndex >= (*buffer).plane_count.min(4) {
        0
    } else {
        (*buffer).plane_bytes_per_row[planeIndex]
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetWidth(pixelBuffer: CVPixelBufferRef) -> usize {
    if pixelBuffer.is_null() {
        0
    } else {
        (*(pixelBuffer as *mut vtf_cv_pixel_buffer)).width
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetHeight(pixelBuffer: CVPixelBufferRef) -> usize {
    if pixelBuffer.is_null() {
        0
    } else {
        (*(pixelBuffer as *mut vtf_cv_pixel_buffer)).height
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetPixelFormatType(pixelBuffer: CVPixelBufferRef) -> u32 {
    if pixelBuffer.is_null() {
        0
    } else {
        (*(pixelBuffer as *mut vtf_cv_pixel_buffer)).pixel_format
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferIsPlanar(pixelBuffer: CVPixelBufferRef) -> bool {
    if pixelBuffer.is_null() {
        return false;
    }
    let buffer = pixelBuffer as *mut vtf_cv_pixel_buffer;
    (*buffer).plane_count > 1 || vtf_is_planar_pixel_format((*buffer).pixel_format)
}

#[no_mangle]
pub unsafe extern "C" fn CVPixelBufferGetPlaneCount(pixelBuffer: CVPixelBufferRef) -> usize {
    if pixelBuffer.is_null() {
        0
    } else {
        (*(pixelBuffer as *mut vtf_cv_pixel_buffer)).plane_count
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVBufferSetAttachment(
    buffer: CVPixelBufferRef,
    key: CFTypeRef,
    value: CFTypeRef,
    _attachmentMode: CVAttachmentMode,
) {
    if buffer.is_null() || key.is_null() {
        return;
    }
    let buffer = buffer as *mut vtf_cv_pixel_buffer;
    let dict = vtf_attachment_dictionary(buffer);
    crate::corefoundation::CFDictionarySetValue(dict, key, value);
}

#[no_mangle]
pub unsafe extern "C" fn CVBufferRemoveAttachment(buffer: CVPixelBufferRef, key: CFTypeRef) {
    if buffer.is_null() || key.is_null() {
        return;
    }
    let buffer = buffer as *mut vtf_cv_pixel_buffer;
    if (*buffer).attachments.is_null() {
        return;
    }
    let dict = (*buffer).attachments as *mut crate::corefoundation::vtf_cf_dictionary;
    if let Some(index) = (*dict).keys.iter().position(|existing| *existing == key) {
        let key_value = (*dict).keys.remove(index);
        let value = (*dict).values.remove(index);
        crate::runtime::CFRelease(key_value);
        crate::runtime::CFRelease(value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVBufferGetAttachments(
    buffer: CVPixelBufferRef,
    _attachmentMode: CVAttachmentMode,
) -> CFDictionaryRef {
    if buffer.is_null() {
        return std::ptr::null();
    }
    let buffer = buffer as *mut vtf_cv_pixel_buffer;
    if (*buffer).attachments.is_null() {
        std::ptr::null()
    } else {
        crate::runtime::CFRetain((*buffer).attachments)
    }
}

#[no_mangle]
pub unsafe extern "C" fn CVBufferCopyAttachments(
    buffer: CVPixelBufferRef,
    _attachmentMode: CVAttachmentMode,
) -> CFDictionaryRef {
    if buffer.is_null() {
        return std::ptr::null();
    }
    let buffer = buffer as *mut vtf_cv_pixel_buffer;
    if (*buffer).attachments.is_null() {
        return std::ptr::null();
    }
    crate::corefoundation::CFDictionaryCreateCopy(std::ptr::null(), (*buffer).attachments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pixel format constants used by the smoke / FFmpeg path.
    const FMT_NV12: u32 = 0x34323076;
    const FMT_BGRA: u32 = 0x42475241;
    /// P010 video range — 10-bit 4:2:0 bi-planar, FourCC 'x420'.
    const FMT_P010: u32 = 0x78343230;
    /// P010 full range — 10-bit 4:2:0 bi-planar, FourCC 'xf20'.
    const FMT_P010_FULL: u32 = 0x78663230;

    /// Build a synthetic pixel buffer that bypasses the host transport.
    ///
    /// Tests need to exercise the CVPixelBuffer surface (width/height/lock/
    /// base address/plane queries) without standing up a host worker.
    /// `recycled = true` short-circuits `vtf_recycle_pixel_buffer` so
    /// `CFRelease` won't try to talk to the transport during cleanup;
    /// `backing_store_owned = false` means finalize won't double-free
    /// memory we never owned.
    unsafe fn synthetic_pixel_buffer(
        width: usize,
        height: usize,
        pixel_format: u32,
        plane_count: usize,
        plane_offsets: [usize; 4],
        plane_widths: [usize; 4],
        plane_heights: [usize; 4],
        plane_bytes_per_row: [usize; 4],
        total_size: usize,
    ) -> *mut vtf_cv_pixel_buffer {
        Box::into_raw(Box::new(vtf_cv_pixel_buffer {
            base: vtf_cf_object {
                magic: 0x534d5654,
                type_id: crate::runtime::VTF_TYPE_PIXEL_BUFFER,
                refcount: std::sync::atomic::AtomicI32::new(1),
                flags: 0,
                proxy_id: 1,
                generation: 1,
                host_id: 0,
                finalize: Some(vtf_finalize_pixel_buffer),
            },
            pool_ref: std::ptr::null_mut(),
            pool_host_id: 0,
            host_generation: 1,
            host_backing_kind: 0,
            recycled: true, // skip transport recycle on cleanup
            cache_valid: false,
            backing_store_owned: false,
            mapped_backing_active: false,
            state: VTF_CV_BUFFER_STATE_GUEST_WRITABLE,
            slot_index: 0,
            slot_offset: 0,
            slot_region_size: total_size as u32,
            lock_flags: 0,
            lock_snapshot: std::ptr::null_mut(),
            lock_snapshot_size: 0,
            backing_store: std::ptr::null_mut(),
            backing_store_size: 0,
            mapped_region_base: std::ptr::null_mut(),
            mapped_region_size: 0,
            total_size,
            width,
            height,
            pixel_format,
            plane_count,
            plane_offsets,
            plane_widths,
            plane_heights,
            plane_bytes_per_row,
            plane_data: [std::ptr::null_mut(); 4],
            attachments: std::ptr::null_mut(),
            locked: false,
        }))
    }

    fn nv12_buffer() -> *mut vtf_cv_pixel_buffer {
        let width = 128;
        let height = 72;
        let y_stride = width;
        let cbcr_stride = width;
        let y_size = y_stride * height;
        let cbcr_size = cbcr_stride * (height / 2);
        unsafe {
            synthetic_pixel_buffer(
                width,
                height,
                FMT_NV12,
                2,
                [0, y_size, 0, 0],
                [width, width / 2, 0, 0],
                [height, height / 2, 0, 0],
                [y_stride, cbcr_stride, 0, 0],
                y_size + cbcr_size,
            )
        }
    }

    fn p010_buffer() -> *mut vtf_cv_pixel_buffer {
        // P010 layout mirrors NV12 (Y plane + interleaved CbCr at
        // half resolution) but every sample is a 16-bit word — so
        // strides double. This synthetic buffer doesn't need to
        // exactly match the worker's 64-byte alignment; what matters
        // for the planar-query path is `pixel_format` and
        // `plane_count`.
        let width = 128;
        let height = 72;
        let y_stride = width * 2;
        let cbcr_stride = width * 2;
        let y_size = y_stride * height;
        let cbcr_size = cbcr_stride * (height / 2);
        unsafe {
            synthetic_pixel_buffer(
                width,
                height,
                FMT_P010,
                2,
                [0, y_size, 0, 0],
                [width, width / 2, 0, 0],
                [height, height / 2, 0, 0],
                [y_stride, cbcr_stride, 0, 0],
                y_size + cbcr_size,
            )
        }
    }

    fn bgra_buffer() -> *mut vtf_cv_pixel_buffer {
        let width = 64;
        let height = 36;
        let stride = width * 4;
        unsafe {
            synthetic_pixel_buffer(
                width,
                height,
                FMT_BGRA,
                1,
                [0, 0, 0, 0],
                [width, 0, 0, 0],
                [height, 0, 0, 0],
                [stride, 0, 0, 0],
                stride * height,
            )
        }
    }

    #[test]
    fn pixelbuffer_width_height_format_match_pool_spec() {
        unsafe {
            let buf = nv12_buffer() as CVPixelBufferRef;
            assert_eq!(CVPixelBufferGetWidth(buf), 128);
            assert_eq!(CVPixelBufferGetHeight(buf), 72);
            assert_eq!(CVPixelBufferGetPixelFormatType(buf), FMT_NV12);
            crate::runtime::CFRelease(buf);
        }
    }

    #[test]
    fn pixelbuffer_planar_query_distinguishes_nv12_and_bgra() {
        unsafe {
            let nv12 = nv12_buffer() as CVPixelBufferRef;
            assert!(CVPixelBufferIsPlanar(nv12), "NV12 must report planar");
            assert_eq!(CVPixelBufferGetPlaneCount(nv12), 2);
            crate::runtime::CFRelease(nv12);

            let bgra = bgra_buffer() as CVPixelBufferRef;
            assert!(!CVPixelBufferIsPlanar(bgra), "BGRA must report non-planar");
            // Plane count is the underlying field even for packed formats —
            // FFmpeg reads it via CVPixelBufferGetPlaneCount when computing
            // plane strides.
            assert_eq!(CVPixelBufferGetPlaneCount(bgra), 1);
            crate::runtime::CFRelease(bgra);
        }
    }

    #[test]
    fn pixelbuffer_get_base_address_returns_owned_backing() {
        unsafe {
            let buf = bgra_buffer();
            // Before lock, base address should still resolve via
            // vtf_ensure_backing_store and allocate guest-owned memory.
            // After we release we have to flip backing_store_owned so the
            // synthetic Vec owned by vtf_ensure_backing_store gets freed.
            let base = CVPixelBufferGetBaseAddress(buf as CVPixelBufferRef);
            assert!(!base.is_null(), "base address must be allocated lazily");
            // The first plane's data pointer matches the base.
            let plane0 = CVPixelBufferGetBaseAddressOfPlane(buf as CVPixelBufferRef, 0);
            assert_eq!(plane0, base);
            crate::runtime::CFRelease(buf as CVPixelBufferRef);
        }
    }

    #[test]
    fn pixelbuffer_planar_base_address_offsets_match_layout() {
        unsafe {
            let buf = nv12_buffer();
            let base = CVPixelBufferGetBaseAddress(buf as CVPixelBufferRef) as usize;
            let plane0 = CVPixelBufferGetBaseAddressOfPlane(buf as CVPixelBufferRef, 0) as usize;
            let plane1 = CVPixelBufferGetBaseAddressOfPlane(buf as CVPixelBufferRef, 1) as usize;
            assert_eq!(
                plane0, base,
                "Y-plane data must start at the buffer base"
            );
            // Second plane (CbCr) starts at the configured plane offset.
            let expected_cbcr_offset = (*buf).plane_offsets[1];
            assert_eq!(plane1 - base, expected_cbcr_offset);
            crate::runtime::CFRelease(buf as CVPixelBufferRef);
        }
    }

    #[test]
    fn pixelbuffer_bytes_per_row_matches_layout() {
        unsafe {
            let buf = nv12_buffer();
            assert_eq!(CVPixelBufferGetBytesPerRow(buf as CVPixelBufferRef), 128);
            assert_eq!(
                CVPixelBufferGetBytesPerRowOfPlane(buf as CVPixelBufferRef, 0),
                128
            );
            assert_eq!(
                CVPixelBufferGetBytesPerRowOfPlane(buf as CVPixelBufferRef, 1),
                128
            );
            // Out-of-range plane index returns 0 instead of overrunning.
            assert_eq!(
                CVPixelBufferGetBytesPerRowOfPlane(buf as CVPixelBufferRef, 7),
                0
            );
            crate::runtime::CFRelease(buf as CVPixelBufferRef);
        }
    }

    #[test]
    fn pixelbuffer_lock_unlock_round_trip() {
        unsafe {
            let buf = bgra_buffer();
            assert!(!(*buf).locked);
            let lock = CVPixelBufferLockBaseAddress(buf as CVPixelBufferRef, 0);
            assert_eq!(lock, kCVReturnSuccess);
            assert!((*buf).locked);
            let unlock = CVPixelBufferUnlockBaseAddress(buf as CVPixelBufferRef, 0);
            assert_eq!(unlock, kCVReturnSuccess);
            assert!(!(*buf).locked);
            crate::runtime::CFRelease(buf as CVPixelBufferRef);
        }
    }

    #[test]
    fn pixelbuffer_lock_remembers_flags() {
        unsafe {
            let buf = bgra_buffer();
            CVPixelBufferLockBaseAddress(buf as CVPixelBufferRef, kCVPixelBufferLock_ReadOnly);
            assert_eq!(
                (*buf).lock_flags,
                kCVPixelBufferLock_ReadOnly as u64,
                "ReadOnly flag must round-trip into the buffer"
            );
            CVPixelBufferUnlockBaseAddress(buf as CVPixelBufferRef, 0);
            crate::runtime::CFRelease(buf as CVPixelBufferRef);
        }
    }

    #[test]
    fn pixelbuffer_p010_reports_planar_with_two_planes() {
        // Regression coverage for the bug where
        // `vtf_is_planar_pixel_format` returned false for P010 —
        // FFmpeg would then ask for `plane_count = 1` (the field
        // value) but read `bytes_per_row` for "the buffer" rather
        // than per plane, getting wrong strides for the chroma
        // plane and corrupting the encoder input.
        unsafe {
            let buf = p010_buffer() as CVPixelBufferRef;
            assert!(
                CVPixelBufferIsPlanar(buf),
                "P010 must report planar via FourCC even when plane_count >= 2"
            );
            assert_eq!(CVPixelBufferGetPlaneCount(buf), 2);
            assert_eq!(CVPixelBufferGetPixelFormatType(buf), FMT_P010);
            // Y plane stride must be width * 2 (16-bit samples).
            assert_eq!(CVPixelBufferGetBytesPerRowOfPlane(buf, 0), 256);
            assert_eq!(CVPixelBufferGetBytesPerRowOfPlane(buf, 1), 256);
            crate::runtime::CFRelease(buf);
        }
    }

    #[test]
    fn pixelbuffer_p010_full_range_is_recognized_as_planar() {
        // The 'xf20' (full-range P010) FourCC is a separate value
        // from 'x420' (video-range). Both must take the planar
        // path, since they share the same bi-planar 10-bit layout.
        unsafe {
            let buf = p010_buffer();
            (*buf).pixel_format = FMT_P010_FULL;
            let buf_ref = buf as CVPixelBufferRef;
            assert!(
                CVPixelBufferIsPlanar(buf_ref),
                "P010 full-range must report planar"
            );
            assert_eq!(CVPixelBufferGetPixelFormatType(buf_ref), FMT_P010_FULL);
            crate::runtime::CFRelease(buf_ref);
        }
    }

    #[test]
    fn null_inputs_are_safe() {
        unsafe {
            assert_eq!(CVPixelBufferGetWidth(std::ptr::null()), 0);
            assert_eq!(CVPixelBufferGetHeight(std::ptr::null()), 0);
            assert_eq!(CVPixelBufferGetPixelFormatType(std::ptr::null()), 0);
            assert_eq!(CVPixelBufferGetBytesPerRow(std::ptr::null()), 0);
            assert_eq!(CVPixelBufferGetPlaneCount(std::ptr::null()), 0);
            assert!(CVPixelBufferGetBaseAddress(std::ptr::null()).is_null());
            assert!(CVPixelBufferGetBaseAddressOfPlane(std::ptr::null(), 0).is_null());
            assert_eq!(
                CVPixelBufferLockBaseAddress(std::ptr::null(), 0),
                kCVReturnInvalidArgument
            );
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(std::ptr::null(), 0),
                kCVReturnInvalidArgument
            );
        }
    }

    /// `read_pool_attribute_int` is the workhorse behind
    /// `CVPixelBufferPoolCreate`'s width/height/pixel-format
    /// dispatch. The previous bug was that it didn't exist —
    /// the entry point hardcoded 640x360 NV12 — so this test
    /// pins the contract: a CFNumber stored under one of our
    /// `kCVPixelBuffer*Key` symbols is read back with the
    /// requested numeric type, both for SInt32-typed numbers
    /// (FFmpeg's primary path) and SInt64-typed numbers (which
    /// the helper transparently widens).
    #[test]
    fn read_pool_attribute_int_round_trips_width_height_pf() {
        unsafe {
            use crate::corefoundation::{
                CFDictionaryCreateMutable, CFDictionarySetValue, CFNumberCreate,
                K_CF_NUMBER_SINT32_TYPE, K_CF_NUMBER_SINT64_TYPE,
            };
            let dict = CFDictionaryCreateMutable(
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            );
            assert!(!dict.is_null());

            // FFmpeg writes width/height as SInt32 (matches Apple's
            // av_buffersrc_parameters → CVPixelBufferPool path).
            let w: i32 = 1920;
            let w_num = CFNumberCreate(
                std::ptr::null(),
                K_CF_NUMBER_SINT32_TYPE,
                &w as *const _ as *const c_void,
            );
            CFDictionarySetValue(dict, kCVPixelBufferWidthKey.0, w_num as *const c_void);

            let h: i32 = 1080;
            let h_num = CFNumberCreate(
                std::ptr::null(),
                K_CF_NUMBER_SINT32_TYPE,
                &h as *const _ as *const c_void,
            );
            CFDictionarySetValue(dict, kCVPixelBufferHeightKey.0, h_num as *const c_void);

            // PixelFormatType is a fourcc (NV12 = 0x34323076 = '420v')
            // which fits in SInt32, but we write it as SInt64 here
            // to exercise the fall-through path in the helper.
            let pf: i64 = 0x3432_3076;
            let pf_num = CFNumberCreate(
                std::ptr::null(),
                K_CF_NUMBER_SINT64_TYPE,
                &pf as *const _ as *const c_void,
            );
            CFDictionarySetValue(
                dict,
                kCVPixelBufferPixelFormatTypeKey.0,
                pf_num as *const c_void,
            );

            assert_eq!(
                read_pool_attribute_int(dict as CFDictionaryRef, kCVPixelBufferWidthKey.0),
                Some(1920)
            );
            assert_eq!(
                read_pool_attribute_int(dict as CFDictionaryRef, kCVPixelBufferHeightKey.0),
                Some(1080)
            );
            assert_eq!(
                read_pool_attribute_int(dict as CFDictionaryRef, kCVPixelBufferPixelFormatTypeKey.0),
                Some(0x3432_3076)
            );

            crate::runtime::CFRelease(dict);
            crate::runtime::CFRelease(w_num as crate::runtime::CFTypeRef);
            crate::runtime::CFRelease(h_num as crate::runtime::CFTypeRef);
            crate::runtime::CFRelease(pf_num as crate::runtime::CFTypeRef);
        }
    }

    #[test]
    fn read_pool_attribute_int_returns_none_on_null_inputs() {
        unsafe {
            assert_eq!(
                read_pool_attribute_int(std::ptr::null(), kCVPixelBufferWidthKey.0),
                None
            );
            // Empty dictionary → key not present → None (no panic,
            // no spurious "0" default).
            let dict = crate::corefoundation::CFDictionaryCreateMutable(
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            );
            assert!(!dict.is_null());
            assert_eq!(
                read_pool_attribute_int(dict as CFDictionaryRef, kCVPixelBufferWidthKey.0),
                None
            );
            crate::runtime::CFRelease(dict);
        }
    }
}
