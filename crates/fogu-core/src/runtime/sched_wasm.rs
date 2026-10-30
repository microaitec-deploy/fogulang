//! The `fg` scheduler on WebAssembly. Without threads or stack switching a
//! task cannot be suspended, so:
//!
//! * `fg f()` runs the task right away **to completion** (on native builds it
//!   runs until it first blocks);
//! * sends never wait for the receiver (channels buffer as needed, see
//!   `chan_send`), so producer tasks finish and leave their values queued;
//! * waiting on a timer (`time.sleep`, `timeout(d)`) really sleeps;
//! * a wait that nothing can satisfy anymore is reported as a deadlock.

use std::cell::{Cell, RefCell};
use std::time::Instant;

/// Kept for API parity with the native scheduler.
#[allow(dead_code)]
pub const TASK_STACK: usize = 0;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    static PROGRESS: Cell<u64> = const { Cell::new(0) };
    static TIMERS: RefCell<Vec<Instant>> = const { RefCell::new(Vec::new()) };
    static STACK_LIMIT: Cell<usize> = const { Cell::new(0) };
}

static STRICT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_strict(on: bool) {
    STRICT.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn add_timer(deadline: Instant) {
    TIMERS.with(|t| t.borrow_mut().push(deadline));
}

pub fn wake() {}

/// Work "off the thread" runs inline on WebAssembly.
pub struct IoGuard(());

impl IoGuard {
    pub fn new() -> IoGuard {
        IoGuard(())
    }
}

impl Default for IoGuard {
    fn default() -> Self {
        IoGuard::new()
    }
}

pub fn in_task() -> bool {
    DEPTH.with(|d| d.get()) > 0
}

#[inline(always)]
fn sp() -> usize {
    let x = 0u8;
    std::hint::black_box(&x) as *const u8 as usize
}

/// The wasm stack size is fixed at link time (`-z stack-size`): keep a margin.
pub fn init_main_stack(size: usize) {
    STACK_LIMIT.with(|l| l.set(sp().saturating_sub(size.saturating_sub(256 * 1024))));
}

#[inline]
pub fn stack_ok() -> bool {
    sp() > STACK_LIMIT.with(|l| l.get())
}

pub fn stack_remaining() -> usize {
    sp().saturating_sub(STACK_LIMIT.with(|l| l.get()))
}

pub fn note_progress() {
    PROGRESS.with(|p| p.set(p.get().wrapping_add(1)));
}

/// Runs the task now, to completion.
pub fn spawn(f: Box<dyn FnOnce() -> Result<(), String>>) -> Result<(), String> {
    note_progress();
    DEPTH.with(|d| d.set(d.get() + 1));
    let r = f();
    DEPTH.with(|d| d.set(d.get() - 1));
    note_progress();
    if let Err(e) = r {
        eprintln!("error in task (fg): {}", e);
        if STRICT.load(std::sync::atomic::Ordering::Relaxed) {
            eprintln!("--strict: terminating the process");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn take_next_timer() -> Option<Instant> {
    TIMERS.with(|t| {
        let mut t = t.borrow_mut();
        let i = (0..t.len()).min_by_key(|&i| t[i])?;
        Some(t.swap_remove(i))
    })
}

/// Waits until `ready()`: only timers can still change anything, so it
/// sleeps until the next deadline, or reports a deadlock.
pub fn wait_until(mut ready: impl FnMut() -> bool) -> Result<(), String> {
    loop {
        if ready() {
            return Ok(());
        }
        match take_next_timer() {
            Some(deadline) => {
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
            }
            None => {
                return Err(if in_task() {
                    "deadlock: on WebAssembly an fg task runs to completion and cannot wait for the rest of the program".into()
                } else {
                    "deadlock: all tasks are blocked waiting on channels".into()
                });
            }
        }
    }
}

pub fn shutdown() {}

/// Restores what a switch hook captured (no task switches on WebAssembly).
pub type Restore = Box<dyn FnOnce()>;

pub fn set_switch_hook(_owner: usize, _hook: std::rc::Rc<dyn Fn() -> Restore>) {}

pub fn clear_switch_hook(_owner: usize) {}

/// The only task: the main program.
pub fn task_id() -> usize {
    0
}

#[allow(dead_code)]
pub fn pending_tasks() -> usize {
    0
}
