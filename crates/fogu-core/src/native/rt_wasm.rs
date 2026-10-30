//! `native::rt` on WebAssembly: there are no threads, so "background" work
//! simply runs inline (same API as `rt.rs` for the portable modules).

/// The result of work that already ran.
pub struct Pending<T> {
    slot: std::cell::RefCell<Option<T>>,
}

impl<T> Pending<T> {
    pub fn try_take(&self) -> Option<T> {
        self.slot.borrow_mut().take()
    }
}

/// Runs `work` now.
pub fn spawn_blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Pending<T> {
    Pending { slot: std::cell::RefCell::new(Some(work())) }
}

pub fn concurrent() -> bool {
    false
}

pub fn wait<T>(p: Pending<T>) -> Result<T, String> {
    p.try_take().ok_or_else(|| "rt: no result".to_string())
}

pub fn offload<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    Ok(work())
}

/// WebAssembly runs on one thread: the cores settings are accepted and ignored.
pub fn machine_cores() -> usize {
    1
}
pub fn cores() -> usize {
    1
}
pub fn set_cores(_n: usize) -> Result<(), String> {
    Ok(())
}
pub fn pin_cores(_cpus: &[usize]) -> Result<(), String> {
    Err("Fogu.cores: choosing CPUs is not supported in WebAssembly".into())
}
pub fn configure_cores(_spec: &str) -> Result<(), String> {
    Ok(())
}
