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
            .redirect(redirect_policy())
            .build()
            .expect("cannot build the HTTP client")
    })
}

/// Follows up to 10 redirects, and only to `http`/`https`: a server cannot
/// send the client to `file:`, `gopher:` or any other scheme (the 3xx is
/// returned as it is).
#[cfg(not(target_family = "wasm"))]
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|a| {
        if !matches!(a.url().scheme(), "http" | "https") {
            a.stop()
        } else if a.previous().len() >= 10 {
            a.error("too many redirects")
        } else {
            a.follow()
        }
    })
}
