# Design note: `fogu:time`

Source: [`crates/fogu-std/src/native/base/time.rs`](../../crates/fogu-std/src/native/base/time.rs).

## API

```fogu
import time from "fogu:time"

const now = time.Date()                 // current instant (UTC)
print(now.getFullYear(), now.getMonth(), now.getDate(), now.getDay())
print(now.getHours(), now.getMinutes(), now.getSeconds(), now.getMilliseconds())
print(now.getTime(), now.toISOString(), now.toString())
print(now.year, now.month, now.day, now.hour, now.minute, now.second, now.weekday)
const later = time.Date()
later.addMs(1000)                       // moves the date forward, in place
print(later.diff(now))                  // about 1000 (ms)

print(time.now())                       // current timestamp in ms
time.delay(500ms)                       // a duration or an int number of ms
time.sleep(0.25)                        // seconds (a float or a duration)
```

## Notes

1. **`Date()` is a factory function**, not a struct: it returns a native object
   with a JavaScript-like API.
2. **The getters follow JavaScript** where it makes sense: `getMonth()` returns
   **0–11**, `getDay()` returns **0 = Sunday**, `getDate()` returns 1–31. The
   direct field `month` is 1–12.
3. **`delay` / `sleep` are cooperative**: the task waits on a scheduler timer and the
   other `fg` tasks keep running. They accept `duration` values (`delay(200ms)`); a
   negative, NaN or infinite time is an error.
4. **Time zones** come from `chrono-tz` (IANA names): every `Date` has a zone, new dates
   take the default zone (`time.zone()`, "UTC" until `time.setZone`).
5. **Calendar conversion** uses Howard Hinnant's `civil_from_days` algorithm,
   exact for every year of the Gregorian calendar.
