use serde::{Deserialize, Serialize};

/// Permission scope patterns (dot-segmented wildcards supported).
/// Each field holds a list of patterns:
///   - `["*"]`              → full access (any value)
///   - `["chat.*"]`         → single-segment wildcard (e.g. `chat.general`)
///   - `["chat.*.events"]`  → mid-segment wildcard (e.g. `chat.room1.events`)
///   - `["chat.**"]`        → multi-segment wildcard (e.g. `chat.a.b.c`)
///   - `["room1","room2"]`  → exact names only
///   - `[]`                 → no access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyPerms {
    pub channels: Vec<String>,
    pub kv: Vec<String>,
    pub topics: Vec<String>,
    pub rpc: Vec<String>,
}

impl ApiKeyPerms {
    pub fn root() -> Self {
        Self {
            channels: vec!["*".into()],
            kv: vec!["*".into()],
            topics: vec!["*".into()],
            rpc: vec!["*".into()],
        }
    }
}

/// Dot-segmented wildcard matching.
///
/// Rules (evaluated in order):
///   - `*`           → matches everything (bare global wildcard)
///   - `**`          → matches everything (same as `*`)
///   - `chat.**`     → matches `chat` plus any suffix of segments
///   - `chat.*.test` → each `*` matches exactly one segment (no dots)
///   - `exact.name`  → literal equality
///
/// Examples:
/// ```
/// assert!(matches_pattern("chat.*.test", "chat.room1.test"));
/// assert!(!matches_pattern("chat.*.test", "chat.a.b.test"));
/// assert!(matches_pattern("chat.**",      "chat.a.b.c"));
/// assert!(matches_pattern("*",            "anything"));
/// ```
pub fn matches_pattern(pattern: &str, value: &str) -> bool {
    // Fast paths
    if pattern == "*" || pattern == "**" {
        return true;
    }

    let p_segs: Vec<&str> = pattern.split('.').collect();
    let v_segs: Vec<&str> = value.split('.').collect();

    wildcard_match(&p_segs, &v_segs)
}

/// Recursive segment-level wildcard matcher.
/// `*`  matches exactly one segment.
/// `**` matches zero or more segments (must be last segment in pattern).
fn wildcard_match(pattern: &[&str], value: &[&str]) -> bool {
    match (pattern.first(), value.first()) {
        // Both exhausted → full match
        (None, None) => true,

        // `**` at end of pattern → consume all remaining value segments
        (Some(&"**"), _) => true,

        // Pattern remaining but value exhausted (and not **)
        (Some(_), None) => false,

        // Value remaining but pattern exhausted
        (None, Some(_)) => false,

        // `*` matches exactly one segment
        (Some(&"*"), Some(_)) => wildcard_match(&pattern[1..], &value[1..]),

        // Literal segment — must match exactly
        (Some(p), Some(v)) => {
            if p == v {
                wildcard_match(&pattern[1..], &value[1..])
            } else {
                false
            }
        }
    }
}

pub fn has_access(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|p| matches_pattern(p, value))
}