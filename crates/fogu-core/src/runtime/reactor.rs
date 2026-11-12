//! Socket readiness on the interpreter thread (Linux).
//!
//! A task that reads a socket with nothing to read used to hand the read to
//! the I/O threads and wait for the answer: two wake-ups of sleeping threads
//! per operation, tens of microseconds each, with the interpreter idle in
//! between. Here the socket is read and written directly (non-blocking
//! system calls), and when it is not ready the task parks behind a
//! [`Gate`] that this thread itself opens: when no task can run, the
//! scheduler sleeps in `epoll_wait` on the sockets its tasks wait for (and on
//! an `eventfd` that [`sched::wake`] writes to, for work that finishes on
//! other threads). One thread, no hand-over — what Go's netpoller and Bun do.
//!
//! Each interpreter thread (the main one, each `sync.Worker`) has its own.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::sched::Gate;

/// Reactors that exist: `wake` has nothing to do while there is none.
static REACTORS: AtomicUsize = AtomicUsize::new(0);
/// The eventfd of each reactor and whether its thread is (about to be) asleep.
static WAKERS: Mutex<Vec<(RawFd, Arc<AtomicBool>)>> = Mutex::new(Vec::new());

#[derive(Default)]
struct Entry {
    read: Option<Gate>,
    write: Option<Gate>,
    /// Known to the epoll instance.
    added: bool,
    /// It became readable while nobody waited for that: the next reader
    /// need not wait (see [`take_readable`]).
    ready_read: bool,
}

struct Reactor {
    ep: RawFd,
    event: RawFd,
    sleeping: Arc<AtomicBool>,
    /// By descriptor number (small, dense integers): no hashing on the hot path.
    waits: Vec<Entry>,
    /// Entries with a gate still closed.
    waiting: usize,
}

thread_local! {
    static REACTOR: RefCell<Option<Reactor>> = const { RefCell::new(None) };
}

/// The token of the eventfd among the epoll events (no fd is `u64::MAX`).
const WAKE_TOKEN: u64 = u64::MAX;

