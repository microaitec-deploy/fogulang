//! Cooperative scheduler of tasks (`fg`) based on coroutines with their
//! own stack (corosensei).
//!
//! Fogu values use `Rc`, so they cannot cross OS threads. Tasks are
//! therefore coroutines on the same thread:
//!
//! * `fg f()` creates the coroutine and runs it **immediately** until it
//!   finishes or blocks (receiving from an empty channel, sending to a full one…).
//! * When it blocks, the task is queued and resumed whenever the main task
//!   (or another one) also blocks — see [`wait_until`].
//! * If a full pass over all tasks makes no progress, a deadlock is
//!   reported instead of hanging.
//! * Work running off this thread (I/O on the worker pool or the tokio
//!   runtime, see `native::rt`) counts as pending: the main task sleeps until
//!   it calls [`wake`] (or the next timer) instead of polling.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};

/// (Virtual) stack size of each task. Only the pages a task touches use
/// memory; on 32-bit targets the address space itself is scarce, so the
/// reservation is smaller there (a shallower recursion limit in tasks).
#[cfg(target_pointer_width = "64")]
pub const TASK_STACK: usize = 32 * 1024 * 1024;
#[cfg(not(target_pointer_width = "64"))]
pub const TASK_STACK: usize = 2 * 1024 * 1024;
/// Finished tasks give their stack back for the next `fg`: creating one is
/// an mmap + mprotect + page faults, destroying it an munmap.
const POOL_MAX: usize = 256;
/// What stays resident in a pooled stack (its top: where the next task
/// starts); deeper pages are returned to the system.
#[cfg_attr(not(unix), allow(dead_code))]
const POOL_KEEP: usize = 64 * 1024;
/// Safety margin before the end of the stack.
const STACK_MARGIN: usize = 512 * 1024;

type Task = Coroutine<(), (), Result<(), String>, TaskStack>;

/// Tasks alive on stacks of their own mapping (with a guard page) before the
/// next ones share big mappings. Each guarded stack costs the process two
/// memory mappings and Linux allows 65530 of them (`vm.max_map_count`): a
/// program with tens of thousands of tasks would run out of mappings — for its
/// stacks and for everything else (the heap, threads).
#[cfg(unix)]
const GUARDED_MAX: usize = 8192;

/// The stack of a task.
pub enum TaskStack {
    /// Its own mapping, ending in a guard page.
    Guarded(DefaultStack),
    /// A slice of a mapping shared with other tasks ([`slab`]).
    #[cfg(unix)]
    Slab(slab::SlabStack),
}

// SAFETY: `Guarded` is corosensei's own stack. A `Slab` stack is `TASK_STACK`
// bytes of mapped memory below `base`; it has no guard page, so its overflow
// protection is the stack limit the interpreter and compiled code check on
// every call (`stack_ok`, `STACK_MARGIN` before the end).
unsafe impl corosensei::stack::Stack for TaskStack {
    fn base(&self) -> corosensei::stack::StackPointer {
        match self {
            TaskStack::Guarded(s) => s.base(),
            #[cfg(unix)]
            TaskStack::Slab(s) => s.base(),
        }
    }

    fn limit(&self) -> corosensei::stack::StackPointer {
        match self {
            TaskStack::Guarded(s) => s.limit(),
            #[cfg(unix)]
            TaskStack::Slab(s) => s.limit(),
        }
    }

    #[cfg(windows)]
    fn teb_fields(&self) -> corosensei::stack::StackTebFields {
        match self {
            TaskStack::Guarded(s) => s.teb_fields(),
        }
    }

    #[cfg(windows)]
    fn update_teb_fields(&mut self, stack_limit: usize, guaranteed_stack_bytes: usize) {
        match self {
            TaskStack::Guarded(s) => s.update_teb_fields(stack_limit, guaranteed_stack_bytes),
        }
    }
}

/// Task stacks carved out of big mappings: no system call to create one, and
/// one memory mapping per [`CHUNK`] stacks instead of two per stack, so the
/// number of tasks is bounded by memory, not by `vm.max_map_count`.
#[cfg(unix)]
mod slab {
    use super::TASK_STACK;
    use corosensei::stack::StackPointer;
    use std::cell::RefCell;

    /// Stacks per mapping.
    const CHUNK: usize = 64;

