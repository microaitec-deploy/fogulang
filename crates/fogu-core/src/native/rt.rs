//! Shared machinery for native I/O: one tokio runtime for every async
//! client/server, and a pool of reusable threads for blocking work.
//!
//! Fogu code itself stays on the interpreter thread (`Value` is not `Send`);
//! what runs here is plain Rust work. While it runs, the calling `fg` task
//! waits cooperatively ([`offload`], [`block_on`]) and the other tasks keep
//! going; completion wakes the scheduler ([`sched::IoGuard`]) instead of it
//! polling.
//!
//! With no other task to run, [`offload`] and [`block_on`] just do the work
//! inline: a program without `fg` pays nothing.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::runtime::sched::{self, IoGuard};

/// The shared multi-threaded tokio runtime (`sql`, `scylla`, `bullmq`,
/// the `http`/`web` server, …).
// ---------------------------------------------------------------------
// cores (Fogu.cores, --cores, FOGU_CORES)
// ---------------------------------------------------------------------

/// Threads the runtime may use (0: decided from the machine).
static CORES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Logical CPUs of the machine (or of the affinity mask the process has).
pub fn machine_cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The cores the runtime uses: the configured number, else the machine's.
pub fn cores() -> usize {
    match CORES.load(std::sync::atomic::Ordering::SeqCst) {
        0 => machine_cores(),
        n => n,
    }
}

/// Sets how many threads the I/O runtime and the worker pool use. It must
/// happen before they start (the first network call, timer or blocking job).
pub fn set_cores(n: usize) -> Result<(), String> {
    if n == 0 {
        return Err("Fogu.cores: the number of cores must be at least 1".into());
    }
    if STARTED.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Fogu.cores: the runtime has already started; set it at the top of the program, with --cores or FOGU_CORES".into());
    }
    CORES.store(n, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Pins the whole process to the given CPUs (0-based). Threads started later
/// inherit it.
pub fn pin_cores(cpus: &[usize]) -> Result<(), String> {
    if cpus.is_empty() {
        return Err("Fogu.cores: give at least one CPU".into());
    }
    let max = cpus.iter().max().copied().unwrap_or(0);
    #[cfg(target_os = "linux")]
    {
        if max >= libc::CPU_SETSIZE as usize {
            return Err(format!("Fogu.cores: CPU {} is out of range", max));
        }
        let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        for &c in cpus {
            unsafe { libc::CPU_SET(c, &mut set) };
        }
        // every thread of the process (the main one, the runtime's), then new ones inherit it
        let tids: Vec<libc::pid_t> = std::fs::read_dir("/proc/self/task")
            .map(|d| d.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect())
            .unwrap_or_default();
        for tid in if tids.is_empty() { vec![0] } else { tids } {
            let r = unsafe { libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) };
            if r != 0 {
                return Err(format!("Fogu.cores: cannot use CPUs {:?}: {}", cpus, std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn SetProcessAffinityMask(process: isize, mask: usize) -> i32;
        }
        if max >= usize::BITS as usize {
            return Err(format!("Fogu.cores: CPU {} is out of range", max));
        }
        let mask = cpus.iter().fold(0usize, |m, &c| m | (1usize << c));
        if unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) } == 0 {
            return Err(format!("Fogu.cores: cannot use CPUs {:?}: {}", cpus, std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = max;
        Err("Fogu.cores: choosing CPUs is not supported on this system (a number of cores is)".into())
    }
}

/// `--cores` / FOGU_CORES: "4" (a number) or "0,2,3" (CPUs to use).
pub fn configure_cores(spec: &str) -> Result<(), String> {
    let spec = spec.trim();
    if spec.contains(',') || spec.contains('-') {
        let mut cpus = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('-') {
                Some((a, b)) => {
                    let (a, b): (usize, usize) = (a.trim().parse().map_err(|_| bad(spec))?, b.trim().parse().map_err(|_| bad(spec))?);
                    cpus.extend(a..=b);
                }
                None => cpus.push(part.parse().map_err(|_| bad(spec))?),
            }
        }
        pin_cores(&cpus)?;
        return set_cores(cpus.len());
    }
    set_cores(spec.parse().map_err(|_| bad(spec))?)
}

fn bad(spec: &str) -> String {
    format!("cores: expected a number or a list of CPUs like 0,2-3, got '{}'", spec)
}

pub fn tokio() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
        let workers = match CORES.load(std::sync::atomic::Ordering::SeqCst) {
            0 => machine_cores().clamp(2, 8),
            n => n,
        };
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name("fogu-io")
            .enable_all()
            .build()
            .expect("cannot start the I/O runtime")
    })
}

// ---------------------------------------------------------------------
// Blocking pool
// ---------------------------------------------------------------------

