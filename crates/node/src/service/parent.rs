//! Windows: the service's node ends with the `conhost` that runs it. Task
//! Scheduler stops a task by ending the process it started, which leaves
//! that process's children running.

use anyhow::{Context, Result, bail};
use std::{
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process,
};
use windows_sys::Win32::{
    Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    },
};

/// Opens this process's parent, to wait for it.
pub(super) fn open_parent() -> Result<OwnedHandle> {
    let parent = parent_pid().context("finding the parent process")?;
    // SAFETY: OpenProcess takes plain values and returns a new handle or
    // null.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
    if handle.is_null() {
        return Err(io::Error::last_os_error()).context("opening the parent process");
    }
    // SAFETY: a new handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Blocks until the process `handle` names exits.
pub(super) fn wait(handle: &OwnedHandle) -> Result<()> {
    // SAFETY: the handle stays open for the call.
    if unsafe { WaitForSingleObject(handle.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error()).context("waiting for the parent process");
    }
    Ok(())
}

fn parent_pid() -> Result<u32> {
    // SAFETY: takes plain values and returns a new handle or an invalid one.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: a new handle that nothing else owns.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let me = process::id();
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())?,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: the snapshot is open and `entry` has its size set, as both
    // calls require.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    while found != 0 {
        if entry.th32ProcessID == me {
            return Ok(entry.th32ParentProcessID);
        }
        // SAFETY: as above.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    bail!("this process is not in the process list")
}
