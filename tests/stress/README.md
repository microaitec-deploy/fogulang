# Stress suite

Programs that push one limit each, run on the three engines (interpreter, JIT,
bytecode VM):

| program | what it pushes |
| --- | --- |
| `recursion_deep` | 50 000 nested calls |
| `recursion_infinite` | unbounded recursion: must end with `stack overflow`, never a crash |
| `tasks_many` | 20 000 `fg` tasks alive at once |
| `channel_flood` | one million messages through a 16-slot channel, four producers |
| `collections_big` | a list of 3 million items, a map of 300 000 keys, a text of 20 MB |
| `structs_many` | one million struct instances created, linked and dropped |
| `errors_storm` | half a million errors propagated with `?` through three calls |
| `arithmetic_limits` | integer overflow in a function every engine has compiled |
| `channel_waiters` | 20 000 tasks blocked on one channel at once (senders, receivers, `select`): wake-ups stay linear |
| `closures_local` | 200 000 calls of a handler with local helpers and closures kept in its scope: memory stays flat |

```bash
target/release/fogu tests/stress/run.fg          # all of them
target/release/fogu tests/stress/run.fg tasks    # the ones whose name has "tasks"
```

A program passes when it prints `<name>.out`, fails the way `<name>.err` says
(when there is one) and is never killed by a signal or by the 300 s limit. The
runner prints the wall time and the peak memory of each run (Linux,
`/usr/bin/time`).

## Results (0.8.1, 2026-11-16, Ryzen 7 5700U, WSL2)

Every program passes on the three engines. Peak memory: 10–14 MB for the
programs that only compute, 115 MB for 20 000 parked tasks (5.8 KB each),
190–220 MB for `collections_big`, 45 MB for `structs_many` (the million
instances are freed as each chain of 200 000 is dropped).

Known limit: `recursion_infinite` touches about 1 GB of stack before the
`stack overflow` error (the interpreter runs on a 1 GiB stack and the limit
is the stack itself). The error is clean, but on a machine with less free
memory than that the kernel may stop the process first.

Timing note: a single run is not a measurement. On WSL2 any process — Go and
Python binaries too — is occasionally kept off the CPU for about 3 s (1 run in
100 to 300); compare minimums of several runs.
