//! The console of a Windows release build. Such a build is a GUI program, so
//! that starting it from the desktop opens no console window, and Windows
//! gives a GUI program no console even when one started it. This takes the
//! console of the program that started the emulator, if it has one.

/// Sends printed output to the console that the emulator was started from.
/// A start from the desktop has no console. A handle that is already set,
/// for example by `> file`, stays.
#[cfg(all(windows, not(debug_assertions)))]
pub fn attach() {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    }

    // SAFETY: the three take plain values and keep nothing that we own.
    // SetStdHandle gets the handle of an open file and keeps it permanently.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle as isize != -1 {
                continue;
            }
            if let Ok(console) = OpenOptions::new().read(true).write(true).open("CONOUT$") {
                SetStdHandle(which, console.into_raw_handle().cast());
            }
        }
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
pub fn attach() {}