    thread_local! {
        /// Bases (highest addresses) of the free stacks of this thread.
        static FREE: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    pub struct SlabStack {
        base: usize,
    }

    impl SlabStack {
        pub fn base(&self) -> StackPointer {
            StackPointer::new(self.base).expect("a mapped stack")
        }

        pub fn limit(&self) -> StackPointer {
            StackPointer::new(self.base - TASK_STACK).expect("a mapped stack")
        }

        /// Gives the pages below the top `keep` bytes back to the system.
        pub fn trim(&self, keep: usize) {
            // SAFETY: the range is inside this stack's own slice of the mapping.
            unsafe {
                libc::madvise((self.base - TASK_STACK) as *mut libc::c_void, TASK_STACK - keep, libc::MADV_DONTNEED);
            }
        }
    }

    impl Drop for SlabStack {
        fn drop(&mut self) {
            // the mapping stays; the slice is free for the next task
            let _ = FREE.try_with(|f| f.borrow_mut().push(self.base));
        }
    }

    /// Free stacks waiting for a task.
    pub fn free() -> usize {
        FREE.with(|f| f.borrow().len())
    }

    pub fn take() -> Result<SlabStack, String> {
        FREE.with(|f| {
            let mut f = f.borrow_mut();
            if let Some(base) = f.pop() {
                return Ok(SlabStack { base });
            }
            let len = CHUNK * TASK_STACK;
            // SAFETY: a fresh anonymous mapping; only the pages a task touches use memory.
            let p = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                    -1,
                    0,
                )
            };
            if p == libc::MAP_FAILED {
                return Err(format!("could not create the task stack: {}", std::io::Error::last_os_error()));
            }
            // no transparent huge pages (see `take_stack`)
            #[cfg(target_os = "linux")]
            // SAFETY: the mapping just created.
            unsafe {
                libc::madvise(p, len, libc::MADV_NOHUGEPAGE);
            }
            let start = p as usize;
            for i in (1..CHUNK).rev() {
                f.push(start + (i + 1) * TASK_STACK);
            }
            Ok(SlabStack { base: start + TASK_STACK })
        })
    }
}

#[derive(Clone, Copy)]
struct Ctx {
    yielder: *const Yielder<(), ()>,
    stack_limit: usize,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Task>> = const { RefCell::new(VecDeque::new()) };
    static CTX: Cell<Ctx> = const { Cell::new(Ctx { yielder: std::ptr::null(), stack_limit: 0 }) };
    static PROGRESS: Cell<u64> = const { Cell::new(0) };
    /// Pending deadlines of `timeout(d)` channels.
    /// Pending deadlines, ordered and without duplicates: waiters re-add theirs
    /// every time they are checked (each wake-up), so equal ones must collapse.
    static TIMERS: RefCell<std::collections::BTreeSet<Instant>> = const { RefCell::new(std::collections::BTreeSet::new()) };
    static STACKS: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
    /// Guarded stacks in use by tasks of this thread.
    static GUARDED: Cell<usize> = const { Cell::new(0) };
}

/// A task stack: a pooled one, or a new one.
fn take_stack() -> Result<TaskStack, String> {
    #[cfg(unix)]
    if GUARDED.with(|g| g.get()) >= GUARDED_MAX {
        return slab::take().map(TaskStack::Slab);
    }
    GUARDED.with(|g| g.set(g.get() + 1));
    if let Some(s) = STACKS.with(|p| p.borrow_mut().pop()) {
        return Ok(TaskStack::Guarded(s));
    }
    let stack = match DefaultStack::new(TASK_STACK) {
        Ok(s) => s,
        Err(e) => {
            GUARDED.with(|g| g.set(g.get() - 1));
            // out of mappings before the threshold (other code uses them too)
            #[cfg(unix)]
            return slab::take().map(TaskStack::Slab).map_err(|_| format!("could not create the task stack: {}", e));
            #[cfg(not(unix))]
            return Err(format!("could not create the task stack: {}", e));
        }
    };
    // no transparent huge pages: the kernel would otherwise give some stacks a whole 2 MB page
    // on their first fault, and a parked task only needs a few 4 KB pages
    #[cfg(target_os = "linux")]
    unsafe {
        let top = { use corosensei::stack::Stack; stack.base().get() };
        libc::madvise((top - TASK_STACK) as *mut libc::c_void, TASK_STACK, libc::MADV_NOHUGEPAGE);
    }
    Ok(TaskStack::Guarded(stack))
}

