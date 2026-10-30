# Fogu vs Bun vs Go vs Python

AMD Ryzen 7 5700U with Radeon Graphics · linux x86_64 · median of 5 runs, in ms (lower is better)

| benchmark | Fogu | Fogu (no JIT) | Bun | Go | Python |
|---|---:|---:|---:|---:|---:|
| startup | 6 | 6 | 4 | 2 | 14 |
| fib | 37 | 2394 | 21 | 13 | 542 |
| loop | 36 | 3349 | 40 | 22 | 1844 |
| mandel | 24 | 1289 | 15 | 6 | 664 |
| strings | 88 | 135 | 54 | 38 | 109 |
| maps | 357 | 345 | 291 | 172 | 297 |
| records | 129 | 133 | 49 | 144 | 230 |
| sort | 33 | 116 | 23 | 39 | 219 |
