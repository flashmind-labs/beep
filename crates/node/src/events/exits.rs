//! Shared, leased process-exit notifications. One OS registration serves each PID.

use anyhow::{Result, bail};
use beep_proto::watched;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

const MAX_PROCESSES: usize = 512;
static PROCESSES: LazyLock<Mutex<HashMap<u32, Weak<Exit>>>> = LazyLock::new(Mutex::default);

pub(crate) struct Exit {
    exited: watch::Receiver<bool>,
    _wait: AbortOnDropHandle<()>,
}

impl Exit {
    pub(crate) fn watch(pid: u32) -> Result<Arc<Self>> {
        if pid == 0 {
            bail!("invalid process id");
        }

        let mut processes = crate::lock(&PROCESSES);
        processes.retain(|_, entry| entry.strong_count() > 0);

        if let Some(held) = processes.get(&pid).and_then(Weak::upgrade)
            && !watched::read(&held.exited, |exited| *exited)
        {
            return Ok(held);
        }

        processes.remove(&pid);

        if processes.len() >= MAX_PROCESSES {
            bail!("too many observed external processes");
        }

        let registration = platform::register(pid)?;
        let (sender, exited) = watch::channel(false);
        let held = Arc::new(Self {
            exited,
            _wait: AbortOnDropHandle::new(tokio::spawn(async move {
                if let Err(error) = registration.wait().await {
                    log::debug!("process exit notification failed: {error}");
                }

                sender.send_replace(true);
            })),
        });
        processes.insert(pid, Arc::downgrade(&held));
        Ok(held)
    }

    /// Whether the process has exited, as last heard.
    pub(crate) fn exited(&self) -> bool {
        watched::read(&self.exited, |exited| *exited)
    }

    pub(crate) async fn wait(&self) {
        let mut exited = self.exited.clone();
        let _ = watched::wait_for(&mut exited, |exited| *exited).await;
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use anyhow::Context;
    use rustix::process::{Pid, PidfdFlags, pidfd_open};
    use std::os::fd::OwnedFd;
    use tokio::io::unix::AsyncFd;

    pub(super) struct Registration(Option<AsyncFd<OwnedFd>>);

    pub(super) fn register(pid: u32) -> Result<Registration> {
        let pid = Pid::from_raw(i32::try_from(pid)?).context("invalid process id")?;

        match pidfd_open(pid, PidfdFlags::NONBLOCK) {
            // SAFETY: the `OwnedFd` moves into the `AsyncFd`, which owns it
            // until it drops, so the descriptor stays open and unchanged.
            Ok(fd) => Ok(Registration(Some(unsafe { AsyncFd::register(fd) }?))),
            Err(rustix::io::Errno::SRCH) => Ok(Registration(None)),
            Err(error) => Err(error.into()),
        }
    }

    impl Registration {
        pub(super) async fn wait(self) -> Result<()> {
            if let Some(fd) = self.0 {
                let _ = fd.readable().await?;
            }

            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "exits/macos.rs"]
mod platform;

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::c_void,
        io,
        os::windows::io::{FromRawHandle, OwnedHandle},
    };
    use tokio::sync::oneshot;
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::Threading::{
            INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, RegisterWaitForSingleObject,
            UnregisterWaitEx, WT_EXECUTEONLYONCE,
        },
    };

    struct Callback(Mutex<Option<oneshot::Sender<()>>>);

    pub(super) struct Registration {
        // Windows uses pointer-shaped handles. These integers are never dereferenced in Rust.
        wait: usize,
        callback: usize,
        process: Option<OwnedHandle>,
        exited: Option<oneshot::Receiver<()>>,
    }

    unsafe extern "system" fn completed(context: *mut c_void, _: bool) {
        // SAFETY: the registration owns this Arc reference until synchronous unregister completes.
        let context = unsafe { &*context.cast::<Callback>() };

        if let Some(sender) = crate::lock(&context.0).take() {
            let _ = sender.send(());
        }
    }

    pub(super) fn register(pid: u32) -> Result<Registration> {
        // SAFETY: no pointers are supplied; the result is checked before taking ownership.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };

        if process.is_null() {
            let error = io::Error::last_os_error();

            if error.raw_os_error() == Some(87) {
                return Ok(Registration {
                    wait: 0,
                    callback: 0,
                    process: None,
                    exited: None,
                });
            }

            return Err(error.into());
        }
        // SAFETY: OpenProcess returned a fresh owned process handle.
        let process = unsafe { OwnedHandle::from_raw_handle(process) };
        let (sender, exited) = oneshot::channel();
        let callback = Arc::into_raw(Arc::new(Callback(Mutex::new(Some(sender)))));
        let mut wait: HANDLE = std::ptr::null_mut();
        use std::os::windows::io::AsRawHandle;
        // SAFETY: output points to live storage; callback remains allocated until unregister below.
        let result = unsafe {
            RegisterWaitForSingleObject(
                &mut wait,
                process.as_raw_handle(),
                Some(completed),
                callback.cast(),
                INFINITE,
                WT_EXECUTEONLYONCE,
            )
        };

        if result == 0 {
            // SAFETY: registration failed, so no callback owns or uses this Arc reference.
            drop(unsafe { Arc::from_raw(callback) });
            return Err(io::Error::last_os_error().into());
        }

        Ok(Registration {
            wait: wait as usize,
            callback: callback as usize,
            process: Some(process),
            exited: Some(exited),
        })
    }

    impl Registration {
        pub(super) async fn wait(mut self) -> Result<()> {
            if let Some(exited) = self.exited.take() {
                let _ = exited.await;
            }

            Ok(())
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            if self.wait == 0 {
                return;
            }

            let wait = self.wait;
            let callback = self.callback;
            let process = self.process.take();
            // Unregister waits for any in-flight callback; keep it off the async executor.
            let cleanup = move || {
                // SAFETY: wait belongs to this registration. INVALID_HANDLE_VALUE waits for callbacks.
                let done = unsafe { UnregisterWaitEx(wait as HANDLE, INVALID_HANDLE_VALUE) };

                if done == 0 {
                    // Preserve the callback and its handle if Windows cannot confirm cancellation.
                    // Leaking this failed registration is safer than freeing an in-flight callback.
                    std::mem::forget(process);
                    return;
                }
                // SAFETY: unregister completed, so the callback can no longer access this reference.
                drop(unsafe { Arc::from_raw(callback as *const Callback) });
                drop(process);
            };
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(cleanup);
            } else {
                cleanup();
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use super::*;
    pub(super) struct Registration;
    pub(super) fn register(_: u32) -> Result<Registration> {
        bail!("process exit notifications are unavailable")
    }
    impl Registration {
        pub(super) async fn wait(self) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/events_exits.rs"]
mod tests;
