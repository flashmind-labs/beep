use super::process::Process;
use std::{
    collections::HashMap,
    io,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{FILETIME, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            TerminateProcess,
        },
    },
};

fn open(pid: u32, access: u32) -> io::Result<OwnedHandle> {
    // SAFETY: takes plain integers and returns a new handle or null.
    let handle = unsafe { OpenProcess(access, 0, pid) };

    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: this is a new handle, owned only here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn birth(handle: &OwnedHandle) -> io::Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the handle is open and each pointer is writable for a FILETIME.
    if unsafe {
        GetProcessTimes(
            handle.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if exit.dwHighDateTime != 0 || exit.dwLowDateTime != 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the process has exited",
        ));
    }

    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

pub(super) async fn snapshot() -> anyhow::Result<HashMap<u32, Process>> {
    tokio::task::spawn_blocking(|| {
        // SAFETY: plain integers; returns a new handle or INVALID_HANDLE_VALUE.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };

        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: the new snapshot has no other owner.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())?,
            ..Default::default()
        };
        let mut all = HashMap::new();
        // SAFETY: the snapshot is open and entry carries the expected size.
        let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };

        while found != 0 {
            let pid = entry.th32ProcessID;

            if let Ok(handle) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)
                && let Ok(birth) = birth(&handle)
            {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                all.insert(
                    pid,
                    Process {
                        pid,
                        parent: entry.th32ParentProcessID,
                        group: 0,
                        birth,
                        started_at: (birth / 10_000_000).saturating_sub(11_644_473_600),
                        command: String::from_utf16_lossy(&entry.szExeFile[..end]).into(),
                    },
                );
            }
            // SAFETY: as above, the snapshot and buffer remain valid.
            found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
        }

        Ok(all)
    })
    .await?
}

pub(super) fn alive(p: &Process) -> bool {
    open(p.pid, PROCESS_QUERY_LIMITED_INFORMATION)
        .and_then(|h| birth(&h))
        .is_ok_and(|b| b == p.birth)
}

pub(super) fn signal(p: &Process, _kill: bool) -> io::Result<()> {
    let Ok(handle) = open(p.pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE) else {
        return Ok(());
    };

    if birth(&handle)? != p.birth {
        return Ok(());
    }
    // SAFETY: this handle names the same observed process, and stays open.
    if unsafe { TerminateProcess(handle.as_raw_handle(), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

pub(super) async fn ports(_: &[Process]) -> HashMap<u32, Vec<u16>> {
    HashMap::new()
}
