# Fogu vs Bun vs Go vs Python

AMD Ryzen 7 5700U with Radeon Graphics · linux x86_64 · median of 5 runs, in ms (lower is better)

| benchmark | Fogu | Fogu (no JIT) | Bun | Go | Python |
|---|---:|---:|---:|---:|---:|
| startup | 10 | 10 | 7 | 3 | 29 |
| fib | 75 | 4890 | 49 | 29 | 1279 |
| loop | 88 | 8355 | 89 | 51 | 4053 |
| mandel | 41 | 2860 | 27 | 13 | 1434 |
| strings | 135 | 286 | 111 | 89 | 231 |
| maps | 308 | 744 | 561 | 323 | 578 |
| records | 318 | 239 | 109 | 332 | 419 |
| sort | 101 | 246 | 44 | 86 | 466 |