impl Reactor {
    fn new() -> Option<Reactor> {
        // SAFETY: plain system calls; the results are checked
        unsafe {
            let ep = libc::epoll_create1(libc::EPOLL_CLOEXEC);
            if ep < 0 {
                return None;
            }
            let event = libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK);
            if event < 0 {
                libc::close(ep);
                return None;
            }
            let mut ev = libc::epoll_event { events: libc::EPOLLIN as u32, u64: WAKE_TOKEN };
            if libc::epoll_ctl(ep, libc::EPOLL_CTL_ADD, event, &mut ev) < 0 {
                libc::close(event);
                libc::close(ep);
                return None;
            }
            let sleeping = Arc::new(AtomicBool::new(false));
            WAKERS.lock().unwrap_or_else(|p| p.into_inner()).push((event, Arc::clone(&sleeping)));
            REACTORS.fetch_add(1, Ordering::SeqCst);
            Some(Reactor { ep, event, sleeping, waits: Vec::new(), waiting: 0 })
        }
    }

    /// Makes sure `fd` is watched. A descriptor is added once, for both
    /// directions and edge-triggered: the kernel then reports each time it
    /// *becomes* readable or writable, with no further call per wait. (A
    /// task only waits after a read or write found the socket not ready, so
    /// no edge is missed; an edge nobody waits for is dropped.)
    /// The entry of `fd` (created empty when new).
    fn entry(&mut self, fd: RawFd) -> &mut Entry {
        let i = fd as usize;
        if i >= self.waits.len() {
            self.waits.resize_with(i + 1, Entry::default);
        }
        &mut self.waits[i]
    }

    fn watch(&mut self, fd: RawFd) -> bool {
        if fd < 0 {
            return false;
        }
        let ep = self.ep;
        let e = self.entry(fd);
        if e.added {
            return true;
        }
        let events = (libc::EPOLLIN | libc::EPOLLOUT | libc::EPOLLRDHUP | libc::EPOLLET) as u32;
        let mut ev = libc::epoll_event { events, u64: fd as u64 };
        // SAFETY: `ev` is a valid event; a bad descriptor is an error, not undefined
        let mut r = unsafe { libc::epoll_ctl(ep, libc::EPOLL_CTL_ADD, fd, &mut ev) };
        if r < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
            r = unsafe { libc::epoll_ctl(ep, libc::EPOLL_CTL_MOD, fd, &mut ev) };
        }
        e.added = r == 0;
        e.added
    }

    fn register(&mut self, fd: RawFd, write: bool) -> Option<Gate> {
        if !self.watch(fd) {
            // cannot be watched (not a socket…): the caller reports it
            return None;
        }
        let e = self.waits.get_mut(fd as usize)?;
        if !write && std::mem::take(&mut e.ready_read) {
            // something arrived meanwhile: try again right away
            return Some(Gate::new(std::sync::atomic::AtomicBool::new(true)));
        }
        let slot = if write { &mut e.write } else { &mut e.read };
        if let Some(g) = slot {
            return Some(Gate::clone(g));
        }
        let gate = Gate::default();
        *slot = Some(Gate::clone(&gate));
        self.waiting += 1;
        super::sched::local_gate(&gate);
        Some(gate)
    }

    /// The descriptor is being closed (the kernel drops it from the epoll
    /// instance by itself): whoever still waited for it is let go.
    ///
    /// Descriptor numbers are reused by the kernel: the entry is emptied and
    /// the registration removed explicitly (the kernel drops it by itself only
    /// when no duplicate of the descriptor is left open), so events of the old
    /// socket cannot be taken for a new one with the same number. An event
    /// already read in the same batch can still arrive for the new socket; it
    /// only makes a waiting operation try again (every operation retries a
    /// non-blocking call and waits anew when it finds nothing), never lose or
    /// invent data.
    fn forget(&mut self, fd: RawFd) {
        if let Some(e) = self.waits.get_mut(fd as usize).map(std::mem::take) {
            if e.added {
                // SAFETY: removing a descriptor from our own epoll instance
                unsafe { libc::epoll_ctl(self.ep, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut()) };
            }
            for g in [e.read, e.write].into_iter().flatten() {
                g.store(true, Ordering::Release);
                super::sched::gate_opened(g);
                self.waiting -= 1;
            }
        }
    }

    /// Waits up to `timeout` (`None`: until something happens) and opens the
    /// gates of the sockets that became ready. Returns whether any did.
    fn poll(&mut self, timeout: Option<Duration>) -> bool {
        let mut events = [libc::epoll_event { events: 0, u64: 0 }; 64];
        let ms = match timeout {
            None => -1,
            // rounded up: a 300 µs deadline must not become a busy loop of 0 ms waits
            Some(d) => d.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32,
        };
        // SAFETY: the buffer holds 64 events
        let n = unsafe { libc::epoll_wait(self.ep, events.as_mut_ptr(), events.len() as i32, ms) };
        let mut opened = false;
        for ev in events.iter().take(n.max(0) as usize) {
            let (token, bits) = (ev.u64, ev.events);
            if token == WAKE_TOKEN {
                let mut count = 0u64;
                // SAFETY: reads the 8-byte counter of our own eventfd
                unsafe { libc::read(self.event, &mut count as *mut u64 as *mut libc::c_void, 8) };
                continue;
            }
            let fd = token as RawFd;
            let Some(e) = self.waits.get_mut(fd as usize) else { continue };
            // an error or a hang-up is news for both directions: the retry reports it
            let broken = bits & (libc::EPOLLERR | libc::EPOLLHUP) as u32 != 0;
            if broken || bits & (libc::EPOLLIN | libc::EPOLLRDHUP) as u32 != 0 {
                match e.read.take() {
                    Some(g) => {
                        g.store(true, Ordering::Release);
                        super::sched::gate_opened(g);
                        self.waiting -= 1;
                        opened = true;
                    }
                    None => e.ready_read = true,
                }
            }
            if broken || bits & libc::EPOLLOUT as u32 != 0 {
                if let Some(g) = e.write.take() {
                    g.store(true, Ordering::Release);
                    super::sched::gate_opened(g);
                    self.waiting -= 1;
                    opened = true;
                }
            }
        }
        opened
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        WAKERS.lock().unwrap_or_else(|p| p.into_inner()).retain(|(fd, _)| *fd != self.event);
        REACTORS.fetch_sub(1, Ordering::SeqCst);
        // SAFETY: closing descriptors this reactor owns
        unsafe {
            libc::close(self.event);
            libc::close(self.ep);
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Reactor) -> R) -> Option<R> {
    REACTOR.with(|r| {
        let mut r = r.borrow_mut();
        if r.is_none() {
            *r = Reactor::new();
        }
        r.as_mut().map(f)
    })
}

