// Fogu programs allocate many small objects (strings, lists, maps): mimalloc
// is much faster than the system allocator for them (Linux and macOS).
#[cfg(all(not(windows), not(target_family = "wasm")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    fogu::toolkit::run();
}
