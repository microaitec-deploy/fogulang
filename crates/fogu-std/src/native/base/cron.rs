//! `fogu:cron` module — task scheduling.
//!
//! Usage:
//!
//!   - `cron.every(duration, fn)`  — runs every N ms/s/m/h
//!   - `cron.schedule("*/5 * * * *", fn)` — standard cron expression (5 fields)
//!   - `cron.tick()` — runs the ready jobs; returns how many ran
//!
//! Since the runtime is synchronous and `Value` is not `Send`, the scheduler
//! does **not** run in the background on its own: the Fogu program must call
//! `cron.tick()` periodically (e.g. inside `while true { cron.tick(); time.delay(100ms) }`).
//!
//! A ready-made loop:
//!
//! ```fogu
//! import cron from "fogu:cron"
//! import time from "fogu:time"
//!
//! cron.every(2s, fn() { print("ping") })
//!
//! while true {
//!     cron.tick()
//!     time.delay(100ms)
//! }
//! ```

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};

// ---------- Global registry (one instance per process) ----------

thread_local! {
    static JOBS: RefCell<Vec<Job>> = RefCell::new(Vec::new());
    static JOBS_PTR: RefCell<Option<Rc<RefCell<Vec<Job>>>>> = RefCell::new(None);
}

pub struct Job {
    pub kind: JobKind,
    pub func: Value,           // Value::Fn or Value::NativeFn
    pub next_run_ms: i64,
    pub name: String,
}

pub enum JobKind {
    /// Simple repetition: fixed interval in ms.
    Every { interval_ms: i64 },
    /// 5-field cron expression.
    Schedule { fields: CronFields },
}

/// A parsed 5-field expression: one bit per allowed value of each field.
#[derive(Clone, Copy)]
pub struct CronFields {
    minute: u64,
    hour: u64,
    day: u64,
    month: u64,
    weekday: u64,
    /// The day-of-month / day-of-week field starts with `*`: as in Vixie cron, when both are
    /// restricted a day matches if EITHER does; when one is `*`, both must.
    day_star: bool,
    weekday_star: bool,
}

