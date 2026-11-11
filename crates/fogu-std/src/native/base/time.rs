//! `fogu:time` — clocks, waits and dates with time zones (chrono + the IANA
//! database of chrono-tz, built in: no system files needed).

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use ahash::AHashMap as HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::format::StrftimeItems;
use chrono::{DateTime, Datelike, FixedOffset, Months, NaiveDate, NaiveDateTime, NaiveTime, Offset, SecondsFormat, TimeZone, Timelike};
use chrono_tz::Tz;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::math::nfn;
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{max_args, nfn_var, str_arg, str_list, with_typed, Num, TypedNative};

/// Seconds since the first call (`time.monotonic()`).
extern "C" fn monotonic_secs() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// Milliseconds since the Unix epoch (`time.now()`).
extern "C" fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---------- Module ----------

pub fn build() -> ModuleValue {
    let mut exports = HashMap::new();

    // `Date()` (now) · `Date(ms)` · `Date(text)` · `Date({ year, month, day, … })`, plus an optional zone.
    exports.insert("Date".into(), nfn_var("Date", 0, new_date));

    // `monotonic()` — seconds (with fractions) from a fixed point; never goes backwards,
    // unlike the wall clock (`now`, `clock`): use it to measure durations.
    exports.insert(
        "monotonic".into(),
        // typed entry: compiled loops read the clock with no `Value` in between
        with_typed(
            nfn("monotonic", 0, |_| Ok(Value::Float(monotonic_secs()))),
            TypedNative { params: &[], ret: Num::Float, entry: monotonic_secs as *const () as usize, fallible: false },
        ),
    );

    // `now()` — current timestamp in ms, like JS's `Date.now()`.
    exports.insert(
        "now".into(),
        with_typed(
            nfn("now", 0, |_| Ok(Value::Int(now_ms()))),
            TypedNative { params: &[], ret: Num::Int, entry: now_ms as *const () as usize, fallible: false },
        ),
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

    // `zone()` — the default time zone of new dates ("UTC" until `setZone`).
    exports.insert("zone".into(), nfn("zone", 0, |_| Ok(Value::Str(default_zone().name().into()))));

    // `setZone(name)` — the default zone of new dates: an IANA name
    // ("America/Sao_Paulo"), "UTC" or "local" (the system's zone).
    exports.insert(
        "setZone".into(),
        nfn("setZone", 1, |a| {
            let z = zone_arg(&a[0], "time.setZone")?;
            DEFAULT_ZONE.with(|d| d.set(z));
            Ok(Value::Nil)
        }),
    );

    // `localZone()` — the system's time zone ("UTC" if it can't be found).
    exports.insert("localZone".into(), nfn("localZone", 0, |_| Ok(Value::Str(local_zone().name().into()))));

    // `zones()` — every IANA time zone name.
    exports.insert(
        "zones".into(),
        nfn("zones", 0, |_| Ok(str_list(chrono_tz::TZ_VARIANTS.iter().map(|z| z.name().to_string())))),
    );

    // `parse(text, format?, zone?)` — a Date, or an error value.
    exports.insert("parse".into(), nfn_var("parse", 1, |a| {
        max_args(a, 3, "time.parse")?;
        let text = str_arg(a, 0, "time.parse")?;
        let fmt = match a.get(1) {
            None | Some(Value::Nil) => None,
            Some(Value::Str(f)) => Some(f.to_string()),
            Some(other) => return Err(format!("time.parse: the format must be a str, got '{}'", other.type_name())),
        };
        let zone = opt_zone(a.get(2), "time.parse")?;
        parse(&text, fmt.as_deref(), zone).map(|d| d.to_value())
    }));

    // `format(date | ms, format, zone?)` — strftime-style text.
    exports.insert("format".into(), nfn_var("format", 2, |a| {
        max_args(a, 3, "time.format")?;
        let ms = instant_arg(&a[0], "time.format")?;
        let fmt = str_arg(a, 1, "time.format")?;
        let zone = opt_zone(a.get(2), "time.format")?.unwrap_or_else(default_zone);
        DateValue { ms, zone }.format(&fmt).map(|s| Value::Str(s.into()))
    }));

    exports.insert("isLeapYear".into(), nfn("isLeapYear", 1, |a| match &a[0] {
        Value::Int(y) => Ok(Value::Bool(is_leap(*y))),
        other => Err(format!("isLeapYear(year) expects an int, got '{}'", other.type_name())),
    }));

    exports.insert("daysInMonth".into(), nfn("daysInMonth", 2, |a| match (&a[0], &a[1]) {
        (Value::Int(y), Value::Int(m)) if (1..=12).contains(m) => Ok(Value::Int(days_in_month(*y, *m as u32) as i64)),
        _ => Err("daysInMonth(year, month) expects ints, month 1-12".into()),
    }));

    ModuleValue {
        name: "fogu:time".into(),
        exports: RefCell::new(exports),
    }
}

// ---------- Time zones ----------

thread_local! {
    static DEFAULT_ZONE: Cell<Tz> = const { Cell::new(Tz::UTC) };
}

fn default_zone() -> Tz {
    DEFAULT_ZONE.with(Cell::get)
}

/// The system's zone: `TZ`, then what the OS reports.
fn local_zone() -> Tz {
    let from_env = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_string());
    from_env
        .into_iter()
        .chain(iana_time_zone::get_timezone().ok())
        .find_map(|n| n.parse::<Tz>().ok())
        .unwrap_or(Tz::UTC)
}

