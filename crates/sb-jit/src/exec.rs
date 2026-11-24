//! Executable memory: written while it is not executable, then made
//! executable and never written again (W^X).
//!
//! PROTOTYPE LIMIT: Linux only. Each mapping holds one function and takes at
//! least a page.

use crate::Ctx;

/// The function type of compiled code.
pub(crate) type Entry = unsafe extern "C" fn(*mut Ctx) -> u32;

/// A mapping holding one function, executable and read-only.
pub(crate) struct Mem {
    ptr: *mut u8,
    mapped: usize,
    len: usize,
}

impl Mem {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn entry(&self) -> Entry {
        // SAFETY: `ptr` is the start of the function `new` copied in, mapped
        // executable for as long as `self` lives.
        unsafe { std::mem::transmute::<*mut u8, Entry>(self.ptr) }
    }
}

#[cfg(target_os = "linux")]
mod os {
    use std::ffi::{c_int, c_long, c_void};

    pub const PROT_READ: c_int = 1;
    pub const PROT_WRITE: c_int = 2;
    pub const PROT_EXEC: c_int = 4;
    pub const MAP_PRIVATE: c_int = 2;
    pub const MAP_ANONYMOUS: c_int = 0x20;

    unsafe extern "C" {
        pub fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            off: c_long,
        ) -> *mut c_void;
        pub fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
        pub fn munmap(addr: *mut c_void, len: usize) -> c_int;
    }
}

#[cfg(target_os = "linux")]
impl Mem {
    /// Copies `code` into a new mapping and makes it executable.
    pub(crate) fn new(code: &[u8]) -> Result<Mem, String> {
        use std::ffi::c_void;
        // The kernel rounds the length up to whole pages, regardless of the
        // page size, for mmap, mprotect and munmap alike.
        let mapped = code.len().max(1);
        // SAFETY: a new private anonymous mapping, which aliases nothing.
        let ptr = unsafe {
            os::mmap(
                std::ptr::null_mut(),
                mapped,
                os::PROT_READ | os::PROT_WRITE,
                os::MAP_PRIVATE | os::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr as isize == -1 || ptr.is_null() {
            return Err("mmap refused".into());
        }
        let ptr = ptr.cast::<u8>();
        let mem = Mem {
            ptr,
            mapped,
            len: code.len(),
        };
        // SAFETY: the mapping is `mapped` bytes, writable, and used by nothing else.
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len()) };
        // SAFETY: the whole mapping, which `mem` owns.
        let made =
            unsafe { os::mprotect(ptr.cast::<c_void>(), mapped, os::PROT_READ | os::PROT_EXEC) };
        if made != 0 {
            return Err("mprotect refused".into());
        }
        flush_icache(ptr, code.len());
        Ok(mem)
    }
}

#[cfg(not(target_os = "linux"))]
impl Mem {
    pub(crate) fn new(_code: &[u8]) -> Result<Mem, String> {
        Err("no executable memory on this OS in the prototype".into())
    }
}

impl Drop for Mem {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        // SAFETY: the mapping `new` made, which nothing uses once `self` is
        // dropped: `Code::run` borrows `self` while code runs.
        unsafe {
            os::munmap(self.ptr.cast(), self.mapped);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (self.ptr, self.mapped);
    }
}

/// Makes the instruction fetch of this core see what was written at
/// `ptr..ptr + len`: Arm's caches for data and instructions are not kept
/// coherent by the hardware. The same sequence as libgcc's `__clear_cache`.
/// The code runs only on the thread that wrote it, so no other core needs
/// to be told.
#[cfg(target_arch = "aarch64")]
fn flush_icache(ptr: *mut u8, len: usize) {
    use std::arch::asm;
    let start = ptr as usize;
    let end = start + len;
    let ctr: u64;
    // SAFETY: CTR_EL0 is readable at EL0 on Linux, and reading it changes
    // nothing.
    unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack)) };
    let dline = 4usize << ((ctr >> 16) & 0xF);
    let iline = 4usize << (ctr & 0xF);
    // IDC: the data cache does not need to be cleaned to the point of
    // unification.
    if ctr & (1 << 28) == 0 {
        let mut at = start & !(dline - 1);
        while at < end {
            // SAFETY: cleans a line of memory this process has mapped.
            unsafe { asm!("dc cvau, {}", in(reg) at, options(nostack)) };
            at += dline;
        }
    }
    // SAFETY: barriers only.
    unsafe { asm!("dsb ish", options(nostack)) };
    // DIC: the instruction cache does not need to be invalidated.
    if ctr & (1 << 29) == 0 {
        let mut at = start & !(iline - 1);
        while at < end {
            // SAFETY: invalidates a line of memory this process has mapped.
            unsafe { asm!("ic ivau, {}", in(reg) at, options(nostack)) };
            at += iline;
        }
        // SAFETY: a barrier only.
        unsafe { asm!("dsb ish", options(nostack)) };
    }
    // SAFETY: a barrier only.
    unsafe { asm!("isb", options(nostack)) };
}

/// x86 keeps instruction fetch coherent with stores.
#[cfg(not(target_arch = "aarch64"))]
fn flush_icache(_ptr: *mut u8, _len: usize) {}
