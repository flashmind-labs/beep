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
#[path = "../../tests/events_exits_macos.rs"]
mod tests;