fn zone_by_name(name: &str) -> Option<Tz> {
    match name {
        "local" | "Local" => Some(local_zone()),
        "UTC" | "utc" | "Z" => Some(Tz::UTC),
        n => n.parse::<Tz>().ok(),
    }
}

/// A zone argument: a name (unknown names are an error value) or a Date (its zone).
fn zone_arg(v: &Value, what: &str) -> Result<Tz, String> {
    match v {
        Value::Str(s) => zone_by_name(s).ok_or_else(|| io_err!("{}: unknown time zone '{}' (an IANA name like 'America/Sao_Paulo', 'UTC' or 'local')", what, s)),
        Value::Object(o) if o.name() == "Date" => o
            .inner()
            .borrow()
            .as_any()
            .downcast_ref::<DateValue>()
            .map(|d| d.zone)
            .ok_or_else(|| format!("{}: expected a zone name", what)),
        other => Err(format!("{}: the zone must be a str, got '{}'", what, other.type_name())),
    }
}

fn opt_zone(v: Option<&Value>, what: &str) -> Result<Option<Tz>, String> {
    match v {
        None | Some(Value::Nil) => Ok(None),
        Some(v) => zone_arg(v, what).map(Some),
    }
}

/// An instant: a Date or milliseconds since the epoch.
fn instant_arg(v: &Value, what: &str) -> Result<i64, String> {
    match v {
        Value::Int(ms) => Ok(*ms),
        Value::Object(o) if o.name() == "Date" => o
            .inner()
            .borrow()
            .as_any()
            .downcast_ref::<DateValue>()
            .map(|d| d.ms)
            .ok_or_else(|| format!("{}: expected a Date", what)),
        other => Err(format!("{} expects a Date or milliseconds, got '{}'", what, other.type_name())),
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The instant of a local date and time in `zone` (the earlier one when the
/// clock is turned back; an error in the hour skipped when it goes forward).
fn from_local(zone: Tz, ndt: NaiveDateTime) -> Result<DateValue, String> {
    match zone.from_local_datetime(&ndt).earliest() {
        Some(dt) => Ok(DateValue { ms: dt.timestamp_millis(), zone }),
        None => Err(io_err!("time: {} does not exist in {} (skipped by a clock change)", ndt, zone.name())),
    }
}

/// `time.parse`: RFC 3339 / ISO 8601, RFC 2822, common layouts, or a strftime format.
fn parse(text: &str, fmt: Option<&str>, zone: Option<Tz>) -> Result<DateValue, String> {
    let text = text.trim();
    let zone_or_default = zone.unwrap_or_else(default_zone);
    let with_offset = |dt: DateTime<FixedOffset>| DateValue { ms: dt.timestamp_millis(), zone: zone_or_default };
    match fmt {
        Some(f) => {
            if let Ok(dt) = DateTime::parse_from_str(text, f) {
                return Ok(with_offset(dt));
            }
            if let Ok(ndt) = NaiveDateTime::parse_from_str(text, f) {
                return from_local(zone_or_default, ndt);
            }
            match NaiveDate::parse_from_str(text, f) {
                Ok(d) => from_local(zone_or_default, d.and_time(NaiveTime::MIN)),
                Err(e) => Err(io_err!("time.parse: '{}' does not match '{}': {}", text, f, e)),
            }
        }
        None => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
                return Ok(with_offset(dt));
            }
            if let Ok(dt) = DateTime::parse_from_rfc2822(text) {
                return Ok(with_offset(dt));
            }
            for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
                if let Ok(ndt) = NaiveDateTime::parse_from_str(text, f) {
                    return from_local(zone_or_default, ndt);
                }
            }
            match NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                Ok(d) => from_local(zone_or_default, d.and_time(NaiveTime::MIN)),
                Err(_) => Err(io_err!("time.parse: unrecognized date '{}' (give a format, e.g. \"%d/%m/%Y\")", text)),
            }
        }
    }
}

