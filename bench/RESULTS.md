# Fogu vs Bun vs Go vs Python

AMD Ryzen 7 5700U with Radeon Graphics · linux x86_64 · median of 5 runs, in ms (lower is better)

| benchmark | Fogu | Fogu (no JIT) | Bun | Go | Python |
|---|---:|---:|---:|---:|---:|
| startup | 8 | 8 | 6 | 3 | 18 |
| fib | 42 | 2411 | 22 | 14 | 579 |
| loop | 40 | 3367 | 42 | 22 | 1909 |
| mandel | 48 | 1241 | 16 | 8 | 661 |
| strings | 86 | 150 | 69 | 46 | 131 |
| maps | 229 | 356 | 298 | 204 | 298 |
| records | 132 | 134 | 56 | 153 | 252 |
| sort | 35 | 116 | 21 | 38 | 219 |
