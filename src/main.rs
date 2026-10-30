// Fogu programs allocate many small objects (strings, lists, maps): mimalloc
// is much faster than the system allocator for them (Linux and macOS).
#[cfg(all(not(windows), not(target_family = "wasm")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    #[cfg(feature = "pprof")]
    prof::start();
    fogu::toolkit::run();
}

/// `FOGU_PPROF=1`: samples the program (SIGPROF, ~1 kHz) and prints the
/// hottest functions to stderr when it exits — including through
/// `process::exit`, which runs the atexit handler.
#[cfg(feature = "pprof")]
mod prof {
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    static GUARD: Mutex<Option<pprof::ProfilerGuard<'static>>> = Mutex::new(None);

    pub fn start() {
        if std::env::var_os("FOGU_PPROF").is_none() {
            return;
        }
        let g = pprof::ProfilerGuardBuilder::default()
            .frequency(997)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .expect("pprof");
        *GUARD.lock().unwrap() = Some(g);
        // SAFETY: registering a plain extern "C" function with no arguments
        unsafe { libc::atexit(dump) };
    }

    extern "C" fn dump() {
        let Some(g) = GUARD.lock().unwrap().take() else {
            eprintln!("[pprof] no profiler guard");
            return;
        };
        let report = match g.report().build() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[pprof] report failed: {}", e);
                return;
            }
        };
        let mut self_t: HashMap<String, isize> = HashMap::new();
        let mut incl: HashMap<String, isize> = HashMap::new();
        let mut total = 0isize;
        for (frames, n) in &report.data {
            total += n;
            let names: Vec<String> = frames.frames.iter().flat_map(|f| f.iter().map(|s| s.name())).collect();
            if let Some(first) = names.first() {
                *self_t.entry(first.clone()).or_default() += n;
            }
            let mut seen = HashSet::new();
            for nm in names {
                if seen.insert(nm.clone()) {
                    *incl.entry(nm).or_default() += n;
                }
            }
        }
        let top = |m: &HashMap<String, isize>, title: &str| {
            let mut v: Vec<(&String, &isize)> = m.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            eprintln!("\n== {} (of {} samples) ==", title, total);
            for (nm, c) in v.into_iter().take(45) {
                let short: String = nm.chars().take(120).collect();
                eprintln!("{:5.1}%  {}", 100.0 * *c as f64 / total.max(1) as f64, short);
            }
        };
        top(&self_t, "self");
        top(&incl, "inclusive");
    }
}