/// `time.Date()` · `Date(ms)` · `Date(text)` · `Date({ year, month, day, hour, minute, second, millis, zone })`,
/// each with an optional zone as the last argument.
fn new_date(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "time.Date")?;
    let zone = opt_zone(a.get(1), "time.Date")?;
    let d = match a.first() {
        None | Some(Value::Nil) => DateValue { ms: now_ms(), zone: zone.unwrap_or_else(default_zone) },
        Some(Value::Int(ms)) => DateValue { ms: *ms, zone: zone.unwrap_or_else(default_zone) },
        Some(Value::Str(s)) => match parse(s, None, zone) {
            Ok(d) => d,
            Err(e) => return Err(e),
        },
        Some(Value::Map(m)) => {
            let m = m.borrow();
            let field = |k: &str, def: i64| -> Result<i64, String> {
                match m.get(k) {
                    None | Some(Value::Nil) => Ok(def),
                    Some(Value::Int(n)) => Ok(*n),
                    Some(other) => Err(format!("time.Date: '{}' must be an int, got '{}'", k, other.type_name())),
                }
            };
            let zone = match m.get("zone") {
                None | Some(Value::Nil) => zone.unwrap_or_else(default_zone),
                Some(v) => zone_arg(v, "time.Date")?,
            };
            let (y, mo, d) = (field("year", 1970)?, field("month", 1)?, field("day", 1)?);
            let (h, mi, s, ms) = (field("hour", 0)?, field("minute", 0)?, field("second", 0)?, field("millis", 0)?);
            let date = i32::try_from(y)
                .ok()
                .and_then(|y| NaiveDate::from_ymd_opt(y, u32::try_from(mo).ok()?, u32::try_from(d).ok()?))
                .ok_or_else(|| format!("time.Date: invalid date {}-{}-{}", y, mo, d))?;
            let t = NaiveTime::from_hms_milli_opt(h as u32, mi as u32, s as u32, ms as u32)
                .filter(|_| [h, mi, s, ms].iter().all(|v| *v >= 0))
                .ok_or_else(|| format!("time.Date: invalid time {}:{}:{}.{}", h, mi, s, ms))?;
            match from_local(zone, date.and_time(t)) {
                Ok(d) => d,
                Err(e) => return Err(e),
            }
        }
        Some(other) => return Err(format!("time.Date expects ms, a text or a map, got '{}'", other.type_name())),
    };
    Ok(d.to_value())
}