// ---------- Module ----------

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    exports.insert(
        "every".into(),
        nfn("every", 2, |args| {
            let ms = as_ms(&args[0])?;
            let func = args[1].clone();
            ensure_callable(&func)?;
            let now = now_ms();
            let name = format!("every({}ms)", ms);
            JOBS.with(|jobs| {
                jobs.borrow_mut().push(Job {
                    kind: JobKind::Every { interval_ms: ms },
                    func,
                    next_run_ms: now.saturating_add(ms),
                    name,
                })
            });
            Ok(Value::Nil)
        }),
    );

    exports.insert(
        "schedule".into(),
        nfn("schedule", 2, |args| {
            let expr = match &args[0] {
                Value::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "schedule(expr, fn): expr must be str, got '{}'",
                        other.type_name()
                    ))
                }
            };
            let func = args[1].clone();
            ensure_callable(&func)?;
            let fields = parse_cron(&expr)?;
            let now = now_ms();
            let next = next_run_after(&fields, now).ok_or_else(|| format!("the cron expression '{}' never matches a date", expr))?;
            JOBS.with(|jobs| {
                jobs.borrow_mut().push(Job {
                    kind: JobKind::Schedule { fields },
                    func,
                    next_run_ms: next,
                    name: format!("schedule({})", expr),
                })
            });
            Ok(Value::Nil)
        }),
    );

    // tick() — runs the ready jobs.
    exports.insert(
        "tick".into(),
        nfn("tick", 0, |_args| {
            let now = now_ms();
            let mut executed = 0i64;
            // Collect the ready jobs, temporarily removing them.
            let ready: Vec<Job> = JOBS.with(|jobs| {
                let mut v = jobs.borrow_mut();
                let mut ready = Vec::new();
                v.retain(|job| {
                    if job.next_run_ms <= now {
                        ready.push(Job {
                            kind: clone_kind(&job.kind),
                            func: job.func.clone(),
                            next_run_ms: job.next_run_ms,
                            name: job.name.clone(),
                        });
                        false
                    } else {
                        true
                    }
                });
                ready
            });

            // Run each job. Since we are on the same thread, this is safe.
            for job in ready {
                // Running a Value::Fn requires `call_value`, which lives in
                // the Interpreter and is not reachable from here. The
                // interpreter therefore intercepts `cron.tick()` and runs
                // the ready jobs itself (see `take_ready_jobs`); this native
                // fallback only reschedules them.
                //
                // (Kept so that the native function stays self-contained if
                // it is ever called without the interception.)
                //
                //
                let _ = &job;
                executed += 1;
                // Reschedule.
                let next = match &job.kind {
                    JobKind::Every { interval_ms } => now.saturating_add(*interval_ms),
                    JobKind::Schedule { fields } => next_run_after(fields, now).unwrap_or(i64::MAX),
                };
                JOBS.with(|jobs| {
                    jobs.borrow_mut().push(Job {
                        kind: clone_kind(&job.kind),
                        func: job.func.clone(),
                        next_run_ms: next,
                        name: job.name.clone(),
                    })
                });
            }

            Ok(Value::Int(executed))
        }),
    );

    exports.insert(
        "list".into(),
        nfn("list", 0, |_args| {
            let names: Vec<Value> = JOBS.with(|jobs| {
                jobs.borrow()
                    .iter()
                    .map(|j| Value::Str(j.name.clone().into()))
                    .collect()
            });
            Ok(Value::List(Rc::new(RefCell::new(names))))
        }),
    );

    exports.insert(
        "clear".into(),
        nfn("clear", 0, |_args| {
            JOBS.with(|jobs| jobs.borrow_mut().clear());
            Ok(Value::Nil)
        }),
    );

    ModuleValue {
        name: "fogu:cron".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Internal API for the Interpreter ----------

/// Returns (in order) the jobs that are ready right now.
/// Reschedules them immediately for the next cycle.
///
/// The `Interpreter` calls this when it intercepts `cron.tick()`, so that it
/// can run the Fogu functions itself (natives cannot call Fogu code).
///
pub fn take_ready_jobs() -> Vec<Value> {
    let now = now_ms();
    let mut ready_funcs = Vec::new();
    JOBS.with(|jobs| {
        let mut v = jobs.borrow_mut();
        let mut keep = Vec::new();
        for job in v.drain(..) {
            if job.next_run_ms <= now {
                ready_funcs.push(job.func.clone());
                let next = match &job.kind {
                    JobKind::Every { interval_ms } => now.saturating_add(*interval_ms),
                    JobKind::Schedule { fields } => next_run_after(fields, now).unwrap_or(i64::MAX),
                };
                keep.push(Job {
                    kind: job.kind,
                    func: job.func,
                    next_run_ms: next,
                    name: job.name,
                });
            } else {
                keep.push(job);
            }
        }
        *v = keep;
    });
    ready_funcs
}

/// Registers a job from the interpreter (when `cron.every` is called from
/// Fogu, this native function already handles it).
pub fn _unused_marker() {}

// ---------- Helpers ----------

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Parses a duration: accepts `Value::Duration`, `Value::Int` (ms) or a
/// `Value::str` such as "2s", "500ms", "5m", "1h".
fn as_ms(v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) if *n > 0 => Ok(*n),
        Value::Duration(ns) if *ns >= 1_000_000 => Ok(*ns / 1_000_000),
        Value::Str(s) => match parse_duration(s)? {
            n if n >= 1 => Ok(n),
            _ => Err(format!("expected a duration of at least 1ms, got \"{}\"", s)),
        },
        other => Err(format!(
            "expected a duration (2s, int ms or a str like \"2s\"; minimum 1ms), got '{}'",
            other.type_name()
        )),
    }
}

fn parse_duration(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("ms") {
        return num
            .trim()
            .parse::<i64>()
            .map_err(|e| format!("invalid duration '{}': {}", s, e));
    }
    if let Some(num) = s.strip_suffix('s') {
        return num
            .trim()
            .parse::<i64>()
            .map(|n| n * 1000)
            .map_err(|e| format!("invalid duration '{}': {}", s, e));
    }
    if let Some(num) = s.strip_suffix('m') {
        return num
            .trim()
            .parse::<i64>()
            .map(|n| n * 60_000)
            .map_err(|e| format!("invalid duration '{}': {}", s, e));
    }
    if let Some(num) = s.strip_suffix('h') {
        return num
            .trim()
            .parse::<i64>()
            .map(|n| n * 3_600_000)
            .map_err(|e| format!("invalid duration '{}': {}", s, e));
    }
    s.parse::<i64>()
        .map_err(|e| format!("invalid duration '{}': {}", s, e))
}

