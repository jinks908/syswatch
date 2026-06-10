//! Measured macOS disk IO via IOKit.
//!
//! `IOBlockStorageDriver` registry entries carry a `Statistics`
//! dictionary with cumulative `Bytes (Read)` / `Bytes (Write)` per
//! device — the same source `iostat` reads. Device-level truth,
//! including kernel and page-cache IO that the previous
//! sum-of-process-counters fallback missed entirely.
//!
//! Raw FFI rather than an io-kit crate: we touch five IOKit calls and
//! four CF calls, not worth a dependency tree.

#![cfg(target_os = "macos")]

use std::os::raw::{c_char, c_void};

type CFTypeRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type IoObject = u32; // mach port; 0 = null
type KernReturn = i32;

const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const KCF_NUMBER_SINT64_TYPE: isize = 4;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    /// Consumes `matching` (CF-release transferred), per IOKit docs.
    fn IOServiceGetMatchingServices(
        master_port: u32,
        matching: CFMutableDictionaryRef,
        existing: *mut IoObject,
    ) -> KernReturn;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> KernReturn;
    fn IORegistryEntryCreateCFProperties(
        entry: IoObject,
        properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> KernReturn;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, the_type: isize, value_ptr: *mut c_void) -> bool;
    fn CFRelease(cf: CFTypeRef);
}

/// Cumulative (bytes_read, bytes_written) summed across every block
/// storage driver. None when IOKit yields no statistics (sandbox, VM
/// without block devices) — caller falls back to the old approximation.
pub fn collect_block_io() -> Option<(u64, u64)> {
    // SAFETY: standard IOKit iterate-and-release. Every CF object we
    // create or receive ownership of is released exactly once; values
    // from CFDictionaryGetValue are borrowed (Get rule) and not
    // released. All pointers are checked before dereference.
    unsafe {
        let matching = IOServiceMatching(c"IOBlockStorageDriver".as_ptr());
        if matching.is_null() {
            return None;
        }
        let mut iter: IoObject = 0;
        // 0 = kIOMasterPortDefault. IOServiceGetMatchingServices
        // consumes `matching` even on failure.
        if IOServiceGetMatchingServices(0, matching, &mut iter) != 0 || iter == 0 {
            return None;
        }

        let key_stats = cfstr(c"Statistics".as_ptr());
        let key_read = cfstr(c"Bytes (Read)".as_ptr());
        let key_write = cfstr(c"Bytes (Write)".as_ptr());

        let mut read_total: u64 = 0;
        let mut write_total: u64 = 0;
        let mut seen = false;
        loop {
            let svc = IOIteratorNext(iter);
            if svc == 0 {
                break;
            }
            let mut props: CFMutableDictionaryRef = std::ptr::null_mut();
            if IORegistryEntryCreateCFProperties(svc, &mut props, std::ptr::null(), 0) == 0
                && !props.is_null()
            {
                let stats = CFDictionaryGetValue(props as CFDictionaryRef, key_stats);
                if !stats.is_null() {
                    seen = true;
                    read_total =
                        read_total.saturating_add(dict_u64(stats as CFDictionaryRef, key_read));
                    write_total =
                        write_total.saturating_add(dict_u64(stats as CFDictionaryRef, key_write));
                }
                CFRelease(props as CFTypeRef);
            }
            IOObjectRelease(svc);
        }
        IOObjectRelease(iter);
        CFRelease(key_stats as CFTypeRef);
        CFRelease(key_read as CFTypeRef);
        CFRelease(key_write as CFTypeRef);

        seen.then_some((read_total, write_total))
    }
}

unsafe fn cfstr(s: *const c_char) -> CFStringRef {
    CFStringCreateWithCString(std::ptr::null(), s, KCF_STRING_ENCODING_UTF8)
}

unsafe fn dict_u64(dict: CFDictionaryRef, key: CFStringRef) -> u64 {
    let num = CFDictionaryGetValue(dict, key as CFTypeRef);
    if num.is_null() {
        return 0;
    }
    let mut v: i64 = 0;
    if CFNumberGetValue(
        num,
        KCF_NUMBER_SINT64_TYPE,
        &mut v as *mut i64 as *mut c_void,
    ) {
        v.max(0) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_block_io_is_monotonic_and_nonzero() {
        // Any real Mac has done disk IO by the time tests run. Two
        // reads a moment apart must be Some, nonzero, and monotonic.
        let Some((r1, w1)) = collect_block_io() else {
            // VM/sandbox without block storage drivers — nothing to assert.
            return;
        };
        assert!(r1 > 0 || w1 > 0);
        let (r2, w2) = collect_block_io().expect("second read");
        assert!(r2 >= r1);
        assert!(w2 >= w1);
    }
}
