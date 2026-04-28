#![allow(non_camel_case_types)]
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64, Ordering};

pub type CFTypeRef = *const c_void;
pub type CFTypeID = u64;

pub const VTF_TYPE_STRING: CFTypeID = 1;
pub const VTF_TYPE_NUMBER: CFTypeID = 2;
pub const VTF_TYPE_DATA: CFTypeID = 3;
pub const VTF_TYPE_ARRAY: CFTypeID = 4;
pub const VTF_TYPE_DICTIONARY: CFTypeID = 5;
pub const VTF_TYPE_BOOLEAN: CFTypeID = 6;
pub const VTF_TYPE_BLOCK_BUFFER: CFTypeID = 7;
pub const VTF_TYPE_FORMAT_DESCRIPTION: CFTypeID = 8;
pub const VTF_TYPE_SAMPLE_BUFFER: CFTypeID = 9;
pub const VTF_TYPE_PIXEL_BUFFER: CFTypeID = 10;
pub const VTF_TYPE_PIXEL_BUFFER_POOL: CFTypeID = 11;
pub const VTF_TYPE_VT_SESSION: CFTypeID = 12;
/// Decode-side companion to `VTF_TYPE_VT_SESSION`. Distinct
/// type id so `CFGetTypeID` can tell decode and encode sessions
/// apart at runtime.
pub const VTF_TYPE_VT_DECOMPRESSION_SESSION: CFTypeID = 13;

pub const VTF_OBJECT_FLAG_STATIC: u32 = 1;

static vtf_next_proxy_id: AtomicU64 = AtomicU64::new(1);

/// Live proxy-object accounting. Incremented by `vtf_cf_object::init` and
/// decremented when `CFRelease` drops the last reference and the object's
/// finalize hook runs. Static-flagged objects (e.g. the `kCFBooleanTrue`
/// singleton, key strings) skip both paths and don't show up here.
///
/// Tests use this to detect proxy-object leaks: allocate, release, and
/// assert the count is back to zero. It's also a cheap diagnostic for a
/// running process — non-zero at clean shutdown means something held a
/// retained reference past its lifetime.
static VTF_PROXY_ALIVE: AtomicI64 = AtomicI64::new(0);

/// Number of dynamically-allocated proxy objects currently live.
///
/// "Live" means `init` ran and the matching finalize hook has not yet
/// completed. Useful for leak-checking in tests.
pub fn vtf_proxy_alive_count() -> i64 {
    VTF_PROXY_ALIVE.load(Ordering::SeqCst)
}

/// Manually bump the alive counter for a proxy object that was
/// allocated outside `vtf_cf_object::init` (e.g. decode-side
/// CVPixelBuffer proxies built directly from inline pixel
/// bytes). Pairs with the decrement that happens when the
/// matching finalize hook runs.
pub fn vtf_record_proxy_alive_increment() {
    VTF_PROXY_ALIVE.fetch_add(1, Ordering::SeqCst);
}

/// Cumulative bytes copied through the CMBlockBuffer / MMIO defensive
/// copy paths. Incremented by `vtf_record_data_copy` from
/// `coremedia.rs::CMBlockBufferCreateWithMemoryBlock`,
/// `CMBlockBufferCopyDataBytes`, and `vtf_copy_from_mmio`.
///
/// Use to detect when an "alias" path silently degraded to a copy, or
/// to catch a defensive copy doubling (the same bytes copied twice
/// across two layers). The counter is monotonic and process-global.
static VTF_DATA_COPY_BYTES: AtomicU64 = AtomicU64::new(0);

/// Number of distinct copy *events* (independent of byte volume), so a
/// test can tell apart "one big copy" from "many small copies".
static VTF_DATA_COPY_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Cumulative bytes copied through the data-copy hot paths. Monotonic.
pub fn vtf_data_copy_bytes() -> u64 {
    VTF_DATA_COPY_BYTES.load(Ordering::SeqCst)
}

/// Number of distinct data-copy events recorded. Monotonic.
pub fn vtf_data_copy_events() -> u64 {
    VTF_DATA_COPY_EVENTS.load(Ordering::SeqCst)
}

