//! Shared, leased process-exit notifications. One OS registration serves each PID.

use anyhow::{Result, bail};
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
            && !*held.exited.borrow()
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

    pub(crate) async fn wait(&self) {
        let mut exited = self.exited.clone();
        let _ = exited.wait_for(|exited| *exited).await;
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
            Ok(fd) => Ok(Registration(Some(AsyncFd::new(fd)?))),
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
mod platform {
    use super::*;
    use anyhow::Context;
    use std::{
        io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
        sync::atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::oneshot;

    static REACTOR: LazyLock<Mutex<Weak<Reactor>>> = LazyLock::new(Mutex::default);

    struct Reactor {
        queue: Arc<OwnedFd>,
        listeners: Arc<Mutex<Listeners>>,
        stopping: Arc<AtomicBool>,
        failed: Arc<AtomicBool>,
        #[cfg(test)]
        stopped: watch::Receiver<bool>,
    }

    #[derive(Default)]
    struct Listeners {
        next: usize,
        waiters: HashMap<usize, oneshot::Sender<()>>,
        processes: HashMap<u32, usize>,
    }

    pub(super) struct Registration {
        reactor: Arc<Reactor>,
        pid: u32,
        token: usize,
        exited: Option<oneshot::Receiver<()>>,
    }

    fn change(queue: &OwnedFd, event: &libc::kevent) -> io::Result<()> {
        loop {
            // SAFETY: event is initialized and valid for one input entry. No output is requested.
            let result = unsafe {
                libc::kevent(
                    queue.as_raw_fd(),
                    event,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if result >= 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    fn wake(queue: &OwnedFd) -> io::Result<()> {
        change(
            queue,
            &libc::kevent {
                ident: 0,
                filter: libc::EVFILT_USER,
                flags: 0,
                fflags: libc::NOTE_TRIGGER,
                data: 0,
                udata: std::ptr::null_mut(),
            },
        )
    }

    impl Reactor {
        fn new() -> Result<Arc<Self>> {
            // SAFETY: kqueue takes no pointers and returns a new owned descriptor.
            let raw = unsafe { libc::kqueue() };
            if raw < 0 {
                return Err(io::Error::last_os_error().into());
            }
            // SAFETY: raw is a newly opened descriptor; ownership is transferred exactly once.
            let queue = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
            change(
                &queue,
                &libc::kevent {
                    ident: 0,
                    filter: libc::EVFILT_USER,
                    flags: libc::EV_ADD | libc::EV_CLEAR,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                },
            )?;
            let listeners = Arc::new(Mutex::new(Listeners::default()));
            let stopping = Arc::new(AtomicBool::new(false));
            let failed = Arc::new(AtomicBool::new(false));
            #[cfg(test)]
            let (stopped_tx, stopped) = watch::channel(false);
            let held = Arc::new(Self {
                queue: queue.clone(),
                listeners: listeners.clone(),
                stopping: stopping.clone(),
                failed: failed.clone(),
                #[cfg(test)]
                stopped,
            });
            // Tokio's macOS reactor cannot register another kqueue as an AsyncFd.
            // One thread waits for every leased PID and exits when the last lease drops.
            std::thread::Builder::new()
                .name("beep-process-exits".into())
                .spawn(move || {
                    let mut events = [libc::kevent {
                        ident: 0,
                        filter: 0,
                        flags: 0,
                        fflags: 0,
                        data: 0,
                        udata: std::ptr::null_mut(),
                    }; 64];
                    while !stopping.load(Ordering::Acquire) {
                        // SAFETY: events supplies space for the requested number of initialized output entries.
                        // queue remains owned throughout the wait; no input entries or timeout are supplied.
                        let count = unsafe {
                            libc::kevent(
                                queue.as_raw_fd(),
                                std::ptr::null(),
                                0,
                                events.as_mut_ptr(),
                                64,
                                std::ptr::null(),
                            )
                        };
                        if count < 0 {
                            let error = io::Error::last_os_error();
                            if error.kind() == io::ErrorKind::Interrupted {
                                continue;
                            }
                            failed.store(true, Ordering::Release);
                            log::debug!("process exit reactor stopped: {error}");
                            break;
                        }
                        for event in events.iter().take(usize::try_from(count).unwrap_or(0)) {
                            if event.filter == libc::EVFILT_PROC {
                                // udata is an opaque integer token, never dereferenced as a pointer.
                                let token = event.udata as usize;
                                let mut held = crate::lock(&listeners);
                                if let Ok(pid) = u32::try_from(event.ident)
                                    && held.processes.get(&pid) == Some(&token)
                                {
                                    held.processes.remove(&pid);
                                }
                                if let Some(sender) = held.waiters.remove(&token) {
                                    let _ = sender.send(());
                                }
                            }
                        }
                    }
                    let mut held = crate::lock(&listeners);
                    held.waiters.clear();
                    held.processes.clear();
                    #[cfg(test)]
                    stopped_tx.send_replace(true);
                })
                .context("starting process exit reactor")?;
            Ok(held)
        }
    }

    impl Drop for Reactor {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::Release);
            if let Err(error) = wake(&self.queue) {
                log::debug!("process exit reactor could not be woken: {error}");
            }
            // The detached thread owns only queue/listeners, so it cannot retain this lease.
            // Its wake releases those final resources without joining on the async executor.
        }
    }

    pub(super) fn register(pid: u32) -> Result<Registration> {
        let reactor = {
            let mut shared = crate::lock(&REACTOR);
            match shared.upgrade() {
                Some(reactor) if !reactor.failed.load(Ordering::Acquire) => reactor,
                _ => {
                    let reactor = Reactor::new()?;
                    *shared = Arc::downgrade(&reactor);
                    reactor
                }
            }
        };
        let mut listeners = crate::lock(&reactor.listeners);
        let token = listeners
            .next
            .checked_add(1)
            .context("process exit registration ids exhausted")?;
        listeners.next = token;
        let (sender, exited) = oneshot::channel();
        let event = libc::kevent {
            ident: usize::try_from(pid)?,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: token as *mut std::ffi::c_void,
        };
        if let Some(previous) = listeners.processes.insert(pid, token) {
            listeners.waiters.remove(&previous);
        }
        listeners.waiters.insert(token, sender);
        if let Err(error) = change(&reactor.queue, &event) {
            let sender = listeners.waiters.remove(&token);
            listeners.processes.remove(&pid);
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
            // The process exited between discovery and registration.
            if let Some(sender) = sender {
                let _ = sender.send(());
            }
        }
        drop(listeners);
        Ok(Registration {
            reactor,
            pid,
            token,
            exited: Some(exited),
        })
    }

    impl Registration {
        pub(super) async fn wait(mut self) -> Result<()> {
            if let Some(exited) = self.exited.take() {
                exited.await.context("process exit reactor closed")?;
            }
            Ok(())
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // A delivered event has already removed its token. Do not delete a newer
            // registration if the system reused that PID before this lease was dropped.
            let mut listeners = crate::lock(&self.reactor.listeners);
            listeners.waiters.remove(&self.token);
            if listeners.processes.get(&self.pid) == Some(&self.token) {
                listeners.processes.remove(&self.pid);
                let event = libc::kevent {
                    ident: self.pid as usize,
                    filter: libc::EVFILT_PROC,
                    flags: libc::EV_DELETE,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                let _ = change(&self.reactor.queue, &event);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn dropping_the_last_lease_wakes_and_stops_the_shared_reactor() {
            let reactor = Reactor::new().unwrap();
            let weak = Arc::downgrade(&reactor);
            let mut stopped = reactor.stopped.clone();
            drop(reactor);
            assert!(weak.upgrade().is_none());
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                stopped.wait_for(|done| *done),
            )
            .await
            .unwrap()
            .unwrap();
        }
    }
}

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
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_and_rewatching_a_pid_keeps_the_new_registration() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "read message"])
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        drop(Exit::watch(pid).unwrap());
        let replacement = Exit::watch(pid).unwrap();
        // Let the cancelled registration clean up after its replacement was installed.
        tokio::task::yield_now().await;
        drop(child.stdin.take());
        tokio::time::timeout(std::time::Duration::from_secs(5), replacement.wait())
            .await
            .unwrap();
        child.wait().await.unwrap();
    }

    #[tokio::test]
    async fn one_kernel_registration_is_shared_and_notifies_without_a_file_write() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "read message"])
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let first = Exit::watch(pid).unwrap();
        let second = Exit::watch(pid).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        drop(child.stdin.take());
        tokio::time::timeout(std::time::Duration::from_secs(5), first.wait())
            .await
            .unwrap();
        child.wait().await.unwrap();
        drop(first);
        drop(second);
        assert!(
            crate::lock(&PROCESSES)
                .get(&pid)
                .unwrap()
                .upgrade()
                .is_none()
        );
    }
}
