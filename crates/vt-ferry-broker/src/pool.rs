//! IOSurface allocation for vt-ferry zero-copy pools.
//!
//! Allocates one packed IOSurface per pool (slots addressed as byte offsets).
//! Stays under XNU's TASK_PORT_REGISTER_MAX = 3 cap on `mach_ports_register`,
//! so the limit is the number of distinct pool *shapes* per VM, not slots.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[allow(non_camel_case_types)]
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

    pub const K_CFNUMBER_INT_TYPE: CFNumberType = 9;
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

        pub fn mach_port_deallocate(target: mach_port_t, name: mach_port_t) -> kern_return_t;
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

        pub static kIOSurfaceWidth: CFStringRef;
        pub static kIOSurfaceHeight: CFStringRef;
        pub static kIOSurfaceBytesPerElement: CFStringRef;
        pub static kIOSurfaceBytesPerRow: CFStringRef;
        pub static kIOSurfacePixelFormat: CFStringRef;
        pub static kIOSurfaceAllocSize: CFStringRef;
    }
}

/// VT pool spec parsed from JSON / CLI. The same shape that vt-ferry-host-worker
/// already understands.
#[derive(Debug, Clone, Deserialize)]
pub struct PoolSpec {
    pub guest_phys_addr: u64,
    pub slot_count: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    #[serde(default = "default_writable")]
    pub writable: bool,
}

fn default_writable() -> bool {
    true
}

/// Worker-facing JSON entry for VT_FERRY_IOSURFACE_POOL_SPECS_JSON.
#[derive(Debug, Serialize, Clone)]
pub struct WorkerIOSurfacePoolSpec {
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub slot_count: u32,
    pub iosurface_id: u32,
}

/// Owned IOSurface + its registered Mach send right. Drop releases both.
///
/// Several fields (`iosurface_id`, `guest_phys_addr`, `len`, `writable`,
/// `name`) are recorded for diagnostic introspection and to keep the
/// shape symmetric with the launcher's pool-spec JSON, even though
/// the broker's hot path doesn't read them. Suppressing dead_code
/// keeps the introspection surface discoverable.
#[allow(dead_code)]
pub struct OwnedIOSurfacePoolEntry {
    surface: iosurface_ffi::IOSurfaceRef,
    pub mach_port: u32,
    pub iosurface_id: u32,
    pub guest_phys_addr: u64,
    pub len: u64,
    pub writable: bool,
    pub name: String,
}

impl Drop for OwnedIOSurfacePoolEntry {
    fn drop(&mut self) {
        unsafe {
            if self.mach_port != 0 {
                let _ = iosurface_ffi::mach_port_deallocate(
                    iosurface_ffi::mach_task_self_,
                    self.mach_port,
                );
            }
            if !self.surface.is_null() {
                iosurface_ffi::CFRelease(self.surface as iosurface_ffi::CFTypeRef);
            }
        }
    }
}

unsafe impl Send for OwnedIOSurfacePoolEntry {}
unsafe impl Sync for OwnedIOSurfacePoolEntry {}

const BGRA_FOURCC: u32 = 0x42475241;
const NV12_FOURCC: u32 = 0x34323076;

/// XNU's TASK_PORT_REGISTER_MAX. mach_ports_register rejects > 3.
pub const MAX_REGISTERED_IOSURFACE_PORTS: usize = 3;

fn page_round_up(value: u64) -> u64 {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    if page_size == 0 {
        return value;
    }
    (value + page_size - 1) & !(page_size - 1)
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    (value + alignment - 1) & !(alignment - 1)
}

fn pool_slot_alloc_size(width: u32, height: u32, pixel_format: u32) -> Result<u64> {
    let raw = match pixel_format {
        BGRA_FOURCC => (width as u64) * 4 * (height as u64),
        NV12_FOURCC => {
            let stride = align_up(width as u64, 64);
            let y = stride * (height as u64);
            let cbcr = stride * ((height as u64) / 2);
            y + cbcr
        }
        other => {
            return Err(anyhow!(
                "pool pixel_format 0x{:x} is not supported (only BGRA=0x42475241 and NV12=0x34323076)",
                other
            ));
        }
    };
    Ok(page_round_up(raw))
}

fn pool_region_name(width: u32, height: u32, pixel_format: u32, slot_count: u32) -> Result<String> {
    let slot_alloc = pool_slot_alloc_size(width, height, pixel_format)?;
    Ok(format!(
        "vt-ferry-pool-pf{}-w{}-h{}-n{}-b{}",
        pixel_format, width, height, slot_count, slot_alloc
    ))
}

fn cf_number_i32(value: i32) -> iosurface_ffi::CFNumberRef {
    unsafe {
        iosurface_ffi::CFNumberCreate(
            std::ptr::null(),
            iosurface_ffi::K_CFNUMBER_INT_TYPE,
            &value as *const _ as *const _,
        )
    }
}

fn cf_number_i64(value: i64) -> iosurface_ffi::CFNumberRef {
    unsafe {
        iosurface_ffi::CFNumberCreate(
            std::ptr::null(),
            iosurface_ffi::K_CFNUMBER_INT_TYPE,
            &value as *const _ as *const _,
        )
    }
}