fn ensure_callable(v: &Value) -> Result<(), String> {
    match v {
        Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) => Ok(()),
        other => Err(format!(
            "expected a function, got '{}'",
            other.type_name()
        )),
    }
}

fn clone_kind(k: &JobKind) -> JobKind {
    match k {
        JobKind::Every { interval_ms } => JobKind::Every {
            interval_ms: *interval_ms,
        },
        JobKind::Schedule { fields } => JobKind::Schedule {
            fields: *fields,
        },
    }
}

// ---------- Cron parser ----------

fn parse_cron(expr: &str) -> Result<CronFields, String> {
    let expr = expr.trim();
    let expanded = match expr.to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        m if m.starts_with('@') => return Err(format!("unknown cron macro '{}' (use @yearly @monthly @weekly @daily @hourly)", expr)),
        _ => expr,
    };
    let parts: Vec<&str> = expanded.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!(
            "a cron expression must have 5 fields (min hour day month weekday), it has {}",
            parts.len()
        ));
    }
    const MONTHS: &[&str] = &["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    const DAYS: &[&str] = &["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    let mut weekday = parse_field(parts[4], 0, 7, DAYS, 0)?;
    // 7 is Sunday too
    if weekday & (1 << 7) != 0 {
        weekday = (weekday & !(1 << 7)) | 1;
    }
    let f = CronFields {
        minute: parse_field(parts[0], 0, 59, &[], 0)?,
        hour: parse_field(parts[1], 0, 23, &[], 0)?,
        day: parse_field(parts[2], 1, 31, &[], 0)?,
        month: parse_field(parts[3], 1, 12, MONTHS, 1)?,
        weekday,
        day_star: parts[2].starts_with('*'),
        weekday_star: parts[4].starts_with('*'),
    };
    // an expression that never fires (`0 0 31 2 *`) is a mistake, not a job that waits forever
    if !f.day_star && f.weekday_star {
        const LONGEST: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let possible = (1..=12).any(|m| f.month & (1 << m) != 0 && (1..=LONGEST[m as usize - 1]).any(|d| f.day & (1 << d) != 0));
        if !possible {
            return Err(format!("the cron expression '{}' never matches a date", expr));
        }
    }
    Ok(f)
}

/// One field as a bit set: a list of `*`, `n`, `a-b` or a name, each with an optional `/step`.
/// `names[i]` stands for the value `i + base`.
fn parse_field(s: &str, min: u32, max: u32, names: &[&str], base: u32) -> Result<u64, String> {
    let value = |t: &str| -> Result<u32, String> {
        let lower = t.to_ascii_lowercase();
        if let Some(i) = names.iter().position(|n| *n == lower) {
            return Ok(i as u32 + base);
        }
        if t.contains(['L', 'W', '#', 'l', 'w', '?']) {
            return Err(format!("'{}' in '{}': L, W, # and ? are not supported", t, s));
        }
        let v: u32 = t.parse().map_err(|_| format!("invalid value '{}' in '{}'", t, s))?;
        if v < min || v > max {
            return Err(format!("value {} out of the range {}-{} in '{}'", v, min, max, s));
        }
        Ok(v)
    };
    let mut bits = 0u64;
    for item in s.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((r, st)) => {
                let st: u32 = st.parse().map_err(|_| format!("invalid step '{}' in '{}'", st, s))?;
                if st == 0 {
                    return Err(format!("invalid step 0 in '{}'", s));
                }
                (r, Some(st))
            }
            None => (item, None),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, b) = (value(a)?, value(b)?);
            if a > b {
                return Err(format!("invalid range '{}' in '{}'", range, s));
            }
            (a, b)
        } else {
            // `5/15`: from 5 to the end, every 15
            let a = value(range)?;
            (a, if step.is_some() { max } else { a })
        };
        let mut v = lo;
        while v <= hi {
            bits |= 1 << v;
            v += step.unwrap_or(1);
        }
    }
    Ok(bits)
}

