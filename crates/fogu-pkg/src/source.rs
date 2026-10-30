//! Package source: git (GitHub, GitLab, any URL), tarball or local folder.

#[derive(Debug, Clone, PartialEq)]
pub enum Reference {
    /// Repository HEAD.
    Default,
    /// Branch, tag or commit.
    Named(String),
    /// Semver range resolved against the tags (`^1.2.0`).
    Semver(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Git { url: String, reference: Reference },
    Tarball { url: String },
    Local { path: String },
}

const HOSTS: &[(&str, &str)] = &[
    ("github", "github.com"),
    ("gitlab", "gitlab.com"),
    ("bitbucket", "bitbucket.org"),
    ("codeberg", "codeberg.org"),
    ("sourcehut", "git.sr.ht"),
];

fn is_shorthand(s: &str) -> bool {
    // user/repo
    let mut parts = s.split('/');
    let (Some(a), Some(b), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ok = |p: &str| {
        !p.is_empty()
            && !p.starts_with('.')
            && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    ok(a) && ok(b)
}

/// Parses `spec` (a dependency value or a `fogu add` argument).
/// Returns the source and the default name.
pub fn parse(spec: &str) -> Result<(Source, String), String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty package source".into());
    }

    // `#ref` or `#semver:^1.0.0` suffix
    let (base, frag) = match spec.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (spec, None),
    };
    let reference = match frag {
        None | Some("") => Reference::Default,
        Some(f) => match f.strip_prefix("semver:") {
            Some(r) => Reference::Semver(r.to_string()),
            None => Reference::Named(f.to_string()),
        },
    };

    // local folders
    if let Some(p) = base.strip_prefix("file:") {
        let p = p.trim_start_matches("//");
        return Ok((Source::Local { path: p.to_string() }, default_name(p)));
    }
    if base.starts_with("./") || base.starts_with("../") || base.starts_with('/') || base == "." {
        return Ok((Source::Local { path: base.to_string() }, default_name(base)));
    }

    // host:user/repo
    if let Some((h, rest)) = base.split_once(':') {
        if let Some((_, domain)) = HOSTS.iter().find(|(k, _)| *k == h) {
            if !is_shorthand(rest) {
                return Err(format!("expected '{}:user/repository', got '{}'", h, spec));
            }
            let url = format!("https://{}/{}.git", domain, rest.trim_end_matches(".git"));
            return Ok((Source::Git { url, reference }, default_name(rest)));
        }
    }

    // user/repo → GitHub
    if is_shorthand(base) {
        let url = format!("https://github.com/{}.git", base.trim_end_matches(".git"));
        return Ok((Source::Git { url, reference }, default_name(base)));
    }

    // URLs
    let (url, forced_git) = match base.strip_prefix("git+") {
        Some(u) => (u.to_string(), true),
        None => (base.to_string(), false),
    };
    let lower = url.to_lowercase();
    let is_scp = url.contains('@') && !url.contains("://") && url.contains(':'); // git@host:u/r.git
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        if forced_git {
            return Err("'git+' is not valid in an archive URL".into());
        }
        return Ok((Source::Tarball { url: url.clone() }, default_name(strip_archive(&url))));
    }
    if forced_git
        || is_scp
        || lower.starts_with("ssh://")
        || lower.starts_with("git://")
        || lower.starts_with("https://")
        || lower.starts_with("http://")
    {
        return Ok((Source::Git { url: url.clone(), reference }, default_name(&url)));
    }

    Err(format!(
        "unrecognized package source: '{}'\n  examples: mcu (a library of the Fogu server), github:user/repo, user/repo#v1.0.0, \
         gitlab:group/repo, https://example.com/repo.git, ./folder",
        spec
    ))
}

fn strip_archive(u: &str) -> &str {
    u.trim_end_matches(".tar.gz").trim_end_matches(".tgz")
}

fn default_name(s: &str) -> String {
    let s = s.trim_end_matches('/').trim_end_matches(".git");
    let last = s.rsplit(['/', ':']).next().unwrap_or(s);
    last.to_string()
}

/// Canonical form written to `fogu.json`.
pub fn canonical(src: &Source) -> String {
    match src {
        Source::Git { url, reference } => {
            let mut base = if url.starts_with("file:") { format!("git+{}", url) } else { url.clone() };
            for (k, d) in HOSTS {
                let pre = format!("https://{}/", d);
                if let Some(rest) = url.strip_prefix(&pre) {
                    if rest.matches('/').count() == 1 {
                        base = format!("{}:{}", k, rest.trim_end_matches(".git"));
                    }
                }
            }
            match reference {
                Reference::Default => base,
                Reference::Named(r) => format!("{}#{}", base, r),
                Reference::Semver(r) => format!("{}#semver:{}", base, r),
            }
        }
        Source::Tarball { url } => url.clone(),
        Source::Local { path } => format!("file:{}", path),
    }
}

/// Splits `[alias@]source[@version]` from `fogu add`.
pub fn split_add_arg(arg: &str) -> Result<(Option<String>, String), String> {
    // `git@host:u/r.git` is not an alias
    let mut alias = None;
    let mut rest = arg.to_string();
    if let Some(i) = rest.find('@') {
        let (l, r) = rest.split_at(i);
        let looks_alias = !l.is_empty()
            && l.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            && (r[1..].contains('/') || r[1..].contains(':'));
        if looks_alias && l != "git" {
            alias = Some(l.to_string());
            rest = r[1..].to_string();
        }
    }
    // `source@^1.2.0` → `source#semver:^1.2.0` (the last '@' after the slash)
    if !rest.contains('#') && !rest.starts_with("git@") {
        if let Some(i) = rest.rfind('@') {
            if i > 0 && rest[..i].contains(['/', ':']) {
                let ver = rest[i + 1..].to_string();
                let base = rest[..i].to_string();
                rest = if crate::semver::looks_like_range(&ver) {
                    format!("{}#semver:{}", base, ver)
                } else {
                    format!("{}#{}", base, ver)
                };
            }
        }
    }
    Ok((alias, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        let (s, n) = parse("github:foo/bar#v1.0.0").unwrap();
        assert_eq!(n, "bar");
        assert_eq!(
            s,
            Source::Git { url: "https://github.com/foo/bar.git".into(), reference: Reference::Named("v1.0.0".into()) }
        );
        let (s, _) = parse("foo/bar").unwrap();
        assert!(matches!(s, Source::Git { .. }));
        assert_eq!(canonical(&s), "github:foo/bar");
        let (s, n) = parse("git@gitlab.com:grp/pkg.git").unwrap();
        assert_eq!(n, "pkg");
        assert!(matches!(s, Source::Git { .. }));
        assert!(matches!(parse("https://x.org/a.tar.gz").unwrap().0, Source::Tarball { .. }));
        assert!(matches!(parse("../lib").unwrap().0, Source::Local { .. }));
        assert!(parse("doesnotexist").is_err());
    }

    #[test]
    fn add_args() {
        assert_eq!(split_add_arg("foo/bar@^1.2.0").unwrap(), (None, "foo/bar#semver:^1.2.0".into()));
        assert_eq!(
            split_add_arg("http@github:foo/bar#main").unwrap(),
            (Some("http".into()), "github:foo/bar#main".into())
        );
        assert_eq!(split_add_arg("git@github.com:foo/bar.git").unwrap(), (None, "git@github.com:foo/bar.git".into()));
    }
}
