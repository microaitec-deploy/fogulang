// The `fogu` executable is linked at a fixed address on Linux (like Go's
// executables): the dynamic loader then has no relocations to apply at every
// start (about 160 000 of them otherwise: ~3 ms and ~6 MB of resident memory).
// The shared library of `ffi/` is not affected.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FOGU_PIE");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    // FOGU_PIE=1 keeps a position-independent executable (full ASLR of the image)
    if os == "linux" && env == "gnu" && std::env::var_os("FOGU_PIE").is_none() {
        println!("cargo:rustc-link-arg-bins=-no-pie");
    }
}
