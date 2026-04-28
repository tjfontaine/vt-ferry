//! Standalone probe that answers one question: can Apple's Hypervisor.framework
//! hv_vm_map a host VA that aliases IOSurface pages?
//!
//! Runs three experiments in sequence, reporting each individually:
//!   1. Baseline: hv_vm_map a plain anonymous mmap'd page (expected: succeed).
//!   2. IOSurface direct: hv_vm_map IOSurfaceGetBaseAddress() directly (may
//!      fail — IOKit-backed VAs may be rejected).
//!   3. IOSurface alias: mach_vm_remap the IOSurface pages to a new VA, then
//!      hv_vm_map that alias (this is what smolvm's launcher does today).
//!
//! For each variant we print the exact HV_ return code so we can compare
//! against hv/Error codes and choose the right architectural pivot.
//!
//! Requires the `com.apple.security.hypervisor` entitlement — re-sign the
//! resulting binary before running.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

type HvReturn = u32;
type HvIpa = u64;
type HvMemoryFlags = u64;
const HV_SUCCESS: HvReturn = 0;
const HV_MEMORY_READ: HvMemoryFlags = 1 << 0;
const HV_MEMORY_WRITE: HvMemoryFlags = 1 << 1;
const HV_MEMORY_EXEC: HvMemoryFlags = 1 << 2;

#[link(name = "Hypervisor", kind = "framework")]
unsafe extern "C" {
    fn hv_vm_create(config: *mut c_void) -> HvReturn;
    fn hv_vm_destroy() -> HvReturn;
    fn hv_vm_map(addr: *mut c_void, ipa: HvIpa, size: usize, flags: HvMemoryFlags) -> HvReturn;
    fn hv_vm_unmap(ipa: HvIpa, size: usize) -> HvReturn;
}

type IOSurfaceRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type CFNumberRef = *const c_void;
type CFNumberType = i32;
const K_CF_NUMBER_INT_TYPE: CFNumberType = 9;
type KernReturn = i32;
type MachPort = u32;
type MachVmAddr = u64;
type MachVmSize = u64;
type VmProt = i32;
type VmInherit = i32;

const KERN_SUCCESS: KernReturn = 0;
const VM_FLAGS_ANYWHERE: i32 = 0x0001;
const VM_INHERIT_NONE: VmInherit = 2;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const libc::c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFNumberCreate(
        alloc: CFAllocatorRef,
        the_type: CFNumberType,
        value_ptr: *const c_void,
    ) -> CFNumberRef;
    fn CFDictionaryCreateMutable(
        alloc: CFAllocatorRef,
        capacity: isize,
        key_cb: *const c_void,
        val_cb: *const c_void,
    ) -> CFMutableDictionaryRef;
    fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: CFTypeRef, val: CFTypeRef);
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceCreate(properties: CFDictionaryRef) -> IOSurfaceRef;
    fn IOSurfaceGetBaseAddress(surface: IOSurfaceRef) -> *mut c_void;
    fn IOSurfaceGetAllocSize(surface: IOSurfaceRef) -> usize;
    fn IOSurfaceLock(surface: IOSurfaceRef, opts: u32, seed: *mut u32) -> KernReturn;
    fn IOSurfaceUnlock(surface: IOSurfaceRef, opts: u32, seed: *mut u32) -> KernReturn;
}

#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    static mach_task_self_: MachPort;
    fn mach_vm_remap(
        target_task: MachPort,
        target_addr: *mut MachVmAddr,
        size: MachVmSize,
        mask: MachVmAddr,
        flags: i32,
        src_task: MachPort,
        src_addr: MachVmAddr,
        copy: i32,
        cur_prot: *mut VmProt,
        max_prot: *mut VmProt,
        inheritance: VmInherit,
    ) -> KernReturn;
}

const K_CF_STRING_ENCODING_ASCII: u32 = 0x0600;

fn cf_string(s: &str) -> CFStringRef {
    let c = std::ffi::CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_ASCII) }
}

fn cf_number_i32(v: i32) -> CFNumberRef {
    unsafe {
        CFNumberCreate(
            ptr::null(),
            K_CF_NUMBER_INT_TYPE,
            &v as *const _ as *const c_void,
        )
    }
}

fn cf_number_i64(v: i64) -> CFNumberRef {
    unsafe {
        CFNumberCreate(
            ptr::null(),
            K_CF_NUMBER_INT_TYPE,
            &v as *const _ as *const c_void,
        )
    }
}

