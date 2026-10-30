# Design note: `fogu:time`

Source: [`src/native/time.rs`](../../src/native/time.rs).

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
3. **`delay` / `sleep` block the thread** with `std::thread::sleep` (and with it
   every `fg` task); the spec's non-blocking behaviour needs the future
   scheduler. They accept `duration` values (`delay(200ms)`).
4. **UTC only**: no time zones, to avoid platform-specific code or external
   dependencies.
5. **Calendar conversion** uses Howard Hinnant's `civil_from_days` algorithm,
   exact for every year of the Gregorian calendar.