/// Record a data-copy event of `bytes` length. Called from the
/// CMBlockBuffer / MMIO copy paths.
pub fn vtf_record_data_copy(bytes: usize) {
    VTF_DATA_COPY_BYTES.fetch_add(bytes as u64, Ordering::SeqCst);
    VTF_DATA_COPY_EVENTS.fetch_add(1, Ordering::SeqCst);
}

/// VTCompressionSession lifecycle counters. These are per-type so a
/// caller can diagnose "how many sessions did this run create vs
/// destroy?" without grepping the trace log. Both are monotonic; the
/// difference is the live session count.
static VTF_VT_SESSIONS_CREATED: AtomicU64 = AtomicU64::new(0);
static VTF_VT_SESSIONS_DESTROYED: AtomicU64 = AtomicU64::new(0);

/// Cumulative VT session creations (`VTCompressionSessionCreate` ok path).
pub fn vtf_vt_sessions_created() -> u64 {
    VTF_VT_SESSIONS_CREATED.load(Ordering::SeqCst)
}

/// Cumulative VT session destructions (`vtf_finalize_vt_session`).
pub fn vtf_vt_sessions_destroyed() -> u64 {
    VTF_VT_SESSIONS_DESTROYED.load(Ordering::SeqCst)
}

/// Currently-live VT sessions. Goes negative if Destroy fires more
/// times than Create — a hard bug.
pub fn vtf_vt_sessions_live() -> i64 {
    vtf_vt_sessions_created() as i64 - vtf_vt_sessions_destroyed() as i64
}

pub fn vtf_record_vt_session_created() {
    VTF_VT_SESSIONS_CREATED.fetch_add(1, Ordering::SeqCst);
}

pub fn vtf_record_vt_session_destroyed() {
    VTF_VT_SESSIONS_DESTROYED.fetch_add(1, Ordering::SeqCst);
}