/// A gate that opens when `fd` can be read (or has an error to report).
/// `None`: this descriptor cannot be watched; use another way to wait.
pub fn readable(fd: RawFd) -> Option<Gate> {
    with(|r| r.register(fd, false)).flatten()
}

/// A gate that opens when `fd` can be written.
pub fn writable(fd: RawFd) -> Option<Gate> {
    with(|r| r.register(fd, true)).flatten()
}

/// `fd` is about to be closed: stops watching it. Call it before the close
/// (a new socket may get the same number).
pub fn forget(fd: RawFd) {
    REACTOR.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.forget(fd);
        }
    });
}

/// Starts watching `fd` (if it is not watched yet), so that what arrives
/// from now on is noticed even while no task waits for it.
pub fn watch(fd: RawFd) -> bool {
    with(|r| r.watch(fd)).unwrap_or(false)
}

/// Did `fd` (watched) become readable since it was last found empty? Asking
/// consumes the answer. Lets a reader that knows the socket was drained skip
/// the system call that would only say "nothing yet". (An arrival not read
/// from the kernel yet answers `false`: the reader then waits, and the next
/// look at the events lets it go — nothing is lost.)
pub fn take_readable(fd: RawFd) -> bool {
    REACTOR.with(|r| {
        let mut r = r.borrow_mut();
        let Some(r) = r.as_mut() else { return true };
        match r.waits.get_mut(fd as usize) {
            Some(e) if e.added => std::mem::take(&mut e.ready_read),
            _ => true,
        }
    })
}

/// Do tasks of this thread wait for sockets? (Then making no progress is
/// waiting, not a deadlock.)
pub fn waiting() -> bool {
    REACTOR.with(|r| r.borrow().as_ref().is_some_and(|r| r.waiting > 0))
}

/// Opens the gates of the sockets that are ready, without waiting.
pub fn poll_now() -> bool {
    REACTOR.with(|r| match r.borrow_mut().as_mut() {
        Some(r) if r.waiting > 0 => r.poll(Some(Duration::ZERO)),
        _ => false,
    })
}

/// Sleeps until a socket is ready, [`notify`] is called or `timeout` passes.
/// `changed` is checked after this thread is marked asleep, so a wake-up
/// between the caller's last look and the sleep is not lost. Returns `false`
/// when this thread has no reactor (the caller sleeps its own way).
pub fn sleep(timeout: Option<Duration>, changed: impl Fn() -> bool) -> bool {
    REACTOR.with(|r| {
        let mut r = r.borrow_mut();
        let Some(r) = r.as_mut() else { return false };
        r.sleeping.store(true, Ordering::SeqCst);
        if !changed() {
            r.poll(timeout);
        }
        r.sleeping.store(false, Ordering::SeqCst);
        true
    })
}