fn make_bgra_iosurface(width: i32, height: i32) -> IOSurfaceRef {
    let bytes_per_row = (width as i64) * 4;
    let alloc_size = bytes_per_row * (height as i64);

    let keys = [
        cf_string("IOSurfaceWidth"),
        cf_string("IOSurfaceHeight"),
        cf_string("IOSurfaceBytesPerElement"),
        cf_string("IOSurfaceBytesPerRow"),
        cf_string("IOSurfaceAllocSize"),
        cf_string("IOSurfacePixelFormat"),
    ];
    let values = [
        cf_number_i32(width) as CFTypeRef,
        cf_number_i32(height) as CFTypeRef,
        cf_number_i32(4) as CFTypeRef,
        cf_number_i64(bytes_per_row) as CFTypeRef,
        cf_number_i64(alloc_size) as CFTypeRef,
        cf_number_i32(0x42475241) as CFTypeRef, // 'BGRA'
    ];

    unsafe {
        let dict = CFDictionaryCreateMutable(
            ptr::null(),
            6,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        for i in 0..6 {
            CFDictionarySetValue(dict, keys[i] as CFTypeRef, values[i]);
        }
        let surface = IOSurfaceCreate(dict as CFDictionaryRef);
        CFRelease(dict as CFTypeRef);
        for k in keys {
            CFRelease(k as CFTypeRef);
        }
        for v in values {
            CFRelease(v);
        }
        surface
    }
}

fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

fn round_up_to_page(v: usize, ps: usize) -> usize {
    (v + ps - 1) & !(ps - 1)
}

fn hv_flags_rwx() -> HvMemoryFlags {
    HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC
}

fn try_hv_vm_map(label: &str, host_va: *mut c_void, guest_ipa: HvIpa, size: usize) {
    let ret = unsafe { hv_vm_map(host_va, guest_ipa, size, hv_flags_rwx()) };
    println!(
        "  [{}] hv_vm_map(host=0x{:016x}, guest=0x{:x}, size=0x{:x}) -> 0x{:08x} ({})",
        label,
        host_va as usize,
        guest_ipa,
        size,
        ret,
        if ret == HV_SUCCESS { "OK" } else { "FAIL" }
    );
    if ret == HV_SUCCESS {
        let r = unsafe { hv_vm_unmap(guest_ipa, size) };
        println!("  [{}] hv_vm_unmap -> 0x{:08x}", label, r);
    }
}

fn main() -> Result<(), String> {
    let ps = page_size();
    println!("system page size = {}", ps);

    // Create HVF VM
    let cr = unsafe { hv_vm_create(ptr::null_mut()) };
    if cr != HV_SUCCESS {
        return Err(format!(
            "hv_vm_create failed: 0x{:x} (is the hypervisor entitlement present?)",
            cr
        ));
    }
    println!("hv_vm_create: OK");

    // Reserve a guest IPA region well out of the way. 0x400000000 = 16 GiB.
    let mut next_ipa: HvIpa = 0x4_0000_0000;

    // === Experiment 1: anonymous mmap page ===
    println!();
    println!("exp-1: anonymous mmap baseline");
    let size = 2 * ps;
    let anon = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANON | libc::MAP_PRIVATE,
            -1,
            0,
        )
    };
    if anon == libc::MAP_FAILED {
        return Err("mmap anon failed".into());
    }
    try_hv_vm_map("anon", anon, next_ipa, size);
    next_ipa += size as u64;

    // === Experiment 2: IOSurface direct base address ===
    println!();
    println!("exp-2: IOSurface direct base address");
    let surface = make_bgra_iosurface(128, 72);
    if surface.is_null() {
        return Err("IOSurfaceCreate returned null".into());
    }
    let alloc = unsafe { IOSurfaceGetAllocSize(surface) };
    let iosurf_size = round_up_to_page(alloc, ps);
    println!(
        "  IOSurface allocSize={} page-rounded size=0x{:x}",
        alloc, iosurf_size
    );

    let mut seed = 0u32;
    let lk = unsafe { IOSurfaceLock(surface, 0, &mut seed) };
    println!("  IOSurfaceLock -> {}", lk);

    let iosurf_base = unsafe { IOSurfaceGetBaseAddress(surface) };
    println!(
        "  IOSurfaceGetBaseAddress = 0x{:016x}",
        iosurf_base as usize
    );
    try_hv_vm_map("iosurf-direct", iosurf_base, next_ipa, iosurf_size);
    next_ipa += iosurf_size as u64;

    // === Experiment 3: mach_vm_remap alias of IOSurface ===
    println!();
    println!("exp-3: mach_vm_remap alias, then hv_vm_map alias");
    let mut alias_addr: MachVmAddr = 0;
    let mut cur_prot: VmProt = 0;
    let mut max_prot: VmProt = 0;
    let kr = unsafe {
        mach_vm_remap(
            mach_task_self_,
            &mut alias_addr,
            iosurf_size as u64,
            0,
            VM_FLAGS_ANYWHERE,
            mach_task_self_,
            iosurf_base as MachVmAddr,
            0, // copy = FALSE => aliasing
            &mut cur_prot,
            &mut max_prot,
            VM_INHERIT_NONE,
        )
    };
    println!(
        "  mach_vm_remap -> {} alias=0x{:x} curProt={} maxProt={}",
        kr, alias_addr, cur_prot, max_prot
    );
    if kr == KERN_SUCCESS && alias_addr != 0 {
        try_hv_vm_map(
            "iosurf-alias",
            alias_addr as *mut c_void,
            next_ipa,
            iosurf_size,
        );
    }

    // === Experiment 4: mlock'd alias ===
    println!();
    println!("exp-4: mlock the alias, then hv_vm_map");
    if kr == KERN_SUCCESS && alias_addr != 0 {
        let mr = unsafe { libc::mlock(alias_addr as *const c_void, iosurf_size) };
        println!(
            "  mlock(alias) -> {} ({})",
            mr,
            if mr == 0 { "OK" } else { "FAIL" }
        );
        try_hv_vm_map(
            "iosurf-alias-mlock",
            alias_addr as *mut c_void,
            next_ipa + iosurf_size as u64,
            iosurf_size,
        );
    }

    // === Experiment 5: mlock the IOSurface direct base ===
    println!();
    println!("exp-5: mlock IOSurface direct base, then hv_vm_map");
    let mr = unsafe { libc::mlock(iosurf_base, iosurf_size) };
    println!("  mlock(iosurf_base) -> {}", mr);
    try_hv_vm_map(
        "iosurf-direct-mlock",
        iosurf_base,
        next_ipa + 2 * iosurf_size as u64,
        iosurf_size,
    );

    // Clean up
    unsafe {
        IOSurfaceUnlock(surface, 0, &mut seed);
        CFRelease(surface as CFTypeRef);
        libc::munmap(anon, size);
        let _ = hv_vm_destroy();
    }

    let _ = MaybeUninit::<u8>::uninit();
    Ok(())
}