// ---------- Date object ----------

pub struct DateValue {
    /// Milliseconds since the epoch (1 January 1970 UTC).
    ms: i64,
    /// The zone its fields (`hour`, `day`…) and `format` use.
    zone: Tz,
}

impl DateValue {
    pub fn now() -> Self {
        Self { ms: now_ms(), zone: default_zone() }
    }

    pub fn from_ms(ms: i64) -> Self {
        Self { ms, zone: default_zone() }
    }

    fn to_value(self) -> Value {
        NativeObjectRef::new(self).to_value()
    }

    fn at(&self, ms: i64) -> Value {
        DateValue { ms, zone: self.zone }.to_value()
    }

    /// The date and time in its zone (`None` far outside ±262 000 years).
    fn local(&self) -> Option<DateTime<Tz>> {
        self.zone.timestamp_millis_opt(self.ms).single()
    }

    /// Splits `ms` into fields in the date's zone.
    fn decomposed(&self) -> DateParts {
        match self.local() {
            Some(dt) => DateParts {
                year: dt.year() as i64,
                month: dt.month(),
                day: dt.day(),
                hour: dt.hour(),
                minute: dt.minute(),
                second: dt.second(),
                millis: dt.timestamp_subsec_millis().min(999),
                weekday: dt.weekday().num_days_from_sunday(),
            },
            None => utc_parts(self.ms),
        }
    }

    /// Offset from UTC in minutes.
    fn offset_minutes(&self) -> i64 {
        self.local().map_or(0, |dt| dt.offset().fix().local_minus_utc() as i64 / 60)
    }

