# Programs: Fogu (tree-walker, VM) vs Bun vs Go vs Python

AMD Ryzen 7 5700U with Radeon Graphics · linux x86_64 · median of 5 runs, in ms (lower is better)

| benchmark | Fogu | Fogu (no JIT) | Fogu (VM) | Bun | Go | Python |
|---|---:|---:|---:|---:|---:|---:|
| parser | 4248 | 4271 | 2057 | — | 159 | 1301 |
| orders | 4564 | 4570 | 1395 | — | 21 | 1225 |
| wordfreq | 1582 | 1549 | 827 | — | 140 | 673 |
