# Design note: `fogu:cron`

Source: [`src/native/cron.rs`](../../src/native/cron.rs).

## API

```fogu
import cron from "fogu:cron"
import time from "fogu:time"

cron.every(2s, () => print("ping"))                    // fixed interval (duration, int ms or "2s")
cron.schedule("*/5 * * * *", () => print("5 minutes")) // standard 5-field cron expression
print(cron.list())                                      // registered jobs
while true {
    cron.tick()                                         // runs the ready jobs, returns how many ran
    time.delay(100ms)
}
cron.clear()
```

Cron expressions have 5 fields — `minute hour day month weekday` — with `*`,
`*/step`, lists `a,b,c` and ranges `a-b`. The next run is found by advancing
minute by minute (giving up after 4 years, as a guard).

## Why `tick()` instead of a background thread

`Value` is built on `Rc<RefCell<…>>`, so it is **not `Send`**: a Fogu function
cannot be called from another OS thread. The jobs therefore live in a
thread-local registry on the main thread, and the program drives the scheduler
by calling `cron.tick()` periodically. This is deterministic and needs no
synchronization.

A native function cannot call Fogu code (it has no access to the interpreter),
so the interpreter **intercepts** `cron.tick()` in `Interpreter::call_value`:
it takes the ready jobs (`cron::take_ready_jobs`, which also reschedules them)
and calls each function in the global environment.

## Future work

A scheduler that runs on its own (no `tick()`) needs either `Value: Send + Sync`
(`Arc` + `Mutex` everywhere) or an event loop in the runtime — both are part of
the "real concurrency" phase in [CHECKLIST.md](../../CHECKLIST.md).
