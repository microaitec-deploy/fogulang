//! The blocking HTTP client shared by `fogu install` and the runtime.

/// An error with its causes (`... : Connection refused`).
pub fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let c_s = c.to_string();
        if !s.contains(&c_s) {
            s.push_str(": ");
            s.push_str(&c_s);
        }
        cur = c.source();
    }
    s
}

/// The shared blocking HTTP client (also used by `fogu install`): no global
/// timeout, connections are reused.
#[cfg(not(target_family = "wasm"))]
pub fn blocking_client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(None)
            .build()
            .expect("cannot build the HTTP client")
    })
}
