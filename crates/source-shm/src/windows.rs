use cluster_core::{Snapshot, SourceContext, SourceState};
use std::io;
use std::ptr::null_mut;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::{
    FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub(crate) struct Mapping {
    pub(crate) view: MEMORY_MAPPED_VIEW_ADDRESS,
    len: usize,
}

impl Mapping {
    pub(crate) fn read_only(name: &str, len: usize) -> io::Result<Option<Self>> {
        Self::open(name, len, FILE_MAP_READ)
    }

    #[allow(unsafe_code)]
    pub(crate) fn open(name: &str, len: usize, access: u32) -> io::Result<Option<Self>> {
        let wide = wide_name(name);
        // SAFETY: `wide` is a NUL-terminated UTF-16 string. The view is closed on drop.
        let handle = unsafe { OpenFileMappingW(access, 0, wide.as_ptr()) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) { Ok(None) } else { Err(error) };
        }
        // SAFETY: the handle is valid and can be closed once the OS creates the view.
        unsafe {
            let view = MapViewOfFile(handle, access, 0, 0, len);
            let result =
                if view.Value.is_null() { Err(io::Error::last_os_error()) } else { Ok(Some(Self { view, len })) };
            CloseHandle(handle);
            result
        }
    }

    #[allow(unsafe_code)]
    pub(crate) fn read(&self, offset: usize, dest: &mut [u8]) -> io::Result<()> {
        if offset > self.len || dest.len() > self.len - offset {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "shared-memory read exceeds the mapped view"));
        }
        // SAFETY: the bounded address belongs to this live view and dest is writable for its full length.
        // Windows copies into our buffer, so we create no Rust references to the live game data.
        let result = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                self.view.Value.cast::<u8>().add(offset).cast(),
                dest.as_mut_ptr().cast(),
                dest.len(),
                null_mut(),
            )
        };
        if result != 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }
}

impl Drop for Mapping {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: only this object owns the view and drop runs once.
        unsafe { UnmapViewOfFile(self.view) };
    }
}

pub(crate) fn wide_name(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) type Sample = ((u64, i32), Snapshot);

pub(crate) fn poll<R>(
    ctx: &SourceContext,
    open: impl Fn() -> io::Result<Option<R>>,
    sample: impl Fn(&R) -> io::Result<Option<Sample>>,
) -> anyhow::Result<()> {
    const RETRY: Duration = Duration::from_millis(100);
    const FRAME: Duration = Duration::from_millis(16);
    const STALE: Duration = Duration::from_secs(2);
    let mut key = None;
    let mut seq = 0;
    let mut live = false;
    let waiting = "Start a driving session on this PC";
    ctx.set_state(SourceState::Waiting, waiting);
    while !ctx.should_stop() {
        let Some(reader) = open()? else {
            std::thread::sleep(RETRY);
            continue;
        };
        let mut last_change = Instant::now();
        while !ctx.should_stop() && last_change.elapsed() < STALE {
            if let Some((new_key, mut snapshot)) = sample(&reader)?
                && key != Some(new_key)
            {
                key = Some(new_key);
                last_change = Instant::now();
                seq += 1;
                snapshot.seq = seq;
                ctx.publish(snapshot);
                if !live {
                    ctx.set_state(SourceState::Live, "Reading local shared memory");
                    live = true;
                }
            }
            std::thread::sleep(FRAME);
        }
        drop(reader); // let the kernel release the old mapping after the game quits
        if live && !ctx.should_stop() {
            let mut empty = Snapshot::new();
            seq += 1;
            empty.seq = seq;
            ctx.publish(empty);
            ctx.set_state(SourceState::Waiting, waiting);
            live = false;
        }
        std::thread::sleep(RETRY);
    }
    Ok(())
}