/// Determines the next instant (in ms, UTC) at which the cron expression should fire, starting
/// from `after_ms` (exclusive). `None` when no date in the next 28 years matches (the
/// weekday/leap-year cycle: such an expression never fires).
fn next_run_after(f: &CronFields, after_ms: i64) -> Option<i64> {
    let start_minute = after_ms.div_euclid(60_000) + 1;
    let first_day = start_minute.div_euclid(1440);
    let has = |mask: u64, v: u32| mask & (1 << v) != 0;
    for day in first_day..first_day + 366 * 28 + 1 {
        let (_, month, mday) = civil_from_days(day);
        let weekday = (day + 4).rem_euclid(7) as u32; // 1970-01-01 = Thursday (4)
        let (d, w) = (has(f.day, mday), has(f.weekday, weekday));
        let day_ok = if !f.day_star && !f.weekday_star { d || w } else { d && w };
        if !has(f.month, month) || !day_ok {
            continue;
        }
        let from = if day == first_day { start_minute.rem_euclid(1440) as u32 } else { 0 };
        for m in from..1440 {
            if has(f.hour, m / 60) && has(f.minute, m % 60) {
                return Some((day * 1440 + m as i64) * 60_000);
            }
        }
    }
    None
}

// ---------- Civil date ----------

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------- CronInfo object (optional, for inspection) ----------

pub struct CronInfo;

impl FoguObject for CronInfo {
    fn name(&self) -> &str {
        "CronInfo"
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "count" => {
                let n = JOBS.with(|j| j.borrow().len() as i64);
                Ok(Value::Int(n))
            }
            other => Err(format!("method '{}' does not exist", other)),
        }
    }
}

// Avoids an unused warning for NativeObjectRef (only used if CronInfo is
// exposed). The marker is kept.
#[allow(dead_code)]
fn _unused(_: NativeObjectRef) {}

#[cfg(test)]
mod tests {
    use super::{next_run_after, parse_cron};

    const DAY: i64 = 86_400_000;

    #[test]
    fn day_of_month_or_day_of_week() {
        // 1970-01-01 was a Thursday: "the 13th or a Friday" fires on Friday the 2nd
        let f = parse_cron("0 0 13 * 5").unwrap();
        assert_eq!(next_run_after(&f, 0), Some(DAY));
        // with `*` in one of them both must match: Friday the 13th is 1970-02-13
        let f = parse_cron("0 0 */1 * 5").unwrap();
        assert_eq!(next_run_after(&f, 0), Some(DAY));
        let f = parse_cron("0 0 13 * *").unwrap();
        assert_eq!(next_run_after(&f, 0), Some(12 * DAY));
    }

    #[test]
    fn syntax_and_validation() {
        let f = parse_cron("1-10/3 * * * *").unwrap();
        assert_eq!(f.minute, (1 << 1) | (1 << 4) | (1 << 7) | (1 << 10));
        assert_eq!(parse_cron("0 0 * * 7").unwrap().weekday, 1);
        assert_eq!(parse_cron("0 0 * * SUN").unwrap().weekday, 1);
        assert_eq!(parse_cron("0 0 * DEC *").unwrap().month, 1 << 12);
        assert!(parse_cron("0 25-30 * * *").is_err());
        assert!(parse_cron("0 0 31 2 *").is_err());
        assert!(parse_cron("0 0 30 2 *").is_err());
        assert!(parse_cron("0 0 29 2 *").is_ok());
        assert!(parse_cron("0 0 L * *").is_err());
        assert!(parse_cron("@reboot").is_err());
        assert!(parse_cron("*/0 * * * *").is_err());
        // 29 February: the next one after 1970 is in 1972
        let f = parse_cron("0 0 29 2 *").unwrap();
        assert_eq!(next_run_after(&f, 0), Some((365 + 365 + 31 + 28) * DAY));
        // the next minute, not the current one
        let f = parse_cron("* * * * *").unwrap();
        assert_eq!(next_run_after(&f, 59_999), Some(60_000));
        assert_eq!(next_run_after(&f, 60_000), Some(120_000));
    }
}