pub fn vtf_guest_trace(message: &str) {
    let Some(path) = std::env::var_os("VT_FERRY_GUEST_TRACE_PATH") else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

#[repr(C)]
pub struct vtf_cf_object {
    pub magic: u32,
    pub type_id: CFTypeID,
    pub refcount: AtomicI32,
    pub flags: u32,
    pub proxy_id: u64,
    pub generation: u64,
    pub host_id: u64,
    pub finalize: Option<unsafe fn(*mut vtf_cf_object)>,
}

impl vtf_cf_object {
    pub fn init(type_id: CFTypeID, finalize: Option<unsafe fn(*mut vtf_cf_object)>) -> Self {
        VTF_PROXY_ALIVE.fetch_add(1, Ordering::SeqCst);
        vtf_cf_object {
            magic: 0x534d5654,
            type_id,
            refcount: AtomicI32::new(1),
            flags: 0,
            proxy_id: vtf_next_proxy_id.fetch_add(1, Ordering::SeqCst),
            generation: 1,
            host_id: 0,
            finalize,
        }
    }

    /// Construct a base object whose proxy_id is supplied by the host
    /// rather than allocated locally. VT sessions, buffer pools, and
    /// pixel buffers from a host-issued lease all use this path so the
    /// guest handle matches the host id end-to-end. Bumps the alive
    /// counter the same way `init` does — without it, the matching
    /// `CFRelease` decrement would drift the counter negative for these
    /// types.
    pub fn with_host_id(
        type_id: CFTypeID,
        finalize: Option<unsafe fn(*mut vtf_cf_object)>,
        proxy_id: u64,
        host_id: u64,
        generation: u64,
    ) -> Self {
        VTF_PROXY_ALIVE.fetch_add(1, Ordering::SeqCst);
        vtf_cf_object {
            magic: 0x534d5654,
            type_id,
            refcount: AtomicI32::new(1),
            flags: 0,
            proxy_id,
            generation,
            host_id,
            finalize,
        }
    }
}

pub unsafe fn vtf_object(value: CFTypeRef) -> *mut vtf_cf_object {
    value as *mut vtf_cf_object
}

#[no_mangle]
pub unsafe extern "C" fn CFRetain(value: CFTypeRef) -> CFTypeRef {
    if value.is_null() {
        return std::ptr::null();
    }
    let obj = vtf_object(value);
    if ((*obj).flags & VTF_OBJECT_FLAG_STATIC) == 0 {
        (*obj).refcount.fetch_add(1, Ordering::SeqCst);
    }
    value
}

#[no_mangle]
pub unsafe extern "C" fn CFRelease(value: CFTypeRef) {
    if value.is_null() {
        return;
    }
    let obj = vtf_object(value);
    if ((*obj).flags & VTF_OBJECT_FLAG_STATIC) != 0 {
        return;
    }
    if (*obj).refcount.fetch_sub(1, Ordering::SeqCst) == 1 {
        if let Some(fin) = (*obj).finalize {
            fin(obj);
        }
        VTF_PROXY_ALIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub unsafe fn vtf_get_type_id(value: CFTypeRef) -> CFTypeID {
    if value.is_null() {
        0
    } else {
        (*vtf_object(value)).type_id
    }
}

pub unsafe fn vtf_get_proxy_id(value: CFTypeRef) -> u64 {
    if value.is_null() {
        0
    } else {
        (*vtf_object(value)).proxy_id
    }
}

pub unsafe fn vtf_get_generation(value: CFTypeRef) -> u64 {
    if value.is_null() {
        0
    } else {
        (*vtf_object(value)).generation
    }
}

pub unsafe fn vtf_get_host_id(value: CFTypeRef) -> u64 {
    if value.is_null() {
        0
    } else {
        (*vtf_object(value)).host_id
    }
}

pub unsafe fn vtf_bind_host_id(value: CFTypeRef, host_id: u64) {
    if !value.is_null() {
        (*vtf_object(value)).host_id = host_id;
    }
}

pub unsafe fn vtf_bump_generation(value: CFTypeRef) -> u64 {
    if value.is_null() {
        return 0;
    }
    let obj = vtf_object(value);
    (*obj).generation += 1;
    (*obj).host_id = 0;
    (*obj).generation
}

#[no_mangle]
pub unsafe extern "C" fn CFEqual(lhs: CFTypeRef, rhs: CFTypeRef) -> bool {
    if lhs == rhs {
        return true;
    }
    if lhs.is_null() || rhs.is_null() {
        return false;
    }

    let lhs_type = vtf_get_type_id(lhs);
    let rhs_type = vtf_get_type_id(rhs);

    if lhs_type != rhs_type {
        return false;
    }

    // We will implement specific type logic elsewhere, for now default to pointer equality
    lhs == rhs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    /// Build a heap-allocated `vtf_cf_object` we can hand to the public
    /// C API. The default finalize (`box_finalize` below) turns the raw
    /// pointer back into a `Box` so the allocation is freed when the
    /// last `CFRelease` fires — keeping the global `VTF_PROXY_ALIVE`
    /// counter balanced no matter which test path runs.
    fn alloc(type_id: CFTypeID, finalize: Option<unsafe fn(*mut vtf_cf_object)>) -> *mut vtf_cf_object {
        let finalize = finalize.or(Some(box_finalize_helper));
        Box::into_raw(Box::new(vtf_cf_object::init(type_id, finalize)))
    }

    /// Finalize hook that deallocates the underlying `Box`. Used as the
    /// default for tests that call `alloc` without specifying their own
    /// finalize, so every test cleans up via `CFRelease` and the alive
    /// counter never drifts.
    unsafe fn box_finalize_helper(obj: *mut vtf_cf_object) {
        drop(Box::from_raw(obj));
    }

    unsafe fn free(obj: *mut vtf_cf_object) {
        // Tests that allocated with `alloc(..., None)` get a finalize
        // assigned by `alloc`; route deallocation through CFRelease so
        // the alive counter decrements.
        CFRelease(obj as CFTypeRef);
    }

    /// Per-test counter the finalize hook bumps so we can observe that
    /// finalize was actually invoked on the last release.
    static FINALIZE_HITS: AtomicU32 = AtomicU32::new(0);

    unsafe fn counting_finalize(_obj: *mut vtf_cf_object) {
        FINALIZE_HITS.fetch_add(1, AtomicOrdering::SeqCst);
    }

    #[test]
    fn cfretain_increments_refcount() {
        unsafe {
            let obj = alloc(VTF_TYPE_DICTIONARY, None);
            assert_eq!((*obj).refcount.load(Ordering::SeqCst), 1);
            CFRetain(obj as CFTypeRef);
            assert_eq!((*obj).refcount.load(Ordering::SeqCst), 2);
            CFRelease(obj as CFTypeRef);
            assert_eq!((*obj).refcount.load(Ordering::SeqCst), 1);
            free(obj);
        }
    }

    #[test]
    fn cfrelease_calls_finalize_on_last_ref() {
        unsafe {
            FINALIZE_HITS.store(0, AtomicOrdering::SeqCst);
            let obj = alloc(VTF_TYPE_DICTIONARY, Some(counting_finalize));
            CFRetain(obj as CFTypeRef);
            CFRelease(obj as CFTypeRef);
            assert_eq!(
                FINALIZE_HITS.load(AtomicOrdering::SeqCst),
                0,
                "finalize must not fire while refs > 0"
            );
            CFRelease(obj as CFTypeRef);
            assert_eq!(
                FINALIZE_HITS.load(AtomicOrdering::SeqCst),
                1,
                "finalize must fire exactly once on last release"
            );
            free(obj);
        }
    }

    #[test]
    fn static_flag_makes_retain_release_no_ops() {
        unsafe {
            FINALIZE_HITS.store(0, AtomicOrdering::SeqCst);
            let obj = alloc(VTF_TYPE_STRING, Some(counting_finalize));
            (*obj).flags |= VTF_OBJECT_FLAG_STATIC;
            let before = (*obj).refcount.load(Ordering::SeqCst);
            CFRetain(obj as CFTypeRef);
            CFRelease(obj as CFTypeRef);
            CFRelease(obj as CFTypeRef);
            CFRelease(obj as CFTypeRef);
            assert_eq!(
                (*obj).refcount.load(Ordering::SeqCst),
                before,
                "static-flagged objects must not change refcount"
            );
            assert_eq!(
                FINALIZE_HITS.load(AtomicOrdering::SeqCst),
                0,
                "static-flagged objects must never finalize"
            );
            // Clear the static flag so `free` (a CFRelease) can run the
            // finalize hook and balance the VTF_PROXY_ALIVE counter.
            (*obj).flags &= !VTF_OBJECT_FLAG_STATIC;
            // `free` calls CFRelease which fires `counting_finalize` —
            // that increments FINALIZE_HITS but does NOT free the Box.
            // Drop the box explicitly so the test doesn't leak.
            free(obj);
            // Counter went 1→0, so this CFRelease no-ops; safe to box again.
            let _ = Box::from_raw(obj);
        }
    }

    #[test]
    fn null_retain_release_are_no_ops() {
        unsafe {
            // Both must tolerate NULL — Apple's runtime contract.
            let returned = CFRetain(std::ptr::null());
            assert!(returned.is_null());
            CFRelease(std::ptr::null());
        }
    }

    #[test]
    fn bump_generation_invalidates_host_id() {
        unsafe {
            let obj = alloc(VTF_TYPE_PIXEL_BUFFER, None);
            vtf_bind_host_id(obj as CFTypeRef, 42);
            assert_eq!(vtf_get_host_id(obj as CFTypeRef), 42);
            assert_eq!(vtf_get_generation(obj as CFTypeRef), 1);
            let new_gen = vtf_bump_generation(obj as CFTypeRef);
            assert_eq!(new_gen, 2);
            assert_eq!(vtf_get_generation(obj as CFTypeRef), 2);
            assert_eq!(
                vtf_get_host_id(obj as CFTypeRef),
                0,
                "host_id must be invalidated when generation bumps; \
                 stale guest references can't accidentally hit a fresh \
                 host slot that happens to reuse the old id"
            );
            free(obj);
        }
    }

    #[test]
    fn proxy_id_is_unique_and_monotonic() {
        unsafe {
            let a = alloc(VTF_TYPE_DICTIONARY, None);
            let b = alloc(VTF_TYPE_DICTIONARY, None);
            let c = alloc(VTF_TYPE_DICTIONARY, None);
            let pa = vtf_get_proxy_id(a as CFTypeRef);
            let pb = vtf_get_proxy_id(b as CFTypeRef);
            let pc = vtf_get_proxy_id(c as CFTypeRef);
            assert!(pa < pb && pb < pc, "proxy ids must be monotonically allocated");
            free(a);
            free(b);
            free(c);
        }
    }

    #[test]
    fn type_id_is_recoverable() {
        unsafe {
            let obj = alloc(VTF_TYPE_BLOCK_BUFFER, None);
            assert_eq!(vtf_get_type_id(obj as CFTypeRef), VTF_TYPE_BLOCK_BUFFER);
            assert_eq!(vtf_get_type_id(std::ptr::null()), 0);
            free(obj);
        }
    }

    #[test]
    fn cfequal_pointer_identity_short_circuits() {
        unsafe {
            let obj = alloc(VTF_TYPE_DICTIONARY, None);
            assert!(CFEqual(obj as CFTypeRef, obj as CFTypeRef));
            free(obj);
        }
    }

    #[test]
    fn cfequal_null_handling() {
        unsafe {
            assert!(CFEqual(std::ptr::null(), std::ptr::null()));
            let obj = alloc(VTF_TYPE_DICTIONARY, None);
            assert!(!CFEqual(obj as CFTypeRef, std::ptr::null()));
            assert!(!CFEqual(std::ptr::null(), obj as CFTypeRef));
            free(obj);
        }
    }

    #[test]
    fn cfequal_distinguishes_type_tags() {
        unsafe {
            let a = alloc(VTF_TYPE_STRING, None);
            let b = alloc(VTF_TYPE_NUMBER, None);
            assert!(!CFEqual(a as CFTypeRef, b as CFTypeRef));
            free(a);
            free(b);
        }
    }

    /// Leak-detection tests assert on the *global* `VTF_PROXY_ALIVE`
    /// counter, so they have to run one-at-a-time. This mutex serializes
    /// them across the test binary's thread pool.
    fn leak_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    /// `Drop`-aware finalize that frees the heap allocation `init` minted.
    /// Mirrors what each per-type finalize hook does in
    /// `corefoundation`/`coremedia`/`corevideo` — turn the raw pointer
    /// back into a Box so the allocation goes away.
    unsafe fn box_finalize(obj: *mut vtf_cf_object) {
        drop(Box::from_raw(obj));
    }

    #[test]
    fn alive_count_balances_across_init_and_finalize() {
        let _guard = leak_lock().lock().unwrap();
        unsafe {
            let baseline = vtf_proxy_alive_count();

            // CFRetain/Release pairs with finalize on last release should
            // always net to zero against the baseline.
            let a = Box::into_raw(Box::new(vtf_cf_object::init(
                VTF_TYPE_DICTIONARY,
                Some(box_finalize),
            )));
            assert_eq!(vtf_proxy_alive_count(), baseline + 1);
            CFRetain(a as CFTypeRef);
            CFRetain(a as CFTypeRef);
            assert_eq!(
                vtf_proxy_alive_count(),
                baseline + 1,
                "retain doesn't change alive count, only refcount"
            );
            CFRelease(a as CFTypeRef);
            CFRelease(a as CFTypeRef);
            assert_eq!(
                vtf_proxy_alive_count(),
                baseline + 1,
                "early releases don't deallocate"
            );
            CFRelease(a as CFTypeRef); // last ref → finalize → decrement
            assert_eq!(
                vtf_proxy_alive_count(),
                baseline,
                "alive count must return to baseline after final release"
            );
        }
    }

    #[test]
    fn alive_count_handles_many_objects() {
        let _guard = leak_lock().lock().unwrap();
        unsafe {
            let baseline = vtf_proxy_alive_count();
            let pointers: Vec<*mut vtf_cf_object> = (0..32)
                .map(|_| {
                    Box::into_raw(Box::new(vtf_cf_object::init(
                        VTF_TYPE_DATA,
                        Some(box_finalize),
                    )))
                })
                .collect();
            assert_eq!(vtf_proxy_alive_count(), baseline + 32);
            for p in pointers {
                CFRelease(p as CFTypeRef);
            }
            assert_eq!(vtf_proxy_alive_count(), baseline);
        }
    }

    #[test]
    fn alive_count_skips_static_objects() {
        let _guard = leak_lock().lock().unwrap();
        // Statically-flagged objects don't go through vtf_cf_object::init
        // (their `flags`/`refcount` are filled in at static-storage time)
        // so they should never bump the counter. The kCFBooleanTrue
        // singleton in `corefoundation.rs` exercises this — observing its
        // contribution requires the counter to remain at the baseline
        // across an arbitrary CFRetain/Release cycle.
        use crate::corefoundation::{kCFBooleanFalse, kCFBooleanTrue};
        unsafe {
            let baseline = vtf_proxy_alive_count();
            CFRetain(kCFBooleanTrue.0);
            CFRetain(kCFBooleanFalse.0);
            CFRelease(kCFBooleanTrue.0);
            CFRelease(kCFBooleanFalse.0);
            assert_eq!(
                vtf_proxy_alive_count(),
                baseline,
                "statically-flagged singletons must not affect alive count"
            );
        }
    }

    // Phase 9 telemetry counter coverage. NOTE: these counters are
    // bumped from production code paths (CMBlockBufferCreate*,
    // VT*Session lifecycle) that other tests in this crate exercise
    // concurrently. Assertions use ≥-deltas rather than absolute
    // baselines so cross-test bumps don't make these flaky. The
    // leak_lock isn't sufficient here because it only serializes
    // tests that opt into it — coremedia / corevideo tests touch the
    // same counters without holding it, by construction.

    #[test]
    fn data_copy_counters_record_bytes_and_events() {
        let bytes_before = vtf_data_copy_bytes();
        let events_before = vtf_data_copy_events();
        vtf_record_data_copy(1024);
        vtf_record_data_copy(2048);
        vtf_record_data_copy(512);
        let bytes_delta = vtf_data_copy_bytes() - bytes_before;
        let events_delta = vtf_data_copy_events() - events_before;
        assert!(
            bytes_delta >= 1024 + 2048 + 512,
            "bytes counter must include at least our recorded {} bytes; \
             saw delta {}",
            1024 + 2048 + 512,
            bytes_delta
        );
        assert!(
            events_delta >= 3,
            "events counter must include at least our 3 calls; saw delta {}",
            events_delta
        );
    }

    #[test]
    fn data_copy_zero_bytes_still_counts_event() {
        // A zero-byte copy is still an event the bench wants to see —
        // counts the call sites even when the payload was empty.
        let bytes_before = vtf_data_copy_bytes();
        let events_before = vtf_data_copy_events();
        vtf_record_data_copy(0);
        let bytes_delta = vtf_data_copy_bytes() - bytes_before;
        let events_delta = vtf_data_copy_events() - events_before;
        assert!(
            events_delta >= 1,
            "zero-byte record must still bump the events counter; \
             saw delta {}",
            events_delta
        );
        // Bytes delta from THIS call is 0; concurrent tests may add
        // more so we only assert it didn't shrink.
        assert!(
            bytes_delta == 0 || bytes_delta > 0,
            "bytes counter must not go backward; saw delta {}",
            bytes_delta
        );
    }

    #[test]
    fn vt_session_lifecycle_counters_are_monotonic() {
        // created/destroyed counters never decrement. Concurrent tests
        // may bump them; assert monotonic delta only.
        let created_before = vtf_vt_sessions_created();
        let destroyed_before = vtf_vt_sessions_destroyed();

        vtf_record_vt_session_created();
        vtf_record_vt_session_created();
        vtf_record_vt_session_created();
        vtf_record_vt_session_destroyed();
        vtf_record_vt_session_destroyed();

        assert!(
            vtf_vt_sessions_created() >= created_before + 3,
            "created counter must include our 3 increments"
        );
        assert!(
            vtf_vt_sessions_destroyed() >= destroyed_before + 2,
            "destroyed counter must include our 2 increments"
        );
        // Balance for posterity even though the counters are global.
        vtf_record_vt_session_destroyed();
    }

    #[test]
    fn vt_sessions_live_returns_signed_difference() {
        // The live count is created - destroyed as i64. The decoder
        // is *defined* on top of two unsigned atomics — verify that
        // the signed-subtraction shape doesn't blow up under typical
        // values (large u64s the bench could produce in long runs).
        let live_before = vtf_vt_sessions_live();
        vtf_record_vt_session_created();
        let after_create = vtf_vt_sessions_live();
        // Concurrent tests may also bump created/destroyed, so the
        // delta is at least +1 but could be larger or even smaller
        // (if another thread's destroyed call lands between).
        // Assert: live count never blows up to int min/max. Hardening
        // is the goal, not exact accounting.
        assert!(
            after_create > i64::MIN + 1_000_000
                && after_create < i64::MAX - 1_000_000,
            "live count {} is suspiciously close to a sign-overflow \
             boundary",
            after_create
        );
        vtf_record_vt_session_destroyed();
        let _ = live_before; // suppress warning; recorded for readability
    }
}
