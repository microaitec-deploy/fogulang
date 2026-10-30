//! Libraries published on the Fogu server (`server/publish-libs.sh`):
//! `fogu add mcu` or `fogu add mcu@0.1.0` asks `<server>/api/libs/<name>` for
//! the version and installs `<server>/libs/<name>/<version>.tar.gz`, a tarball
//! source pinned to that version (and to its sha256 by `fogu.lock`).
//!
//! The server is `FOGU_SERVER`, the one the install script saved in
//! `$FOGU_HOME/server`, or the official one (`DEFAULT_SERVER`).

/// The official Fogu server: releases, runtimes and the published libraries.
pub const DEFAULT_SERVER: &str = "https://fogulang.microaitec.com";

/// The Fogu server, without a trailing slash.
pub fn server_url() -> Option<String> {
    let from_env = std::env::var("FOGU_SERVER").ok().filter(|s| !s.trim().is_empty());
    let saved = || std::fs::read_to_string(crate::home().join("server")).ok();
    from_env
        .or_else(saved)
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .or_else(|| Some(DEFAULT_SERVER.to_string()))
}

/// `name` or `name@version` (a bare library name, not a path, URL or repository).
pub fn parse_name(spec: &str) -> Option<(String, Option<String>)> {
    let (name, version) = match spec.split_once('@') {
        Some((n, v)) => (n, Some(v)),
        None => (spec, None),
    };
    let name_ok = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let version_ok = version.is_none_or(|v| {
        !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
    });
    (name_ok && version_ok).then(|| (name.to_string(), version.map(|v| v.trim_start_matches('v').to_string())))
}

/// The archive URL of `name` at `version` (the latest when `None`).
#[cfg(not(target_family = "wasm"))]
pub fn resolve(name: &str, version: Option<&str>) -> Result<(String, String), String> {
    let server = server_url().ok_or_else(|| {
        format!(
            "'{}' is a library name but no Fogu server is configured\n  set FOGU_SERVER=https://… or reinstall Fogu with the install script",
            name
        )
    })?;
    let api = format!("{}/api/libs/{}", server, name);
    let resp = crate::net::blocking_client()
        .get(&api)
        .send()
        .map_err(|e| format!("cannot reach {}: {}", server, crate::net::error_chain(&e)))?;
    if resp.status().as_u16() == 404 {
        return Err(format!("no library '{}' on {} (see {}/libs)", name, server, server));
    }
    let info: serde_json::Value = resp
        .error_for_status()
        .and_then(|r| r.json())
        .map_err(|e| format!("{}: {}", api, crate::net::error_chain(&e)))?;
    let versions: Vec<&str> = info["versions"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v["version"].as_str()).collect())
        .unwrap_or_default();
    let chosen = match version {
        None | Some("latest") => info["latest"].as_str().map(str::to_string),
        Some(v) => versions.iter().find(|x| **x == v).map(|x| x.to_string()),
    }
    .ok_or_else(|| {
        format!("library '{}' has no version {} (available: {})", name, version.unwrap_or("latest"), versions.join(", "))
    })?;
    Ok((format!("{}/libs/{}/{}.tar.gz", server, name, chosen), chosen))
}

#[cfg(target_family = "wasm")]
pub fn resolve(name: &str, _version: Option<&str>) -> Result<(String, String), String> {
    Err(format!("cannot download library '{}' on WebAssembly", name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse_name("mcu"), Some(("mcu".into(), None)));
        assert_eq!(parse_name("data-science@v0.1.0"), Some(("data-science".into(), Some("0.1.0".into()))));
        assert_eq!(parse_name("mcu@latest"), Some(("mcu".into(), Some("latest".into()))));
        for not in ["foo/bar", "github:foo/bar", "./lib", "https://x.org/a.tar.gz", "1abc", "mcu@", "git@github.com:a/b.git"] {
            assert_eq!(parse_name(not), None, "{}", not);
        }
    }
}
