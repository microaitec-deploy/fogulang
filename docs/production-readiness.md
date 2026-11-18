# Production readiness

Where Fogu stands for production use, and what would change that. This page
states a classification and the evidence behind it; it is updated when the
evidence changes, not when features are added.

## Three different claims

| Claim | What it means |
| --- | --- |
| **Indicative of production** | The architecture, the tests and the results point to viability. |
| **Ready for production** | It meets technical and operational criteria defined for a concrete context. |
| **Recommended for production** | There is enough evidence of stability, security, performance and reliability in real use. |

## Where Fogu is now

**Pre-production, with positive indicators.** Fogu is past the experimental
stage, but there is not yet enough evidence to recommend it for production.
Controlled trials in pre-production environments are justified. Running
production services on it, or recommending that others do, is not justified
yet.

What is not in question is whether Fogu can run programs. What has to be shown
is that it keeps running them correctly and predictably, with consistent
performance, when conditions are no longer ideal: long uptimes, load, failures,
hostile input.

## Evidence that exists

- **Three engines, one language.** The conformance suite (`tests/spec`) runs
  every program on the tree-walking interpreter, the JIT and the bytecode VM and
  requires identical output. The suite includes permanent tests of closures that
  escape their scope, determinism (Mutex and Semaphore order, the text of cyclic
  values and floats) and numeric representation and serialization.
- **Memory.** The leak of scopes kept alive by their own closures (a request
  handler with a local helper) is fixed. In the measurement, 300,000 calls stay
  at 8–11 MB, where 0.8.0 grew to 242 MB.
  [architecture/closure-cycles.md](architecture/closure-cycles.md).
- **Errors.** `return a, b` is a tuple, so `[v, err]` and `?` no longer mistake a
  returned list for a value/error pair ([architecture/tuples.md](architecture/tuples.md)).
- **Security.** The fixes of the two test rounds are in place: the canonical
  request path before routing and middlewares, contained handler panics, limits
  on body, headers, time and JSON depth, CSRF on topcoat, JWT time claims, XSS in
  templates and Markdown, BatBadBut. `cargo audit` has no open vulnerability. One
  advisory without an available fix is accepted, with its exposure and closing
  condition in `.cargo/audit.toml`.
- **Concurrency.** Tasks blocked on channels, `select` and timers park behind
  gates instead of being polled. `fg` errors are shown. Task identities are never
  reused.
- **Stress.** `tests/stress` pushes one limit per program (call depth, 20,000
  live tasks, channel floods, large collections, allocation churn, error
  volume, closure memory) on the three engines.
- **Performance discipline.** `bench/compare.fg` compares two binaries the way
  every performance claim of the project is made: a quiet machine, interleaved
  runs, checked outputs, median, p95 and spread.

## Evidence still missing

| # | Evidence | How it is produced | Status |
| --- | --- | --- | --- |
| 1 | The consolidation complete and every test green | `cargo test --release`, `tests/stress/run.fg` | green on Linux (WSL2); not yet released |
| 2 | 72 hours of stability on a second machine | the soak test kept with the test rounds (`test/soak72`): load, memory, descriptors, latency, data integrity | script ready and checked; not run |
| 3 | Behaviour under prolonged load | the same soak test, and `bench/run_net.fg` | not run |
| 4 | Security risks closed or documented | `cargo audit`, the security tests of the rounds, the accepted advisory | documented; the HTML policy for text responses is a pending decision |
| 5 | Performance against the previous release | `bench/compare.fg` on a quiet machine | not run |
| 6 | Real use and recovery from failures | trials in pre-production: restarts, killed processes, full disks, lost connections | not started |

Known gaps that weigh on this classification are listed in the README
("Status and limitations"). The two largest are plain data cycles, which are
never freed because there is no cycle collector, and the Windows-specific fixes,
which have not yet been run on Windows.

## What changes the classification

- **Ready for production, in a stated context** (for example, an internal
  service on Linux): items 1 to 5 done, with their results recorded here, and
  no open finding of the soak test.
- **Recommended for production:** the above, plus item 6 over a period of real
  use, with what failed and how it was fixed recorded.