/// Gives the stack of a finished task back to the pool.
fn recycle(task: Task) {
    if task.started() && !task.done() {
        return; // still holds frames: never reused
    }
    let stack = match task.into_stack() {
        TaskStack::Guarded(s) => s,
        #[cfg(unix)]
        TaskStack::Slab(s) => {
            // back to its slab (on drop). The first free ones keep their top
            // pages for the next task; the rest give every page back, or a
            // burst of tasks would leave its stacks resident forever (a slab
            // is never unmapped)
            s.trim(if slab::free() < POOL_MAX { POOL_KEEP } else { 0 });
            return;
        }
    };
    GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
    STACKS.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() >= POOL_MAX {
            return; // dropped: unmapped
        }
        // a deep recursion left pages behind: give them back (they read as zeros again)
        #[cfg(target_os = "linux")]
        unsafe {
            let top = { use corosensei::stack::Stack; stack.base().get() };
            libc::madvise((top - TASK_STACK) as *mut libc::c_void, TASK_STACK - POOL_KEEP, libc::MADV_DONTNEED);
        }
        p.push(stack);
    });
}

static STRICT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `--strict`: an error in an `fg` task terminates the process (spec §31).
/// By default the error is reported and the other tasks keep running.
pub fn set_strict(on: bool) {
    STRICT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Registers the deadline of a `timeout(d)` channel: while there are pending
/// deadlines, making no progress is not a deadlock — we wait for the deadline.
pub fn add_timer(deadline: Instant) {
    TIMERS.with(|t| t.borrow_mut().insert(deadline));
}

/// Operations running off the interpreter thread (see [`IoGuard`]).
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
/// Bumped by [`wake`]; the main task sleeps on `WAKE_CV` until it changes.
static WAKE_GEN: AtomicU64 = AtomicU64::new(0);
static WAKE_LOCK: Mutex<()> = Mutex::new(());
static WAKE_CV: Condvar = Condvar::new();
/// How many threads are inside [`sleep_until_woken`]: [`wake`] only takes the
/// lock and notifies when one is, so the common case (the interpreter is busy,
/// e.g. a message per WebSocket frame) costs two atomics, not a lock and a
/// futex call.
static SLEEPERS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// In-flight operations that are *this thread's own work*, seen from the
    /// thread that asked for it (see [`set_served`]).
    static SERVED: Cell<usize> = const { Cell::new(0) };
}

/// An interpreter that runs on behalf of another thread (a `sync.Worker`
/// serving a call) is itself the in-flight operation its caller waits for:
/// its caller's [`IoGuard`] must not make this thread believe something is
/// still coming, or a deadlock of its own tasks would hang both forever
/// instead of being reported. `n` is how many such guards are held for it.
pub fn set_served(n: usize) {
    SERVED.with(|s| s.set(n));
}

/// Wakes the interpreter thread: something it may be waiting for is ready.
/// Callable from any thread.
///
/// No wake-up is lost: the sleeper registers in `SLEEPERS` *before* it reads
/// `WAKE_GEN`, and `wake` bumps `WAKE_GEN` *before* it reads `SLEEPERS` (all
/// `SeqCst`). So either `wake` sees the sleeper and notifies it (under the
/// lock, so after it is waiting), or the sleeper sees the new generation and
/// does not wait.
pub fn wake() {
    WAKE_GEN.fetch_add(1, Ordering::SeqCst);
    if SLEEPERS.load(Ordering::SeqCst) > 0 {
        let _g = WAKE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        WAKE_CV.notify_all();
    }
}

/// Marks an operation running off this thread: while one exists, making no
/// progress is not a deadlock. Dropping it (on any thread) calls [`wake`].
pub struct IoGuard(());

impl IoGuard {
    pub fn new() -> IoGuard {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        IoGuard(())
    }
}

impl Default for IoGuard {
    fn default() -> Self {
        IoGuard::new()
    }
}