    fn iso_string(&self) -> String {
        let p = utc_parts(self.ms);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            p.year, p.month, p.day, p.hour, p.minute, p.second, p.millis
        )
    }

    /// RFC 3339 in the date's zone: `2026-11-04T10:00:00.000-03:00`.
    fn local_iso(&self) -> String {
        match self.local() {
            Some(dt) if self.zone != Tz::UTC => dt.to_rfc3339_opts(SecondsFormat::Millis, false),
            _ => self.iso_string(),
        }
    }

    fn human_string(&self) -> String {
        let p = self.decomposed();
        let wd = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][p.weekday as usize];
        let mo = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(p.month - 1) as usize];
        let zone = match self.local() {
            Some(dt) if self.zone != Tz::UTC => dt.format("%Z").to_string(),
            _ => "UTC".into(),
        };
        format!("{} {} {:02} {:04} {:02}:{:02}:{:02} {}", wd, mo, p.day, p.year, p.hour, p.minute, p.second, zone)
    }

    /// strftime-style formatting (`%Y-%m-%d %H:%M:%S`, `%A`, `%Z`…).
    fn format(&self, fmt: &str) -> Result<String, String> {
        let items = StrftimeItems::new(fmt).parse().map_err(|_| format!("format: invalid format '{}'", fmt))?;
        let dt = self.local().ok_or("format: the date is out of range")?;
        let mut out = String::new();
        write!(out, "{}", dt.format_with_items(items.iter())).map_err(|_| format!("format: cannot format with '{}'", fmt))?;
        Ok(out)
    }

    /// add({ years, months, weeks, days, hours, minutes, seconds, millis }) or add(duration):
    /// calendar units move the local date (the end of the month is kept), the others the instant.
    fn add(&self, v: &Value, sign: i64) -> Result<i64, String> {
        match v {
            Value::Duration(ns) => Ok(self.ms + sign * ns.div_euclid(1_000_000)),
            Value::Int(ms) => Ok(self.ms + sign * ms),
            Value::Map(m) => {
                let m = m.borrow();
                let get = |k: &str| -> Result<i64, String> {
                    match m.get(k) {
                        None | Some(Value::Nil) => Ok(0),
                        Some(Value::Int(n)) => Ok(*n * sign),
                        Some(other) => Err(format!("add: '{}' must be an int, got '{}'", k, other.type_name())),
                    }
                };
                for k in m.keys() {
                    if !["years", "months", "weeks", "days", "hours", "minutes", "seconds", "millis"].contains(&k.as_str()) {
                        return Err(format!("add: unknown unit '{}' (years months weeks days hours minutes seconds millis)", k));
                    }
                }
                let mut ms = self.ms;
                let months = get("years")? * 12 + get("months")?;
                let days = get("weeks")? * 7 + get("days")?;
                if months != 0 || days != 0 {
                    let dt = self.local().ok_or("add: the date is out of range")?.naive_local();
                    let dt = if months >= 0 {
                        dt.checked_add_months(Months::new(months as u32))
                    } else {
                        dt.checked_sub_months(Months::new(months.unsigned_abs() as u32))
                    }
                    .and_then(|d| d.checked_add_signed(chrono::TimeDelta::days(days)))
                    .ok_or("add: the date is out of range")?;
                    ms = from_local(self.zone, dt).or_else(|_| {
                        // a local time skipped by a clock change: keep the same UTC offset
                        let off = self.offset_minutes() * 60_000;
                        Ok::<_, String>(DateValue { ms: dt.and_utc().timestamp_millis() - off, zone: self.zone })
                    })?.ms;
                }
                Ok(ms + get("hours")? * 3_600_000 + get("minutes")? * 60_000 + get("seconds")? * 1000 + get("millis")?)
            }
            other => Err(format!("add expects a map of units, a duration or ms, got '{}'", other.type_name())),
        }
    }

    /// The start of the year, month, week (Monday), day, hour or minute, in the date's zone.
    fn start_of(&self, unit: &str) -> Result<i64, String> {
        let dt = self.local().ok_or("startOf: the date is out of range")?.naive_local();
        let d = dt.date();
        let ndt = match unit {
            "year" => NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap().and_time(NaiveTime::MIN),
            "month" => NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap().and_time(NaiveTime::MIN),
            "week" => (d - chrono::TimeDelta::days(d.weekday().num_days_from_monday() as i64)).and_time(NaiveTime::MIN),
            "day" => d.and_time(NaiveTime::MIN),
            "hour" => d.and_hms_opt(dt.hour(), 0, 0).unwrap(),
            "minute" => d.and_hms_opt(dt.hour(), dt.minute(), 0).unwrap(),
            "second" => d.and_hms_opt(dt.hour(), dt.minute(), dt.second()).unwrap(),
            other => return Err(format!("startOf: unknown unit '{}' (year month week day hour minute second)", other)),
        };
        Ok(from_local(self.zone, ndt).map(|d| d.ms).unwrap_or(self.ms))
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

/// UTC fields by hand (any i64 of milliseconds).
fn utc_parts(ms: i64) -> DateParts {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    DateParts {
        year,
        month,
        day,
        hour: seconds_of_day / 3600,
        minute: (seconds_of_day % 3600) / 60,
        second: seconds_of_day % 60,
        millis,
        weekday: ((days + 4).rem_euclid(7)) as u32, // 1970-01-01 = Thursday
    }
}

fn other_ms(args: &[Value], what: &str) -> Result<i64, String> {
    match args {
        [v] => instant_arg(v, what),
        _ => Err(format!("{}(other) expects 1 argument", what)),
    }
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
            "zone" => Some(Value::Str(self.zone.name().into())),
            "offset" => Some(Value::Int(self.offset_minutes())),
            "dayOfYear" => self.local().map(|d| Value::Int(d.ordinal() as i64)),
            "week" => self.local().map(|d| Value::Int(d.iso_week().week() as i64)),
            "isLeapYear" => Some(Value::Bool(is_leap(p.year))),
            "daysInMonth" => Some(Value::Int(days_in_month(p.year, p.month) as i64)),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let arity = |n: usize| -> Result<(), String> {
            if args.len() == n { Ok(()) } else { Err(format!("{}() expects {} argument(s)", method, n)) }
        };
        match method {
            "getTime" => Ok(Value::Int(self.ms)),
            "getFullYear" => Ok(Value::Int(self.decomposed().year)),
            // JS-style: month 0..11
            "getMonth" => Ok(Value::Int(self.decomposed().month as i64 - 1)),
            "getDate" => Ok(Value::Int(self.decomposed().day as i64)),
            // 0 = Sunday
            "getDay" => Ok(Value::Int(self.decomposed().weekday as i64)),
            "getHours" => Ok(Value::Int(self.decomposed().hour as i64)),
            "getMinutes" => Ok(Value::Int(self.decomposed().minute as i64)),
            "getSeconds" => Ok(Value::Int(self.decomposed().second as i64)),
            "getMilliseconds" => Ok(Value::Int(self.decomposed().millis as i64)),
            "getTimezoneOffset" => Ok(Value::Int(-self.offset_minutes())),
            "toISOString" => Ok(Value::Str(self.iso_string().into())),
            "toString" => Ok(Value::Str(self.human_string().into())),
            // RFC 3339 with the zone's offset
            "iso" | "toLocalISO" => Ok(Value::Str(self.local_iso().into())),
            "format" => {
                arity(1)?;
                let f = str_arg(args, 0, "format")?;
                self.format(&f).map(|s| Value::Str(s.into()))
            }
            // setZone(name): the same instant, seen in another zone (in place)
            "setZone" => {
                arity(1)?;
                self.zone = zone_arg(&args[0], "setZone")?;
                Ok(Value::Nil)
            }
            // in(zone): a copy in another zone
            "in" | "inZone" => {
                arity(1)?;
                Ok(DateValue { ms: self.ms, zone: zone_arg(&args[0], method)? }.to_value())
            }
            "clone" => Ok(self.at(self.ms)),
            // add/subtract: a new Date
            "add" => {
                arity(1)?;
                let ms = self.add(&args[0], 1)?;
                Ok(self.at(ms))
            }
            "sub" | "subtract" => {
                arity(1)?;
                let ms = self.add(&args[0], -1)?;
                Ok(self.at(ms))
            }
            "startOf" => {
                arity(1)?;
                let ms = self.start_of(&str_arg(args, 0, "startOf")?)?;
                Ok(self.at(ms))
            }
            // addMs(ms): moves the date in place
            "addMs" => match args {
                [Value::Int(n)] => {
                    self.ms = self.ms.saturating_add(*n);
                    Ok(Value::Nil)
                }
                [other] => Err(format!("addMs expects int, got '{}'", other.type_name())),
                _ => Err("addMs(ms) expects 1 argument".into()),
            },
            // milliseconds between this and another Date
            "diff" => Ok(Value::Int(self.ms - other_ms(args, "diff")?)),
            "before" => Ok(Value::Bool(self.ms < other_ms(args, "before")?)),
            "after" => Ok(Value::Bool(self.ms > other_ms(args, "after")?)),
            "equals" => Ok(Value::Bool(self.ms == other_ms(args, "equals")?)),
            "sameDay" => {
                let o = DateValue { ms: other_ms(args, "sameDay")?, zone: self.zone }.decomposed();
                let p = self.decomposed();
                Ok(Value::Bool((p.year, p.month, p.day) == (o.year, o.month, o.day)))
            }
            other => Err(format!("method '{}' does not exist on Date", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "getTime", "getFullYear", "getMonth", "getDate", "getDay", "getHours", "getMinutes", "getSeconds",
            "getMilliseconds", "getTimezoneOffset", "toISOString", "toString", "iso", "format", "setZone", "in",
            "clone", "add", "sub", "startOf", "addMs", "diff", "before", "after", "equals", "sameDay",
        ]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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