/// Wakes the threads asleep in [`sleep`] (called by `sched::wake`, from any thread).
pub fn notify() {
    if REACTORS.load(Ordering::SeqCst) == 0 {
        return;
    }
    for (fd, sleeping) in WAKERS.lock().unwrap_or_else(|p| p.into_inner()).iter() {
        if sleeping.load(Ordering::SeqCst) {
            let one = 1u64;
            // SAFETY: writes the 8-byte increment an eventfd expects
            unsafe { libc::write(*fd, &one as *const u64 as *const libc::c_void, 8) };
        }
    }
}

// ---------------------------------------------------------------------
// Non-blocking system calls on a descriptor
// ---------------------------------------------------------------------

fn result(n: isize) -> std::io::Result<usize> {
    if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
}

/// `recv` without waiting (`WouldBlock` when there is nothing yet; 0 at the
/// end of a stream).
pub fn recv(fd: RawFd, buf: &mut [u8]) -> std::io::Result<usize> {
    // SAFETY: `buf` is valid for `buf.len()` bytes
    result(unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), libc::MSG_DONTWAIT) })
}

/// `send` without waiting (`MSG_NOSIGNAL`: a closed peer is an error, not a signal).
pub fn send(fd: RawFd, data: &[u8]) -> std::io::Result<usize> {
    // SAFETY: `data` is valid for `data.len()` bytes
    result(unsafe { libc::send(fd, data.as_ptr() as *const libc::c_void, data.len(), libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL) })
}

/// `recvfrom` without waiting: the datagram and who sent it.
pub fn recv_from(fd: RawFd, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
    // SAFETY: the address buffer is a zeroed sockaddr_storage of the size passed
    unsafe {
        let mut addr: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let n = result(libc::recvfrom(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            libc::MSG_DONTWAIT,
            &mut addr as *mut _ as *mut libc::sockaddr,
            &mut len,
        ))?;
        let from = match addr.ss_family as i32 {
            libc::AF_INET => {
                let a = &*(&addr as *const _ as *const libc::sockaddr_in);
                SocketAddr::from((std::net::Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)), u16::from_be(a.sin_port)))
            }
            libc::AF_INET6 => {
                let a = &*(&addr as *const _ as *const libc::sockaddr_in6);
                SocketAddr::V6(std::net::SocketAddrV6::new(
                    std::net::Ipv6Addr::from(a.sin6_addr.s6_addr),
                    u16::from_be(a.sin6_port),
                    a.sin6_flowinfo,
                    a.sin6_scope_id,
                ))
            }
            _ => return Err(std::io::Error::other("datagram from an unknown address family")),
        };
        Ok((n, from))
    }
}

/// `sendto` without waiting.
pub fn send_to(fd: RawFd, data: &[u8], to: SocketAddr) -> std::io::Result<usize> {
    // SAFETY: the address structures are initialized for their family and length
    unsafe {
        match to {
            SocketAddr::V4(a) => {
                let mut sa: libc::sockaddr_in = std::mem::zeroed();
                sa.sin_family = libc::AF_INET as libc::sa_family_t;
                sa.sin_port = a.port().to_be();
                sa.sin_addr.s_addr = u32::from(*a.ip()).to_be();
                result(libc::sendto(
                    fd,
                    data.as_ptr() as *const libc::c_void,
                    data.len(),
                    libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                ))
            }
            SocketAddr::V6(a) => {
                let mut sa: libc::sockaddr_in6 = std::mem::zeroed();
                sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sa.sin6_port = a.port().to_be();
                sa.sin6_addr.s6_addr = a.ip().octets();
                sa.sin6_flowinfo = a.flowinfo();
                sa.sin6_scope_id = a.scope_id();
                result(libc::sendto(
                    fd,
                    data.as_ptr() as *const libc::c_void,
                    data.len(),
                    libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                ))
            }
        }
    }
}
