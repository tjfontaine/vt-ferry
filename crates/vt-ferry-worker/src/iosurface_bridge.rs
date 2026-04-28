//! Worker-side seam for receiving IOSurface-backed pool slots from the
//! launcher via kernel Mach-port inheritance.
//!
//! The smolvm launcher allocates IOSurfaces, registers their Mach send rights
//! with `mach_ports_register(mach_task_self())`, then spawns the worker. This
//! module performs the matching `mach_ports_lookup` on worker startup and
//! exposes helpers that wrap a received IOSurface as a `CVPixelBuffer` via
//! `CVPixelBufferCreateWithIOSurface` — bypassing any host-side byte copy when
//! the pool is IOSurface-backed.

use libc::{c_int, c_uint, c_void};

#[allow(non_camel_case_types)]
// FFI surface intentionally exposes Apple symbols this module
// might call (today's path is "match what the worker proves it
// needs"; tomorrow's bring-up may want IOSurfaceGetWidth /
// IOSurfaceGetHeight / IOSurfaceGetAllocSize for sanity probes).
// Suppressing dead_code here keeps the bindings discoverable
// without spamming the build with unused-symbol warnings.
#[allow(dead_code)]
mod ffi {
    use super::*;

    pub type mach_port_t = c_uint;
    pub type kern_return_t = c_int;
    pub type mach_msg_type_number_t = c_uint;
    pub type mach_port_array_t = *mut mach_port_t;

    pub const KERN_SUCCESS: kern_return_t = 0;

    #[repr(C)]
    pub struct __IOSurface(c_void);
    pub type IOSurfaceRef = *mut __IOSurface;
    pub type CFTypeRef = *const c_void;
    pub type CFAllocatorRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CVPixelBufferRef = *mut c_void;
    pub type CVReturn = i32;

    unsafe extern "C" {
        pub static mach_task_self_: mach_port_t;

        pub fn mach_ports_lookup(
            target_task: mach_port_t,
            init_port_set: *mut mach_port_array_t,
            init_port_set_cnt: *mut mach_msg_type_number_t,
        ) -> kern_return_t;

        pub fn mach_port_deallocate(target: mach_port_t, name: mach_port_t) -> kern_return_t;

        pub fn vm_deallocate(
            target_task: mach_port_t,
            address: usize,
            size: usize,
        ) -> kern_return_t;
    }