impl Drop for IoGuard {
    fn drop(&mut self) {
        // The order matters to the deadlock check of `wait_until`, which reads
        // the wake generation, runs the tasks and then reads `IN_FLIGHT`:
        // the generation moves *before* the operation stops counting as in
        // flight (so a scheduler that polled the tasks too early sees a new
        // generation and looks again instead of reporting a deadlock), and
        // the real wake-up comes *after* (so a scheduler that went to sleep
        // because the operation was still in flight is woken).
        WAKE_GEN.fetch_add(1, Ordering::SeqCst);
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        wake();
    }
}

/// Sleeps until [`wake`] is called after `since` was read, or `deadline`.
fn sleep_until_woken(since: u64, deadline: Option<Instant>) {
    let mut g = WAKE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    SLEEPERS.fetch_add(1, Ordering::SeqCst);
    while WAKE_GEN.load(Ordering::SeqCst) == since {
        match deadline {
            None => g = WAKE_CV.wait(g).unwrap_or_else(|p| p.into_inner()),
            Some(d) => {
                let now = Instant::now();
                if d <= now {
                    break;
                }
                g = WAKE_CV.wait_timeout(g, d - now).unwrap_or_else(|p| p.into_inner()).0;
            }
        }
    }
    SLEEPERS.fetch_sub(1, Ordering::SeqCst);
    drop(g);
}

/// Restores what [`set_switch_hook`]'s capture saw.
pub type Restore = Box<dyn FnOnce()>;

thread_local! {
    /// Saves per-task interpreter state (the current file and line) when a
    /// task gives up the thread, and restores it when it gets it back.
    static SWITCH_HOOK: RefCell<Option<(usize, Rc<dyn Fn() -> Restore>)>> = const { RefCell::new(None) };
}

/// Installs the hook (`owner` identifies who installed it).
pub fn set_switch_hook(owner: usize, hook: Rc<dyn Fn() -> Restore>) {
    SWITCH_HOOK.with(|h| *h.borrow_mut() = Some((owner, hook)));
}

/// Removes the hook if `owner` installed it.
pub fn clear_switch_hook(owner: usize) {
    SWITCH_HOOK.with(|h| {
        let mut h = h.borrow_mut();
        if h.as_ref().is_some_and(|(o, _)| *o == owner) {
            *h = None;
        }
    });
}

fn capture() -> Option<Restore> {
    let hook = SWITCH_HOOK.with(|h| h.borrow().as_ref().map(|(_, f)| Rc::clone(f)));
    hook.map(|f| f())
}

/// An id of the running task (0 for the main program), unique while it runs.
pub fn task_id() -> usize {
    CTX.with(|c| c.get().yielder as usize)
}

/// Is the current code running inside an `fg` task?
pub fn in_task() -> bool {
    !CTX.with(|c| c.get()).yielder.is_null()
}

/// Removes the nearest pending deadline.
/// The earliest pending deadline. Deadlines already passed are all dropped at
/// once (the waiters that still need one re-add it when they are checked).
fn take_next_timer() -> Option<Instant> {
    TIMERS.with(|t| {
        let mut t = t.borrow_mut();
        let first = *t.first()?;
        let now = Instant::now();
        if first <= now {
            *t = t.split_off(&now);
        } else {
            t.pop_first();
        }
        Some(first)
    })
}

#[inline(always)]
fn sp() -> usize {
    let x = 0u8;
    std::hint::black_box(&x) as *const u8 as usize
}

/// Sets the stack limit of the current thread (called when the interpreter's
/// main thread starts, with the size of its stack).
pub fn init_main_stack(size: usize) {
    let limit = sp().saturating_sub(size.saturating_sub(STACK_MARGIN));
    CTX.with(|c| {
        let mut v = c.get();
        v.stack_limit = limit;
        c.set(v);
    });
}

/// Is there enough stack left for one more level of recursion?
#[inline]
pub fn stack_ok() -> bool {
    sp() > CTX.with(|c| c.get().stack_limit)
}

/// The lowest address the stack of the running thread / task may reach
/// (0 if unknown).
pub fn stack_limit() -> usize {
    CTX.with(|c| c.get().stack_limit)
}

/// Bytes of stack still available (0 if unknown/exhausted).
pub fn stack_remaining() -> usize {
    sp().saturating_sub(CTX.with(|c| c.get().stack_limit))
}

/// Records that something progressed (send/receive completed, task finished).
pub fn note_progress() {
    PROGRESS.with(|p| p.set(p.get().wrapping_add(1)));
}

