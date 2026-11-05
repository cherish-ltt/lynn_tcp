use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
};

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every operation forwards the original pointer/layout to System without altering it.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size as u64, Ordering::Relaxed);
        }
        pointer
    }
}

pub(super) fn snapshot(handled: u64) -> std::io::Result<String> {
    let calls = CALLS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let resources = {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
        let rss_scale = if cfg!(target_os = "macos") { 1 } else { 1024 };
        format!(
            "{{\"user_cpu_secs\":{},\"system_cpu_secs\":{},\"peak_rss_bytes\":{}}}",
            seconds(usage.ru_utime),
            seconds(usage.ru_stime),
            usage.ru_maxrss as u64 * rss_scale,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let resources = "null";
    Ok(format!(
        "{{\"allocation_calls\":{calls},\"requested_bytes\":{bytes},\"handled_requests\":{handled},\"resources\":{resources}}}\n"
    ))
}
