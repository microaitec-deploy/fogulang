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

#[derive(Clone, Copy)]
pub struct CronFields {
    minute: FieldSpec,
    hour: FieldSpec,
    day: FieldSpec,
    month: FieldSpec,
    weekday: FieldSpec,
}

/// The values a field of a cron expression accepts (bit `v` set: `v`
/// matches), and whether it was written `*` (day of month and day of week
/// then combine with "or", as in standard cron).
#[derive(Clone, Copy)]
struct FieldSpec {
    mask: u64,
    any: bool,
}

impl FieldSpec {
    fn matches(&self, v: u32) -> bool {
        v < 64 && self.mask & (1u64 << v) != 0
    }
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
                    next_run_ms: now + ms,
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
            let next = next_run_after(&fields, now)
                .ok_or_else(|| format!("cron expression '{}' never matches a date", expr))?;
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
                let next = next_after(&job, now);
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
                let next = next_after(&job, now);
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
    let ms = match v {
        Value::Int(n) => *n,
        Value::Duration(ns) => *ns / 1_000_000,
        Value::Str(s) => parse_duration(s)?,
        other => {
            return Err(format!(
                "expected a duration (2s, int ms or a str like \"2s\"; minimum 1ms), got '{}'",
                other.type_name()
            ))
        }
    };
    if ms < 1 {
        return Err(format!("the interval must be at least 1ms, got {}", v.to_display_string()));
    }
    Ok(ms)
}

/// When a job that just ran runs next. An `every` job keeps its cadence (the
/// next run is counted from the previous one, not from when `tick()` came),
/// skipping the runs it missed.
fn next_after(job: &Job, now: i64) -> i64 {
    match &job.kind {
        JobKind::Every { interval_ms } => {
            let iv = *interval_ms;
            let mut next = job.next_run_ms + iv;
            if next <= now {
                next += ((now - next) / iv + 1) * iv;
            }
            next
        }
        // an expression that matched once matches again within years
        JobKind::Schedule { fields } => next_run_after(fields, now).unwrap_or(i64::MAX),
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
    let expr = match expr.trim() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        other => other,
    };
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!(
            "a cron expression must have 5 fields (min hour day month weekday), it has {}",
            parts.len()
        ));
    }
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    let mut weekday = parse_field(parts[4], 0, 7, &DAYS, 0)?;
    // 7 is Sunday too
    if weekday.mask & (1 << 7) != 0 {
        weekday.mask = (weekday.mask & !(1 << 7)) | 1;
    }
    Ok(CronFields {
        minute: parse_field(parts[0], 0, 59, &[], 0)?,
        hour: parse_field(parts[1], 0, 23, &[], 0)?,
        day: parse_field(parts[2], 1, 31, &[], 0)?,
        month: parse_field(parts[3], 1, 12, &MONTHS, 1)?,
        weekday,
    })
}

/// One field: a list of `*`, `n` or `a-b`, each with an optional `/step`
/// (`n/step` is `n-max/step`); names (`jan`, `mon`) where the field has them,
/// `names[i]` being the value `i + first`.
fn parse_field(s: &str, min: u32, max: u32, names: &[&str], first: u32) -> Result<FieldSpec, String> {
    let value = |t: &str| -> Result<u32, String> {
        let t = t.trim();
        if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(t)) {
            return Ok(i as u32 + first);
        }
        if t.contains(['L', 'W', '#', '?']) {
            return Err(format!("'{}': L, W, # and ? are not supported in cron expressions", s));
        }
        let v: u32 = t.parse().map_err(|_| format!("invalid value '{}' in '{}'", t, s))?;
        if v < min || v > max {
            return Err(format!("value {} out of the range {}-{} in '{}'", v, min, max, s));
        }
        Ok(v)
    };
    let mut mask = 0u64;
    let any = s == "*";
    for part in s.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, st)) => {
                let st: u32 = st.trim().parse().map_err(|_| format!("invalid step in '{}'", s))?;
                if st == 0 {
                    return Err(format!("invalid step 0 in '{}'", s));
                }
                (r, Some(st))
            }
            None => (part, None),
        };
        let (lo, hi) = if range.trim() == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, b) = (value(a)?, value(b)?);
            if a > b {
                return Err(format!("invalid range '{}' in '{}'", range, s));
            }
            (a, b)
        } else {
            let a = value(range)?;
            // `5/15`: from 5 to the end, every 15
            (a, if step.is_some() { max } else { a })
        };
        let mut v = lo;
        while v <= hi {
            mask |= 1u64 << v;
            v += step.unwrap_or(1);
        }
    }
    Ok(FieldSpec { mask, any })
}

/// Determines the next instant (in ms) at which the cron expression should
/// fire, starting from `after_ms` (exclusive); `None` if it never does (31
/// February).
fn next_run_after(f: &CronFields, after_ms: i64) -> Option<i64> {
    // Advance minute by minute until one matches.
    let start_minute = after_ms / 60_000 + 1;
    let mut t = start_minute;
    loop {
        let total_minutes = t;
        let total_secs = total_minutes * 60;
        let days = total_secs.div_euclid(86_400);
        let secs_in_day = total_secs.rem_euclid(86_400) as u32;
        let hour = secs_in_day / 3600;
        let minute = (secs_in_day % 3600) / 60;

        let (year, month, day) = civil_from_days(days);
        let weekday = ((days + 4).rem_euclid(7)) as u32; // 1970-01-01 = Thursday (4)

        let _ = year; // the year is not used in the expression

        // standard cron: with both restricted, either the day of the month or
        // the day of the week may match
        let day_ok = if f.day.any || f.weekday.any {
            f.day.matches(day) && f.weekday.matches(weekday)
        } else {
            f.day.matches(day) || f.weekday.matches(weekday)
        };
        if f.month.matches(month) && day_ok && f.hour.matches(hour) && f.minute.matches(minute) {
            return Some(total_minutes * 60_000);
        }
        t += 1;
        // every date repeats within 28 years (29 February within 8)
        if t > start_minute + 60 * 24 * 366 * 9 {
            return None;
        }
    }
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