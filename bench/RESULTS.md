# Fogu vs Bun vs Go vs Python

AMD Ryzen 7 5700U with Radeon Graphics · linux x86_64 · median of 5 runs, in ms (lower is better)

| benchmark | Fogu | Fogu (no JIT) | Bun | Go | Python |
|---|---:|---:|---:|---:|---:|
| startup | 1 | 1 | 5 | 3 | 14 |
| fib | 13 | 1712 | 21 | 15 | 550 |
| loop | 26 | 2963 | 40 | 22 | 1787 |
| mandel | 7 | 1011 | 13 | 8 | 631 |
| strings | 35 | 100 | 58 | 38 | 104 |
| maps | 159 | 268 | 280 | 185 | 263 |
| records | 74 | 98 | 48 | 143 | 226 |
| sort | 23 | 91 | 21 | 36 | 207 |
