use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};

// ---------- Module ----------

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    // Factory function: `Date()` returns a new Date object with the current
    // instant (equivalent to JS's `new Date()`).
    exports.insert(
        "Date".into(),
        nfn("Date", 0, |_args| {
            let d = DateValue::now();
            Ok(NativeObjectRef::new(d).to_value())
        }),
    );

    // `monotonic()` — seconds (with fractions) from a fixed point; never goes backwards,
    // unlike the wall clock (`now`, `clock`): use it to measure durations.
    exports.insert(
        "monotonic".into(),
        nfn("monotonic", 0, |_| {
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            Ok(Value::Float(START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()))
        }),
    );

    // `now()` — current timestamp in ms, like JS's `Date.now()`.
    exports.insert(
        "now".into(),
        nfn("now", 0, |_| {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as i64;
            Ok(Value::Int(ms))
        }),
    );

    // `delay(d)` — sleeps for a duration (or an int number of milliseconds).
    exports.insert(
        "delay".into(),
        nfn("delay", 1, |args| match &args[0] {
            Value::Int(ms) if *ms >= 0 => {
                crate::native::util::sleep_coop(Duration::from_millis(*ms as u64))?;
                Ok(Value::Nil)
            }
            Value::Duration(ns) if *ns >= 0 => {
                crate::native::util::sleep_coop(Duration::from_nanos(*ns as u64))?;
                Ok(Value::Nil)
            }
            other => Err(format!(
                "delay(ms) expects a duration or int >= 0, got '{}'",
                other.type_name()
            )),
        }),
    );

    // `sleep(fractional_seconds)` — variant in seconds (also accepts a duration).
    exports.insert(
        "sleep".into(),
        nfn("sleep", 1, |args| {
            let secs = match &args[0] {
                Value::Int(n) => *n as f64,
                Value::Float(f) => *f,
                Value::Duration(ns) => *ns as f64 / 1e9,
                other => {
                    return Err(format!(
                        "sleep(s) expects a number or a duration, got '{}'",
                        other.type_name()
                    ))
                }
            };
            if secs < 0.0 {
                return Err("sleep(s) expects seconds >= 0".into());
            }
            crate::native::util::sleep_coop(Duration::from_secs_f64(secs))?;
            Ok(Value::Nil)
        }),
    );

    ModuleValue {
        name: "fogu:time".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Date object ----------

pub struct DateValue {
    /// Milliseconds since the epoch (1 January 1970 UTC).
    ms: i64,
}

impl DateValue {
    pub fn now() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self { ms }
    }

    pub fn from_ms(ms: i64) -> Self {
        Self { ms }
    }

    /// Splits `ms` into a set of UTC fields.
    /// Does not use `chrono` to avoid external dependencies; the conversion
    /// is implemented by hand (civil_from_days algorithm).
    fn decomposed(&self) -> DateParts {
        let secs = self.ms.div_euclid(1000);
        let millis = self.ms.rem_euclid(1000) as u32;

        let days = secs.div_euclid(86_400);
        let seconds_of_day = secs.rem_euclid(86_400) as u32;

        let hour = seconds_of_day / 3600;
        let minute = (seconds_of_day % 3600) / 60;
        let second = seconds_of_day % 60;

        let (year, month, day) = civil_from_days(days);

        let weekday = ((days + 4).rem_euclid(7)) as u32; // 1970-01-01 = Thursday

        DateParts {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millis,
            weekday,
        }
    }

    fn iso_string(&self) -> String {
        let p = self.decomposed();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            p.year, p.month, p.day, p.hour, p.minute, p.second, p.millis
        )
    }

    fn human_string(&self) -> String {
        let p = self.decomposed();
        let wd = [
            "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat",
        ][p.weekday as usize];
        let mo = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun",
            "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ][(p.month - 1) as usize];
        format!(
            "{} {} {:02} {:04} {:02}:{:02}:{:02} UTC",
            wd, mo, p.day, p.year, p.hour, p.minute, p.second
        )
    }
}

struct DateParts {
    year: i64,
    month: u32,   // 1..12
    day: u32,     // 1..31
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    weekday: u32, // 0=Sun .. 6=Sat
}

impl FoguObject for DateValue {
    fn name(&self) -> &str {
        "Date"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let p = self.decomposed();
        match field {
            "ms" | "time" => Some(Value::Int(self.ms)),
            "year" => Some(Value::Int(p.year)),
            "month" => Some(Value::Int(p.month as i64)),
            "day" => Some(Value::Int(p.day as i64)),
            "hour" => Some(Value::Int(p.hour as i64)),
            "minute" => Some(Value::Int(p.minute as i64)),
            "second" => Some(Value::Int(p.second as i64)),
            "millis" => Some(Value::Int(p.millis as i64)),
            "weekday" => Some(Value::Int(p.weekday as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // --- JS: getTime() → ms since the epoch
            "getTime" => Ok(Value::Int(self.ms)),

            // --- JS: getFullYear()
            "getFullYear" => Ok(Value::Int(self.decomposed().year)),

            // --- JS: getMonth() → 0..11 (like JS!)
            "getMonth" => Ok(Value::Int(self.decomposed().month as i64 - 1)),

            // --- JS: getDate() → 1..31
            "getDate" => Ok(Value::Int(self.decomposed().day as i64)),

            // --- JS: getDay() → 0=Sun .. 6=Sat
            "getDay" => Ok(Value::Int(self.decomposed().weekday as i64)),

            "getHours" => Ok(Value::Int(self.decomposed().hour as i64)),
            "getMinutes" => Ok(Value::Int(self.decomposed().minute as i64)),
            "getSeconds" => Ok(Value::Int(self.decomposed().second as i64)),
            "getMilliseconds" => {
                Ok(Value::Int(self.decomposed().millis as i64))
            }

            // --- JS: toISOString()
            "toISOString" => Ok(Value::Str(self.iso_string().into())),

            // --- friendly toString()
            "toString" => Ok(Value::Str(self.human_string().into())),

            // --- add/subtract ms
            "addMs" => {
                if args.len() != 1 {
                    return Err("addMs(ms) expects 1 argument".into());
                }
                match &args[0] {
                    Value::Int(n) => {
                        self.ms = self.ms.saturating_add(*n);
                        Ok(Value::Nil)
                    }
                    other => Err(format!(
                        "addMs expects int, got '{}'",
                        other.type_name()
                    )),
                }
            }

            // --- difference in ms between this and another Date
            "diff" => {
                if args.len() != 1 {
                    return Err("diff(other) expects 1 argument".into());
                }
                match &args[0] {
                    Value::Object(o) => {
                        if o.name() == "Date" {
                            let other_ms = o
                                .call("getTime", &[])
                                .map_err(|e| e.to_string())?;
                            if let Value::Int(other) = other_ms {
                                return Ok(Value::Int(self.ms - other));
                            }
                        }
                        Err("diff expects a Date".into())
                    }
                    other => Err(format!(
                        "diff expects a Date object, got '{}'",
                        other.type_name()
                    )),
                }
            }

            other => Err(format!("method '{}' does not exist on Date", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "getTime",
            "getFullYear",
            "getMonth",
            "getDate",
            "getDay",
            "getHours",
            "getMinutes",
            "getSeconds",
            "getMilliseconds",
            "toISOString",
            "toString",
            "addMs",
            "diff",
        ]
    }
}

// ---------- Civil conversion (Howard Hinnant) ----------

/// Returns (year, month (1-12), day (1-31)) from the number of days since
/// 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}