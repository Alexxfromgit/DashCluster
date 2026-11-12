//! Windows release builds are GUI-subsystem apps (no console window). When started from a
//! terminal with arguments, attach to that terminal so `--help`, errors and logs are visible.

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole has no memory-safety preconditions. It fails harmlessly when there
    // is no parent console or one is already attached (debug builds).
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}