    #[link(name = "IOSurface", kind = "framework")]
    unsafe extern "C" {
        pub fn IOSurfaceLookupFromMachPort(port: mach_port_t) -> IOSurfaceRef;
        pub fn IOSurfaceGetWidth(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceGetHeight(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceGetID(surface: IOSurfaceRef) -> u32;
        pub fn IOSurfaceGetAllocSize(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceGetBaseAddress(surface: IOSurfaceRef) -> *mut c_void;
        pub fn IOSurfaceGetBytesPerRow(surface: IOSurfaceRef) -> usize;
        pub fn IOSurfaceLock(surface: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;
        pub fn IOSurfaceUnlock(surface: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;
    }

    pub type CVPixelBufferReleaseBytesCallback =
        Option<unsafe extern "C" fn(*mut c_void, *const c_void)>;
    pub type CVPixelBufferReleasePlanarBytesCallback = Option<
        unsafe extern "C" fn(*mut c_void, *const c_void, usize, usize, *const *const c_void),
    >;

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        pub fn CVPixelBufferCreateWithIOSurface(
            allocator: CFAllocatorRef,
            surface: IOSurfaceRef,
            attributes: CFDictionaryRef,
            pixel_buffer_out: *mut CVPixelBufferRef,
        ) -> CVReturn;

        pub fn CVPixelBufferCreateWithBytes(
            allocator: CFAllocatorRef,
            width: usize,
            height: usize,
            pixel_format_type: u32,
            base_address: *mut c_void,
            bytes_per_row: usize,
            release_callback: CVPixelBufferReleaseBytesCallback,
            release_ref_con: *mut c_void,
            pixel_buffer_attributes: CFDictionaryRef,
            pixel_buffer_out: *mut CVPixelBufferRef,
        ) -> CVReturn;

        pub fn CVPixelBufferCreateWithPlanarBytes(
            allocator: CFAllocatorRef,
            width: usize,
            height: usize,
            pixel_format_type: u32,
            data_ptr: *mut c_void,
            data_size: usize,
            number_of_planes: usize,
            plane_base_addresses: *mut *mut c_void,
            plane_widths: *mut usize,
            plane_heights: *mut usize,
            plane_bytes_per_row: *mut usize,
            release_callback: CVPixelBufferReleasePlanarBytesCallback,
            release_ref_con: *mut c_void,
            pixel_buffer_attributes: CFDictionaryRef,
            pixel_buffer_out: *mut CVPixelBufferRef,
        ) -> CVReturn;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFRelease(cf: CFTypeRef);
        pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    }
}

/// Owned IOSurface reference. Releases the +1 ref count on drop.
// Several accessors (width/height/alloc_size/bytes_per_row/unlock)
// are exposed for diagnostic and future-needs use but the current
// hot path doesn't call them. Suppressing dead_code keeps the API
// discoverable without burying real warnings.
#[allow(dead_code)]
pub struct OwnedIOSurface {
    inner: ffi::IOSurfaceRef,
}

#[allow(dead_code)]
impl OwnedIOSurface {
    /// SAFETY: `port` must be a valid send right for an IOSurface Mach port in
    /// the current task. On success, the returned `OwnedIOSurface` owns a +1
    /// ref count and releases it on drop.
    pub unsafe fn from_mach_port(port: u32) -> Option<Self> {
        let surface = unsafe { ffi::IOSurfaceLookupFromMachPort(port) };
        if surface.is_null() {
            None
        } else {
            Some(OwnedIOSurface { inner: surface })
        }
    }

    pub fn width(&self) -> usize {
        unsafe { ffi::IOSurfaceGetWidth(self.inner) }
    }

    pub fn height(&self) -> usize {
        unsafe { ffi::IOSurfaceGetHeight(self.inner) }
    }

    pub fn id(&self) -> u32 {
        unsafe { ffi::IOSurfaceGetID(self.inner) }
    }

    pub fn alloc_size(&self) -> usize {
        unsafe { ffi::IOSurfaceGetAllocSize(self.inner) }
    }

    pub fn bytes_per_row(&self) -> usize {
        unsafe { ffi::IOSurfaceGetBytesPerRow(self.inner) }
    }

    /// Returns the base VA of the IOSurface's backing memory. Valid as long
    /// as this `OwnedIOSurface` is retained. Caller is responsible for
    /// pairing reads/writes with `IOSurfaceLock` / `IOSurfaceUnlock`.
    pub fn base_address(&self) -> *mut u8 {
        unsafe { ffi::IOSurfaceGetBaseAddress(self.inner) as *mut u8 }
    }

    pub fn lock_read_only(&self) -> Result<u32, i32> {
        let mut seed: u32 = 0;
        let kr = unsafe {
            ffi::IOSurfaceLock(self.inner, 0x1 /* readOnly */, &mut seed)
        };
        if kr == 0 { Ok(seed) } else { Err(kr) }
    }

    pub fn unlock_read_only(&self) -> i32 {
        let mut seed: u32 = 0;
        unsafe { ffi::IOSurfaceUnlock(self.inner, 0x1, &mut seed) }
    }

    /// Lock the IOSurface for CPU read+write. Held for the pool's
    /// lifetime so slot pointers remain resident for VT's reads — VT
    /// doesn't implicitly lock CreateWithBytes/WithPlanarBytes wrappers
    /// because they aren't IOSurface-backed from its perspective.
    pub fn lock_read_write(&self) -> Result<u32, i32> {
        let mut seed: u32 = 0;
        let kr = unsafe { ffi::IOSurfaceLock(self.inner, 0, &mut seed) };
        if kr == 0 { Ok(seed) } else { Err(kr) }
    }

    /// Wrap this IOSurface as a CVPixelBuffer via `CVPixelBufferCreateWithIOSurface`.
    /// Caller owns the returned CVPixelBufferRef and must CFRelease it when done.
    pub fn wrap_as_cv_pixel_buffer(&self) -> Result<ffi::CVPixelBufferRef, i32> {
        let mut pixel_buffer: ffi::CVPixelBufferRef = std::ptr::null_mut();
        let status = unsafe {
            ffi::CVPixelBufferCreateWithIOSurface(
                std::ptr::null(),
                self.inner,
                std::ptr::null(),
                &mut pixel_buffer,
            )
        };
        if status != 0 || pixel_buffer.is_null() {
            Err(status)
        } else {
            Ok(pixel_buffer)
        }
    }

    pub fn as_raw(&self) -> ffi::IOSurfaceRef {
        self.inner
    }

    /// Retain this IOSurface and return an opaque handle the caller can
    /// pass as a release refcon. Paired with `release_retained_iosurface`.
    pub fn retain_as_refcon(&self) -> *mut libc::c_void {
        unsafe { ffi::CFRetain(self.inner as ffi::CFTypeRef) as *mut libc::c_void }
    }

    /// Bump the IOSurface's CFRetain and wrap as a fresh `OwnedIOSurface`.
    /// Lets the pool directory hand out the same IOSurface to multiple
    /// CREATE_BUFFER_POOL dispatches without giving up its own reference —
    /// FFmpeg's h264_videotoolbox encoder init issues the call twice.
    pub fn clone_ref(&self) -> Self {
        unsafe {
            ffi::CFRetain(self.inner as ffi::CFTypeRef);
        }
        OwnedIOSurface { inner: self.inner }
    }

    /// Wrap a contiguous slot sub-region (at `slot_offset_bytes`, one plane)
    /// as a CVPixelBuffer via `CVPixelBufferCreateWithBytes`. The returned
    /// CVPixelBuffer's backing pages ARE this IOSurface's pages — the guest
    /// writes land directly in the bytes VT reads — but the CVPixelBuffer
    /// is not IOSurface-backed from `CVPixelBufferGetIOSurface`'s view. VT
    /// still accepts it for encode (validated empirically during bring-up).
    ///
    /// The CVPixelBuffer holds a +1 retain on this IOSurface via its
    /// release callback so the backing pages outlive the slot wrapper.
    pub fn wrap_slot_single_plane(
        &self,
        slot_offset_bytes: usize,
        slot_width: usize,
        slot_height: usize,
        bytes_per_row: usize,
        pixel_format: u32,
    ) -> Result<ffi::CVPixelBufferRef, i32> {
        unsafe {
            let base = ffi::IOSurfaceGetBaseAddress(self.inner) as *mut u8;
            let slot_base = base.add(slot_offset_bytes) as *mut libc::c_void;
            let retained = self.retain_as_refcon();
            let mut pixel_buffer: ffi::CVPixelBufferRef = std::ptr::null_mut();
            let status = ffi::CVPixelBufferCreateWithBytes(
                std::ptr::null(),
                slot_width,
                slot_height,
                pixel_format,
                slot_base,
                bytes_per_row,
                Some(release_iosurface_retain),
                retained,
                std::ptr::null(),
                &mut pixel_buffer,
            );
            if status != 0 || pixel_buffer.is_null() {
                ffi::CFRelease(retained as ffi::CFTypeRef);
                Err(status)
            } else {
                Ok(pixel_buffer)
            }
        }
    }

    /// Wrap a slot as a biplanar CVPixelBuffer (NV12) via
    /// `CVPixelBufferCreateWithPlanarBytes`. The caller passes explicit
    /// plane pointers relative to this IOSurface's base; the IOSurface
    /// itself is treated as a raw byte region, not as a plane-structured
    /// IOSurface (CVPixelBufferCreateWithPlanarBytes doesn't introspect the
    /// backing surface, it trusts the pointers we give it).
    ///
    /// `plane_offsets_bytes`, `plane_widths`, `plane_heights`,
    /// `plane_bytes_per_row` each have `number_of_planes` entries (expected
    /// 2 for NV12). The CVPixelBuffer holds a +1 retain on this IOSurface.
    pub fn wrap_slot_planar(
        &self,
        width: usize,
        height: usize,
        pixel_format: u32,
        slot_base_offset_bytes: usize,
        slot_total_bytes: usize,
        plane_offsets_bytes: &[usize],
        plane_widths: &[usize],
        plane_heights: &[usize],
        plane_bytes_per_row: &[usize],
    ) -> Result<ffi::CVPixelBufferRef, i32> {
        let n = plane_offsets_bytes.len();
        if plane_widths.len() != n || plane_heights.len() != n || plane_bytes_per_row.len() != n {
            return Err(-1);
        }
        unsafe {
            let base = ffi::IOSurfaceGetBaseAddress(self.inner) as *mut u8;
            let slot_base = base.add(slot_base_offset_bytes);
            let mut plane_bases: Vec<*mut libc::c_void> = plane_offsets_bytes
                .iter()
                .map(|off| slot_base.add(*off) as *mut libc::c_void)
                .collect();
            let mut plane_widths_mut: Vec<usize> = plane_widths.to_vec();
            let mut plane_heights_mut: Vec<usize> = plane_heights.to_vec();
            let mut plane_bpr_mut: Vec<usize> = plane_bytes_per_row.to_vec();

            let retained = self.retain_as_refcon();
            let mut pixel_buffer: ffi::CVPixelBufferRef = std::ptr::null_mut();
            let status = ffi::CVPixelBufferCreateWithPlanarBytes(
                std::ptr::null(),
                width,
                height,
                pixel_format,
                slot_base as *mut libc::c_void,
                slot_total_bytes,
                n,
                plane_bases.as_mut_ptr(),
                plane_widths_mut.as_mut_ptr(),
                plane_heights_mut.as_mut_ptr(),
                plane_bpr_mut.as_mut_ptr(),
                Some(release_iosurface_retain_planar),
                retained,
                std::ptr::null(),
                &mut pixel_buffer,
            );
            if status != 0 || pixel_buffer.is_null() {
                ffi::CFRelease(retained as ffi::CFTypeRef);
                Err(status)
            } else {
                Ok(pixel_buffer)
            }
        }
    }
}

/// Wrap raw shared memory as a single-plane CVPixelBuffer. The caller owns the
/// returned CVPixelBufferRef and must keep `base_address` valid for the
/// lifetime of that buffer.
pub unsafe fn wrap_bytes_single_plane(
    base_address: *mut u8,
    slot_width: usize,
    slot_height: usize,
    bytes_per_row: usize,
    pixel_format: u32,
) -> Result<ffi::CVPixelBufferRef, i32> {
    let mut pixel_buffer: ffi::CVPixelBufferRef = std::ptr::null_mut();
    let status = unsafe {
        ffi::CVPixelBufferCreateWithBytes(
            std::ptr::null(),
            slot_width,
            slot_height,
            pixel_format,
            base_address.cast(),
            bytes_per_row,
            None,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut pixel_buffer,
        )
    };
    if status != 0 || pixel_buffer.is_null() {
        Err(status)
    } else {
        Ok(pixel_buffer)
    }
}

/// Wrap raw shared memory as a planar CVPixelBuffer. The caller owns the
/// returned CVPixelBufferRef and must keep the backing bytes valid for the
/// lifetime of that buffer.
pub unsafe fn wrap_bytes_planar(
    base_address: *mut u8,
    width: usize,
    height: usize,
    pixel_format: u32,
    slot_total_bytes: usize,
    plane_offsets_bytes: &[usize],
    plane_widths: &[usize],
    plane_heights: &[usize],
    plane_bytes_per_row: &[usize],
) -> Result<ffi::CVPixelBufferRef, i32> {
    let n = plane_offsets_bytes.len();
    if plane_widths.len() != n || plane_heights.len() != n || plane_bytes_per_row.len() != n {
        return Err(-1);
    }
    let mut plane_bases: Vec<*mut libc::c_void> = plane_offsets_bytes
        .iter()
        .map(|off| unsafe { base_address.add(*off).cast::<libc::c_void>() })
        .collect();
    let mut plane_widths_mut: Vec<usize> = plane_widths.to_vec();
    let mut plane_heights_mut: Vec<usize> = plane_heights.to_vec();
    let mut plane_bpr_mut: Vec<usize> = plane_bytes_per_row.to_vec();
    let mut pixel_buffer: ffi::CVPixelBufferRef = std::ptr::null_mut();
    let status = unsafe {
        ffi::CVPixelBufferCreateWithPlanarBytes(
            std::ptr::null(),
            width,
            height,
            pixel_format,
            base_address.cast(),
            slot_total_bytes,
            n,
            plane_bases.as_mut_ptr(),
            plane_widths_mut.as_mut_ptr(),
            plane_heights_mut.as_mut_ptr(),
            plane_bpr_mut.as_mut_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut pixel_buffer,
        )
    };
    if status != 0 || pixel_buffer.is_null() {
        Err(status)
    } else {
        Ok(pixel_buffer)
    }
}

unsafe extern "C" fn release_iosurface_retain(
    release_ref_con: *mut libc::c_void,
    _base_address: *const libc::c_void,
) {
    if !release_ref_con.is_null() {
        unsafe {
            ffi::CFRelease(release_ref_con as ffi::CFTypeRef);
        }
    }
}

unsafe extern "C" fn release_iosurface_retain_planar(
    release_ref_con: *mut libc::c_void,
    _data_ptr: *const libc::c_void,
    _data_size: usize,
    _number_of_planes: usize,
    _plane_base_addresses: *const *const libc::c_void,
) {
    if !release_ref_con.is_null() {
        unsafe {
            ffi::CFRelease(release_ref_con as ffi::CFTypeRef);
        }
    }
}

impl Drop for OwnedIOSurface {
    fn drop(&mut self) {
        if !self.inner.is_null() {
            unsafe {
                ffi::CFRelease(self.inner as ffi::CFTypeRef);
            }
        }
    }
}

// SAFETY: IOSurfaceRef is a CoreFoundation type that is internally thread-safe.
unsafe impl Send for OwnedIOSurface {}
unsafe impl Sync for OwnedIOSurface {}

/// Test-only helper so cross-module tests can reach the private
/// `ffi::mach_task_self_` without re-declaring the FFI.
#[cfg(test)]
pub(crate) fn __mach_task_self_for_tests() -> u32 {
    unsafe { ffi::mach_task_self_ }
}

/// Look up Mach ports registered by the parent task via `mach_ports_register`.
/// Each returned port is a send right in the current task; caller should wrap
/// them in `OwnedIOSurface` via `from_mach_port` to retain their IOSurface
/// and release the port reference cleanly.
pub fn lookup_registered_ports() -> Vec<u32> {
    let mut ports: ffi::mach_port_array_t = std::ptr::null_mut();
    let mut count: ffi::mach_msg_type_number_t = 0;
    let kr = unsafe { ffi::mach_ports_lookup(ffi::mach_task_self_, &mut ports, &mut count) };
    if kr != ffi::KERN_SUCCESS || ports.is_null() {
        return Vec::new();
    }

    let result: Vec<u32> = unsafe { std::slice::from_raw_parts(ports, count as usize) }
        .iter()
        .copied()
        .collect();

    // Release the kernel-allocated array; the port rights inside are now
    // owned by this task and will be kept alive by their retention in the
    // returned Vec (each will be consumed by `OwnedIOSurface::from_mach_port`
    // which doesn't transfer the right — IOSurfaceLookupFromMachPort holds
    // the surface via its own internal reference).
    unsafe {
        let _ = ffi::vm_deallocate(
            ffi::mach_task_self_,
            ports as usize,
            (count as usize) * std::mem::size_of::<ffi::mach_port_t>(),
        );
    }

    result
}

/// Registry of IOSurface references looked up at worker startup. The vt-real
/// backend consults this when it needs to wrap a launcher-provided IOSurface
/// into a CVPixelBuffer rather than allocating its own.
///
/// Several accessors (`len` / `is_empty` / `get` / `find_by_id`) are kept
/// for diagnostic introspection but the current dispatch path doesn't use
/// them. Suppressing dead_code keeps the API discoverable.
#[allow(dead_code)]
pub struct IOSurfaceRegistry {
    surfaces: Vec<OwnedIOSurface>,
}

#[allow(dead_code)]
impl IOSurfaceRegistry {
    pub fn load_from_registered_ports() -> Self {
        let ports = lookup_registered_ports();
        let mut surfaces = Vec::with_capacity(ports.len());
        for port in ports {
            if port == 0 {
                continue;
            }
            if let Some(surface) = unsafe { OwnedIOSurface::from_mach_port(port) } {
                surfaces.push(surface);
            }
        }
        IOSurfaceRegistry { surfaces }
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&OwnedIOSurface> {
        self.surfaces.get(index)
    }

    pub fn find_by_id(&self, id: u32) -> Option<&OwnedIOSurface> {
        self.surfaces.iter().find(|s| s.id() == id)
    }

    /// Move the IOSurface with the given ID out of the registry, if present.
    /// Ownership transfers to the caller.
    pub fn take_by_id(&mut self, id: u32) -> Option<OwnedIOSurface> {
        let position = self.surfaces.iter().position(|s| s.id() == id)?;
        Some(self.surfaces.remove(position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // See note on the production `mod ffi` allows. Same Apple
    // naming-convention exemption applies, plus dead_code for FFI
    // types/constants the test scaffolding declares but doesn't
    // call directly (they're exposed for completeness so the test
    // FFI matches the C surface).
    #[allow(non_camel_case_types, non_upper_case_globals, dead_code)]
    mod test_ffi {
        use libc::{c_int, c_uint, c_void};

        pub type mach_port_t = c_uint;
        pub type kern_return_t = c_int;
        pub type mach_msg_type_number_t = c_uint;
        pub type mach_port_array_t = *mut mach_port_t;

        #[repr(C)]
        pub struct __IOSurface(c_void);
        pub type IOSurfaceRef = *mut __IOSurface;
        pub type CFTypeRef = *const c_void;
        pub type CFDictionaryRef = *const c_void;
        pub type CFAllocatorRef = *const c_void;
        pub type CFStringRef = *const c_void;
        pub type CFNumberRef = *const c_void;
        pub type CFNumberType = c_int;
        pub type CFIndex = isize;
        pub type Boolean = u8;
        pub type CFDictionaryKeyCallBacks = c_void;
        pub type CFDictionaryValueCallBacks = c_void;
        pub type CFMutableDictionaryRef = *mut c_void;

        pub const kCFNumberIntType: CFNumberType = 9;

        unsafe extern "C" {
            pub static mach_task_self_: mach_port_t;
            pub fn mach_ports_register(
                target_task: mach_port_t,
                init_port_set: mach_port_array_t,
                init_port_set_cnt: mach_msg_type_number_t,
            ) -> kern_return_t;
        }

        #[link(name = "IOSurface", kind = "framework")]
        unsafe extern "C" {
            pub fn IOSurfaceCreate(properties: CFDictionaryRef) -> IOSurfaceRef;
            pub fn IOSurfaceCreateMachPort(surface: IOSurfaceRef) -> mach_port_t;

            pub static kIOSurfaceWidth: CFStringRef;
            pub static kIOSurfaceHeight: CFStringRef;
            pub static kIOSurfaceBytesPerElement: CFStringRef;
            pub static kIOSurfaceBytesPerRow: CFStringRef;
            pub static kIOSurfacePixelFormat: CFStringRef;
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            pub fn CFRelease(cf: CFTypeRef);
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

            pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
            pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
        }
    }

    // Build a minimal IOSurface from Rust for the round-trip test.
    fn make_test_iosurface() -> test_ffi::IOSurfaceRef {
        unsafe {
            let props = test_ffi::CFDictionaryCreateMutable(
                std::ptr::null(),
                0,
                &test_ffi::kCFTypeDictionaryKeyCallBacks,
                &test_ffi::kCFTypeDictionaryValueCallBacks,
            );
            let width: c_int = 256;
            let height: c_int = 144;
            let bpr: c_int = 1024;
            let bpe: c_int = 4;
            let fmt: c_int = 0x42475241; // 'BGRA'

            let width_num = test_ffi::CFNumberCreate(
                std::ptr::null(),
                test_ffi::kCFNumberIntType,
                &width as *const _ as *const _,
            );
            let height_num = test_ffi::CFNumberCreate(
                std::ptr::null(),
                test_ffi::kCFNumberIntType,
                &height as *const _ as *const _,
            );
            let bpr_num = test_ffi::CFNumberCreate(
                std::ptr::null(),
                test_ffi::kCFNumberIntType,
                &bpr as *const _ as *const _,
            );
            let bpe_num = test_ffi::CFNumberCreate(
                std::ptr::null(),
                test_ffi::kCFNumberIntType,
                &bpe as *const _ as *const _,
            );
            let fmt_num = test_ffi::CFNumberCreate(
                std::ptr::null(),
                test_ffi::kCFNumberIntType,
                &fmt as *const _ as *const _,
            );

            test_ffi::CFDictionarySetValue(
                props,
                test_ffi::kIOSurfaceWidth as *const _,
                width_num as *const _,
            );
            test_ffi::CFDictionarySetValue(
                props,
                test_ffi::kIOSurfaceHeight as *const _,
                height_num as *const _,
            );
            test_ffi::CFDictionarySetValue(
                props,
                test_ffi::kIOSurfaceBytesPerElement as *const _,
                bpe_num as *const _,
            );
            test_ffi::CFDictionarySetValue(
                props,
                test_ffi::kIOSurfaceBytesPerRow as *const _,
                bpr_num as *const _,
            );
            test_ffi::CFDictionarySetValue(
                props,
                test_ffi::kIOSurfacePixelFormat as *const _,
                fmt_num as *const _,
            );

            let surface = test_ffi::IOSurfaceCreate(props);

            test_ffi::CFRelease(width_num as test_ffi::CFTypeRef);
            test_ffi::CFRelease(height_num as test_ffi::CFTypeRef);
            test_ffi::CFRelease(bpr_num as test_ffi::CFTypeRef);
            test_ffi::CFRelease(bpe_num as test_ffi::CFTypeRef);
            test_ffi::CFRelease(fmt_num as test_ffi::CFTypeRef);
            test_ffi::CFRelease(props as test_ffi::CFTypeRef);

            surface
        }
    }

    #[test]
    fn test_mach_ports_register_roundtrip_wraps_iosurface() {
        unsafe {
            let surface = make_test_iosurface();
            assert!(!surface.is_null(), "IOSurfaceCreate returned nil");

            let original_id = {
                let as_ref = OwnedIOSurface {
                    inner: surface as *mut _,
                };
                let id = as_ref.id();
                std::mem::forget(as_ref); // IOSurfaceLookupFromMachPort will
                // give us another +1 we'll own.
                id
            };

            // Get a Mach port for the IOSurface. This is a fresh send right
            // in the current task's namespace.
            let port = test_ffi::IOSurfaceCreateMachPort(surface);
            assert!(port != 0, "IOSurfaceCreateMachPort returned 0");

            // Register the port in the task's "registered ports" slot. On real
            // parent/child scenarios the child inherits this; here we just
            // validate the API round-trip within the same task.
            let mut ports_arr: [test_ffi::mach_port_t; 1] = [port];
            let kr = test_ffi::mach_ports_register(
                test_ffi::mach_task_self_,
                ports_arr.as_mut_ptr(),
                ports_arr.len() as test_ffi::mach_msg_type_number_t,
            );
            assert_eq!(kr, 0, "mach_ports_register kr={}", kr);

            let registry = IOSurfaceRegistry::load_from_registered_ports();
            assert!(
                !registry.is_empty(),
                "IOSurfaceRegistry empty after load_from_registered_ports"
            );

            let recovered = registry
                .find_by_id(original_id)
                .expect("registry did not contain IOSurface with original id");
            assert_eq!(recovered.width(), 256);
            assert_eq!(recovered.height(), 144);

            let cv_ref = recovered
                .wrap_as_cv_pixel_buffer()
                .expect("wrap_as_cv_pixel_buffer failed");
            assert!(!cv_ref.is_null());
            ffi::CFRelease(cv_ref as ffi::CFTypeRef);

            // Clear the registered ports slot so we don't leak state between
            // tests.
            let mut empty: [test_ffi::mach_port_t; 0] = [];
            let _ = test_ffi::mach_ports_register(test_ffi::mach_task_self_, empty.as_mut_ptr(), 0);

            // Release the Mach port and the original IOSurface.
            let _ = ffi::mach_port_deallocate(ffi::mach_task_self_, port);
            test_ffi::CFRelease(surface as test_ffi::CFTypeRef);
        }
    }
}