type Job = Box<dyn FnOnce() + Send>;

/// Threads outside tokio (the work may block, or `block_on` a runtime).
/// Grows on demand up to `MAX_THREADS`; idle threads exit after a while.
struct Pool {
    queue: Mutex<(VecDeque<Job>, usize /* idle */, usize /* threads */)>,
    cv: Condvar,
}

const MAX_THREADS: usize = 64;
const IDLE_EXIT: Duration = Duration::from_secs(30);

fn pool() -> &'static Arc<Pool> {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(Pool { queue: Mutex::new((VecDeque::new(), 0, 0)), cv: Condvar::new() }))
}

fn pool_spawn(job: Job) {
    let p = pool();
    let mut q = p.queue.lock().unwrap_or_else(|e| e.into_inner());
    q.0.push_back(job);
    if q.1 > 0 {
        p.cv.notify_one();
        return;
    }
    // with Fogu.cores set: at most 4 blocking threads per core
    let max = match CORES.load(std::sync::atomic::Ordering::SeqCst) {
        0 => MAX_THREADS,
        n => (n * 4).clamp(2, MAX_THREADS),
    };
    STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
    if q.2 >= max {
        return; // a busy thread takes it when done
    }
    q.2 += 1;
    drop(q);
    let p = Arc::clone(p);
    let spawned = std::thread::Builder::new().name("fogu-blocking".into()).spawn(move || worker(&p));
    if spawned.is_err() {
        let p = pool();
        let mut q = p.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.2 -= 1;
        // no thread: run what is queued right here
        let jobs: Vec<Job> = q.0.drain(..).collect();
        drop(q);
        jobs.into_iter().for_each(|j| j());
    }
}

fn worker(p: &Pool) {
    let mut q = p.queue.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(job) = q.0.pop_front() {
            drop(q);
            job();
            q = p.queue.lock().unwrap_or_else(|e| e.into_inner());
            continue;
        }
        q.1 += 1;
        let (g, t) = p.cv.wait_timeout(q, IDLE_EXIT).unwrap_or_else(|e| e.into_inner());
        q = g;
        q.1 -= 1;
        if t.timed_out() && q.0.is_empty() {
            q.2 -= 1;
            return;
        }
    }
}

// ---------------------------------------------------------------------
// Pending results
// ---------------------------------------------------------------------

/// The result of work running elsewhere; taken once it is ready.
pub struct Pending<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Pending<T> {
    pub fn try_take(&self) -> Option<T> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Runs blocking `work` on the pool.
pub fn spawn_blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    let slot = Arc::new(Mutex::new(None));
    let out = Arc::clone(&slot);
    let guard = IoGuard::new();
    pool_spawn(Box::new(move || {
        let r = work();
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        drop(guard); // wakes the scheduler, after the result is in place
    }));
    Pending { slot }
}

/// Runs `fut` on the shared runtime.
pub fn spawn<F>(fut: F) -> Pending<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let slot = Arc::new(Mutex::new(None));
    let out = Arc::clone(&slot);
    let guard = IoGuard::new();
    tokio().spawn(async move {
        let r = fut.await;
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        drop(guard);
    });
    Pending { slot }
}

/// Is there another task that could run while this one waits?
pub fn concurrent() -> bool {
    sched::in_task() || sched::pending_tasks() > 0
}

/// Waits cooperatively for `p`.
pub fn wait<T>(p: Pending<T>) -> Result<T, String> {
    let mut out = None;
    sched::wait_until(|| {
        if out.is_none() {
            out = p.try_take();
        }
        out.is_some()
    })?;
    Ok(out.expect("ready"))
}

/// Runs blocking `work` (a network call, a child process…) without stopping
/// the other `fg` tasks. Inline when there are none.
pub fn offload<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    if !concurrent() {
        return Ok(work());
    }
    wait(spawn_blocking(work))
}

/// Runs `fut` on the shared runtime without stopping the other `fg` tasks.
/// With none, it simply blocks on it.
pub fn block_on<F>(fut: F) -> Result<F::Output, String>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    if !concurrent() {
        return Ok(tokio().block_on(fut));
    }
    wait(spawn(fut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_and_runtime() {
        let ps: Vec<_> = (0..20).map(|i| spawn_blocking(move || i * 2)).collect();
        let mut sum = 0;
        for p in ps {
            loop {
                if let Some(v) = p.try_take() {
                    sum += v;
                    break;
                }
                std::thread::yield_now();
            }
        }
        assert_eq!(sum, (0..20).map(|i| i * 2).sum::<i32>());
        assert_eq!(block_on(async { 41 + 1 }).unwrap(), 42);
        assert_eq!(offload(|| "inline").unwrap(), "inline");
    }
}
