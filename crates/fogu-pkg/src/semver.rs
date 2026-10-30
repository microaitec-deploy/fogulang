//! Minimal semantic versions (X.Y.Z) and ranges (`^`, `~`, `>=`, `1.x`, `*`…).

use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Version(pub u64, pub u64, pub u64);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `v1.2.3`, `1.2`, `1` → version. Pre-releases (`-rc1`) are rejected.
pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches('v');
    if s.is_empty() || s.contains('-') {
        return None;
    }
    let s = s.split('+').next()?;
    let mut it = s.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    let patch = it.next().map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    if it.next().is_some() {
        return None;
    }
    Some(Version(major, minor, patch))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Conjunction of comparators.
#[derive(Debug, Clone)]
pub struct Range {
    cmps: Vec<(Op, Version)>,
}

impl Range {
    pub fn matches(&self, v: Version) -> bool {
        self.cmps.iter().all(|(op, r)| {
            let o = v.cmp(r);
            match op {
                Op::Eq => o == Ordering::Equal,
                Op::Gt => o == Ordering::Greater,
                Op::Ge => o != Ordering::Less,
                Op::Lt => o == Ordering::Less,
                Op::Le => o != Ordering::Greater,
            }
        })
    }
}

fn partial(s: &str) -> Option<(Version, usize)> {
    // returns the filled-in version and how many numeric parts were given (0..=3)
    let s = s.trim().trim_start_matches('v');
    let mut nums = Vec::new();
    for p in s.split('.') {
        if matches!(p, "x" | "X" | "*" | "") {
            break;
        }
        nums.push(p.parse::<u64>().ok()?);
    }
    if nums.len() > 3 {
        return None;
    }
    let n = nums.len();
    let g = |i: usize| nums.get(i).copied().unwrap_or(0);
    Some((Version(g(0), g(1), g(2)), n))
}

fn bump(v: Version, parts: usize) -> Version {
    match parts {
        0 => Version(u64::MAX, 0, 0),
        1 => Version(v.0 + 1, 0, 0),
        2 => Version(v.0, v.1 + 1, 0),
        _ => Version(v.0, v.1, v.2 + 1),
    }
}

pub fn parse_range(s: &str) -> Option<Range> {
    let s = s.trim();
    if s.is_empty() || s == "*" || s == "latest" || s == "x" {
        return Some(Range { cmps: vec![] });
    }
    let mut cmps = Vec::new();
    // several comparators separated by spaces: ">=1.2.0 <2.0.0"
    let mut toks: Vec<String> = Vec::new();
    for t in s.split_whitespace() {
        // "^ 1.2" is not supported; but ">= 1.2" is
        if matches!(t, "^" | "~" | ">" | ">=" | "<" | "<=" | "=") {
            toks.push(t.to_string());
        } else if let Some(last) = toks.last_mut() {
            if matches!(last.as_str(), "^" | "~" | ">" | ">=" | "<" | "<=" | "=") {
                last.push_str(t);
                continue;
            }
            toks.push(t.to_string());
        } else {
            toks.push(t.to_string());
        }
    }
    for t in toks {
        let (op, rest) = if let Some(r) = t.strip_prefix('^') {
            ('^', r)
        } else if let Some(r) = t.strip_prefix('~') {
            ('~', r)
        } else if let Some(r) = t.strip_prefix(">=") {
            ('g', r)
        } else if let Some(r) = t.strip_prefix("<=") {
            ('l', r)
        } else if let Some(r) = t.strip_prefix('>') {
            ('>', r)
        } else if let Some(r) = t.strip_prefix('<') {
            ('<', r)
        } else if let Some(r) = t.strip_prefix('=') {
            ('=', r)
        } else {
            ('=', t.as_str())
        };
        let (v, n) = partial(rest)?;
        match op {
            '^' => {
                cmps.push((Op::Ge, v));
                let upper = if v.0 > 0 || n <= 1 {
                    Version(v.0 + 1, 0, 0)
                } else if v.1 > 0 || n == 2 {
                    Version(0, v.1 + 1, 0)
                } else {
                    Version(0, 0, v.2 + 1)
                };
                cmps.push((Op::Lt, upper));
            }
            '~' => {
                cmps.push((Op::Ge, v));
                cmps.push((Op::Lt, if n <= 1 { Version(v.0 + 1, 0, 0) } else { Version(v.0, v.1 + 1, 0) }));
            }
            'g' => cmps.push((Op::Ge, v)),
            'l' => cmps.push((Op::Le, v)),
            '>' => cmps.push((Op::Gt, v)),
            '<' => cmps.push((Op::Lt, v)),
            _ => {
                // "1.2.3" exact; "1.2" / "1.x" → range
                if n >= 3 {
                    cmps.push((Op::Eq, v));
                } else {
                    cmps.push((Op::Ge, v));
                    cmps.push((Op::Lt, bump(v, n)));
                }
            }
        }
    }
    Some(Range { cmps })
}

/// Does this text look like a semver range (and not a branch/tag name)?
pub fn looks_like_range(s: &str) -> bool {
    let t = s.trim();
    t == "*"
        || t == "latest"
        || t.starts_with(['^', '~', '>', '<', '='])
        || (t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_digit() || ".xX*- <>=^~".contains(c)))
}

/// Picks the highest version (among `(tag_name, version)`) that satisfies the range.
pub fn best<'a>(range: &Range, tags: &'a [(String, Version)]) -> Option<&'a (String, Version)> {
    tags.iter().filter(|(_, v)| range.matches(*v)).max_by_key(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(r: &str, v: &str) -> bool {
        parse_range(r).unwrap().matches(parse_version(v).unwrap())
    }

    #[test]
    fn ranges() {
        assert!(m("^1.2.0", "1.9.3"));
        assert!(!m("^1.2.0", "2.0.0"));
        assert!(!m("^1.2.0", "1.1.9"));
        assert!(m("^0.2.3", "0.2.9"));
        assert!(!m("^0.2.3", "0.3.0"));
        assert!(m("~1.2.3", "1.2.9"));
        assert!(!m("~1.2.3", "1.3.0"));
        assert!(m("1.x", "1.7.0"));
        assert!(m(">=1.0.0 <2.0.0", "1.5.0"));
        assert!(m("*", "9.9.9"));
        assert!(m("1.2.3", "1.2.3"));
        assert!(!m("1.2.3", "1.2.4"));
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some(Version(1, 2, 0)));
        assert_eq!(parse_version("1.0.0-rc1"), None);
        assert!(looks_like_range("^1.0.0"));
        assert!(!looks_like_range("main"));
    }
}