fn progress() -> u64 {
    PROGRESS.with(|p| p.get())
}

/// Creates a task and runs it right away until it blocks/finishes.
pub fn spawn(f: Box<dyn FnOnce() -> Result<(), String>>) -> Result<(), String> {
    let stack = take_stack()?;
    let mut task: Task = Coroutine::with_stack(stack, move |yielder, ()| {
        let limit = sp().saturating_sub(TASK_STACK - STACK_MARGIN);
        let mine = Ctx {
            yielder: yielder as *const _,
            stack_limit: limit,
        };
        CTX.with(|c| c.set(mine));
        f()
    });
    note_progress();
    let finished = step(&mut task);
    if finished {
        recycle(task);
    } else {
        QUEUE.with(|q| q.borrow_mut().push_back(task));
    }
    Ok(())
}

/// Resumes a task once. Returns `true` if it finished.
fn step(task: &mut Task) -> bool {
    let prev = CTX.with(|c| c.get());
    let r = task.resume(());
    CTX.with(|c| c.set(prev));
    match r {
        CoroutineResult::Yield(()) => false,
        CoroutineResult::Return(res) => {
            note_progress();
            if let Err(e) = res {
                eprintln!("error in task (fg): {}", e);
                if STRICT.load(std::sync::atomic::Ordering::Relaxed) {
                    eprintln!("--strict: terminating the process");
                    std::process::exit(1);
                }
            }
            true
        }
    }
}

/// Runs one round over all queued tasks.
fn run_pass() {
    let n = QUEUE.with(|q| q.borrow().len());
    for _ in 0..n {
        let task = QUEUE.with(|q| q.borrow_mut().pop_front());
        let Some(mut task) = task else { break };
        if step(&mut task) {
            recycle(task);
        } else {
            QUEUE.with(|q| q.borrow_mut().push_back(task));
        }
    }
}

/// Blocks the current task until `ready()` is true, yielding control to
/// the other tasks. Returns an error on deadlock.
pub fn wait_until(mut ready: impl FnMut() -> bool) -> Result<(), String> {
    loop {
        if ready() {
            return Ok(());
        }
        let ctx = CTX.with(|c| c.get());
        if !ctx.yielder.is_null() {
            // Inside a task: yield to the resumer.
            let state = capture();
            unsafe { (*ctx.yielder).suspend(()) };
            CTX.with(|c| c.set(ctx));
            if let Some(restore) = state {
                restore();
            }
            continue;
        }
        // Main task: run the others.
        let woken = WAKE_GEN.load(Ordering::SeqCst);
        let before = progress();
        let state = capture();
        run_pass();
        if let Some(restore) = state {
            restore();
        }
        if ready() {
            return Ok(());
        }
        if progress() == before {
            // operations of this thread's own caller do not count (see `set_served`)
            let busy = IN_FLIGHT.load(Ordering::SeqCst) > SERVED.with(|s| s.get());
            if let Some(deadline) = take_next_timer() {
                // a completion (wake) ends the sleep early; the deadline
                // stays pending until it passes
                sleep_until_woken(woken, Some(deadline));
                if Instant::now() < deadline {
                    add_timer(deadline);
                }
                continue;
            }
            if busy {
                sleep_until_woken(woken, None);
                continue;
            }
            // Work off this thread that finished *during* the pass above is no
            // longer in flight, and the task waiting for it was polled before
            // its result arrived: nothing progressed, yet it is not a
            // deadlock. Every completion bumps the wake generation, so a new
            // generation means "look again".
            if WAKE_GEN.load(Ordering::SeqCst) != woken {
                continue;
            }
            return Err(
                "deadlock: all tasks are blocked waiting on channels".into(),
            );
        }
    }
}

/// Ends the main program: discards pending tasks (like Go).
pub fn shutdown() {
    QUEUE.with(|q| {
        // We do not drop suspended coroutines (that would require unwinding
        // their stacks); the process is about to exit anyway.
        for t in q.borrow_mut().drain(..) {
            std::mem::forget(t);
        }
    });
}

/// Number of tasks still queued (diagnostics/tests).
#[allow(dead_code)]
pub fn pending_tasks() -> usize {
    QUEUE.with(|q| q.borrow().len())
}