fn create_packed_bgra_iosurface(
    width: u32,
    height: u32,
    alloc_size: u64,
) -> Result<(iosurface_ffi::IOSurfaceRef, u32)> {
    let w = width as i32;
    let h = height as i32;
    let bpr = (width as i32) * 4;
    let bpe = 4i32;
    let fmt = BGRA_FOURCC as i32;

    unsafe {
        let props = iosurface_ffi::CFDictionaryCreateMutable(
            std::ptr::null(),
            0,
            &iosurface_ffi::kCFTypeDictionaryKeyCallBacks,
            &iosurface_ffi::kCFTypeDictionaryValueCallBacks,
        );
        let width_n = cf_number_i32(w);
        let height_n = cf_number_i32(h);
        let bpr_n = cf_number_i32(bpr);
        let bpe_n = cf_number_i32(bpe);
        let fmt_n = cf_number_i32(fmt);
        let alloc_n = cf_number_i64(alloc_size as i64);

        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfaceWidth as *const _,
            width_n as *const _,
        );
        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfaceHeight as *const _,
            height_n as *const _,
        );
        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfaceBytesPerElement as *const _,
            bpe_n as *const _,
        );
        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfaceBytesPerRow as *const _,
            bpr_n as *const _,
        );
        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfacePixelFormat as *const _,
            fmt_n as *const _,
        );
        iosurface_ffi::CFDictionarySetValue(
            props,
            iosurface_ffi::kIOSurfaceAllocSize as *const _,
            alloc_n as *const _,
        );

        let surface = iosurface_ffi::IOSurfaceCreate(props);
        iosurface_ffi::CFRelease(width_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(height_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(bpr_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(bpe_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(fmt_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(alloc_n as iosurface_ffi::CFTypeRef);
        iosurface_ffi::CFRelease(props as iosurface_ffi::CFTypeRef);

        if surface.is_null() {
            return Err(anyhow!(
                "IOSurfaceCreate returned nil for packed shape {}x{} alloc_size={}",
                width,
                height,
                alloc_size
            ));
        }
        let surface_id = iosurface_ffi::IOSurfaceGetID(surface);
        Ok((surface, surface_id))
    }
}

/// Allocate one packed IOSurface per pool. Returns owned entries (which carry
/// the Mach port + IOSurface CF ref) and the worker-side specs.
pub fn allocate_pools(
    specs: &[PoolSpec],
) -> Result<(Vec<OwnedIOSurfacePoolEntry>, Vec<WorkerIOSurfacePoolSpec>)> {
    if specs.len() > MAX_REGISTERED_IOSURFACE_PORTS {
        return Err(anyhow!(
            "{} pools requested but TASK_PORT_REGISTER_MAX={} caps mach_ports_register",
            specs.len(),
            MAX_REGISTERED_IOSURFACE_PORTS
        ));
    }

    let mut entries = Vec::with_capacity(specs.len());
    let mut worker_specs = Vec::with_capacity(specs.len());

    for spec in specs {
        if spec.slot_count == 0 {
            return Err(anyhow!("pool requires slot_count >= 1"));
        }
        let slot_aligned = pool_slot_alloc_size(spec.width, spec.height, spec.pixel_format)?;
        let total = slot_aligned
            .checked_mul(spec.slot_count as u64)
            .ok_or_else(|| anyhow!("packed pool size overflow"))?;
        let raw_bpr = (spec.width as u64) * 4;
        if raw_bpr == 0 {
            return Err(anyhow!("pool width must be > 0"));
        }
        let raw_height = ((total + raw_bpr - 1) / raw_bpr) as u32;
        let raw_alloc = (raw_bpr * raw_height as u64).max(total);
        let (surface, surface_id) =
            create_packed_bgra_iosurface(spec.width, raw_height, raw_alloc)?;

        let port = unsafe { iosurface_ffi::IOSurfaceCreateMachPort(surface) };
        if port == 0 {
            unsafe { iosurface_ffi::CFRelease(surface as iosurface_ffi::CFTypeRef) };
            return Err(anyhow!("IOSurfaceCreateMachPort returned 0"));
        }

        let name = pool_region_name(spec.width, spec.height, spec.pixel_format, spec.slot_count)?;
        entries.push(OwnedIOSurfacePoolEntry {
            surface,
            mach_port: port,
            iosurface_id: surface_id,
            guest_phys_addr: spec.guest_phys_addr,
            len: total,
            writable: spec.writable,
            name,
        });
        worker_specs.push(WorkerIOSurfacePoolSpec {
            width: spec.width,
            height: spec.height,
            pixel_format: spec.pixel_format,
            slot_count: spec.slot_count,
            iosurface_id: surface_id,
        });
    }

    Ok((entries, worker_specs))
}

/// Plant the pool entries' Mach send rights in this task's registered-ports
/// slot. Children spawned after this call inherit them via mach_ports_lookup.
pub fn register_ports(entries: &[OwnedIOSurfacePoolEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    if entries.len() > MAX_REGISTERED_IOSURFACE_PORTS {
        return Err(anyhow!(
            "TASK_PORT_REGISTER_MAX={} but {} ports requested",
            MAX_REGISTERED_IOSURFACE_PORTS,
            entries.len()
        ));
    }
    let mut ports: Vec<u32> = entries.iter().map(|e| e.mach_port).collect();
    let kr = unsafe {
        iosurface_ffi::mach_ports_register(
            iosurface_ffi::mach_task_self_,
            ports.as_mut_ptr(),
            ports.len() as u32,
        )
    };
    if kr != iosurface_ffi::KERN_SUCCESS {
        return Err(anyhow!("mach_ports_register failed kr={} (0x{:x})", kr, kr));
    }
    Ok(())
}
