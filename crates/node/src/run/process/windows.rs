//! Job objects: every process a child starts joins its job, and closing
//! or terminating the job ends them all.

use std::{
    ffi::c_void,
    io,
    mem::size_of,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
};
use tokio::process::{Child, Command};
use windows_sys::Win32::System::{
    JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    },
    Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW},
};

pub(crate) type Held = OwnedHandle;

/// The exit code the processes of a killed job get.
const KILLED: u32 = 1;

/// A job holding a child and everything it starts. The processes die when
/// the last handle to it closes, or on `kill`.
#[derive(Debug, Clone)]
pub(crate) struct Group(Arc<OwnedHandle>);

impl Group {
    pub(super) fn of(child: &Child) -> io::Result<Option<Self>> {
        let Some(process) = child.raw_handle() else {
            return Ok(None);
        };
        // SAFETY: both arguments may be null: no security attributes and no
        // name.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `job` is a new handle that nothing else owns.
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(io::Error::other)?;
        // SAFETY: the job handle is open, and `limits` is the struct this
        // information class takes, `size` bytes long.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast::<c_void>(),
                size,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both handles are open: the job, and the child's process,
        // which tokio holds until the child is dropped.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(Self(Arc::new(job))))
    }

    /// Windows cannot ask a console program to end without sharing its
    /// console, so this ends the job at once, like `kill`.
    pub(crate) fn terminate(&self) {
        self.kill();
    }

    /// Ends every process in the job.
    pub(crate) fn kill(&self) {
        // SAFETY: the job handle stays open while `self` holds it.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), KILLED) } == 0 {
            log::warn!(
                "could not end a job's processes: {}",
                io::Error::last_os_error()
            );
        }
    }
}

/// No console window, which a node running hidden would otherwise open for
/// every console program, and a process group of its own.
pub(super) fn own_group(command: &mut Command) -> &mut Command {
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
}

pub(super) fn hold_output(child: &Child) -> Vec<Held> {
    let stdout = child.stdout.as_ref().map(AsHandle::as_handle);
    let stderr = child.stderr.as_ref().map(AsHandle::as_handle);
    stdout
        .into_iter()
        .chain(stderr)
        .filter_map(|handle| match handle.try_clone_to_owned() {
            Ok(held) => Some(held),
            Err(e) => {
                log::debug!("could not hold a pipe of the child: {e}");
                None
            }
        })
        .collect()
}
