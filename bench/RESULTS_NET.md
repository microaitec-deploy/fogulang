# Fogu networking: servers and tasks

AMD Ryzen 7 5700U with Radeon Graphics · 16 cores · median of 10 rounds · fogu 0.7.5

## HTTP

`GET /hello` (a small JSON body), 64 keep-alive connections, 5 s.

| | requests/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Fogu | 48797 | 0.89 | 8.2 | 0 |
| Fogu (previous) | 46531 | 0.9 | 9.05 | 0 |
| Go | 69917 | 0.46 | 4.95 | 0 |
| Bun | 60490 | 0.9 | 3.52 | 0 |
| Python (stdlib) | 940 | 50.19 | 890.0 | 0 |

## WebSocket echo

50 connections, each sends a message and waits for its echo, 5 s.

| | messages/s | errors |
|---|---:|---:|
| Fogu | 124342 | 0 |
| Fogu (previous) | 114290 | 0 |
| Go | 79930 | 0 |
| Bun | 165849 | 0 |

## TCP echo

64 connections, 64-byte messages, each waits for its echo, 5 s.

| | messages/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Fogu | 24125 | 2.48 | 7.52 | 0 |
| Fogu (previous) | 39698 | 1.49 | 4.72 | 0 |
| Go | 125542 | 0.26 | 2.63 | 0 |
| Bun | 128129 | 0.38 | 2.06 | 0 |
| Python (asyncio) | 27939 | 2.22 | 4.01 | 0 |

## TCP echo, 256 connections

256 connections, 64-byte messages, 5 s. Fogu also with one sync.Worker per core (reusePort): `bench/net/tcp_workers.fg`.

| | messages/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Fogu (workers) | 471244 | 0.29 | 4.87 | 0 |
| Fogu | 61128 | 3.9 | 26.93 | 0 |
| Go | 389435 | 0.34 | 3.65 | 0 |
| Bun | 75113 | 3.09 | 7.71 | 0 |
| Python (asyncio) | 28911 | 8.37 | 37.42 | 0 |

## UDP echo

64 sockets, 64-byte datagrams, each waits for its echo, 5 s.

| | messages/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Fogu | 32401 | 1.94 | 3.4 | 0 |
| Fogu (previous) | 3642 | 17.77 | 20.22 | 0 |
| Go | 182316 | 0.23 | 1.78 | 0 |
| Bun | 157099 | 0.29 | 1.96 | 0 |
| Python (asyncio) | 25392 | 2.45 | 4.25 | 0 |

## Scaling: TCP echo on 1, 2, 4 and 8 cores

256 connections, 64-byte messages, 5 s per run, at least 10 runs per case. Fogu: one `sync.Worker` per core with `tcpListen(addr, { reusePort: true })` (`bench/net/tcp_workers.fg`); Go: `GOMAXPROCS` = cores. The server is pinned to that many physical cores, the load generator to the other logical CPUs. Messages/s: median, with the lowest and highest run; latencies of the median run. Every run is in `results_net.json`.

| cores | server | msg/s (median) | min – max | growth | p50 ms | p95 ms | p99 ms | errors | median / Go |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | Fogu | 66086 | 63230 – 84980 | 1.0× | 3.6 | 5.83 | 12.88 | 0 | 0.74 |
| 1 | Fogu (previous) | 62776 | 56317 – 77420 | 1.0× | 3.72 | 6.39 | 9.29 | 0 | 0.7 |
| 1 | Go | 89196 | 77992 – 114800 | 1.0× | 2.73 | 4.69 | 5.92 | 0 | 1.0 |
| 2 | Fogu | 147092 | 117833 – 156311 | 2.22× | 1.6 | 2.85 | 4.0 | 0 | 0.69 |
| 2 | Fogu (previous) | 142401 | 133860 – 148465 | 2.26× | 1.66 | 2.87 | 3.83 | 0 | 0.67 |
| 2 | Go | 211703 | 163356 – 222325 | 2.37× | 1.06 | 2.39 | 3.26 | 0 | 1.0 |
| 4 | Fogu | 274827 | 201003 – 278648 | 4.15× | 0.85 | 1.85 | 2.58 | 0 | 0.76 |
| 4 | Fogu (previous) | 262024 | 202321 – 278066 | 4.17× | 0.89 | 1.87 | 2.63 | 0 | 0.72 |
| 4 | Go | 360089 | 328170 – 364444 | 4.03× | 0.68 | 1.56 | 2.21 | 0 | 1.0 |
| 8 | Fogu | 548217 | 439469 – 555512 | 8.29× | 0.33 | 1.15 | 2.24 | 0 | 0.95 |
| 8 | Fogu (previous) | 539888 | 516691 – 550509 | 8.6× | 0.33 | 1.15 | 2.19 | 0 | 0.94 |
| 8 | Go | 572314 | 555818 – 584405 | 6.41× | 0.32 | 1.15 | 1.81 | 0 | 1.0 |

Load average of the machine (1 min) before and after: 1.49 → 13.62.

## Memory per task

20000 sleeping tasks.

| | bytes |
|---|---:|
| Fogu | 5220 |
| Python | 2236 |
| Bun | 1576 |

## Concurrent waits

`sleeps`: tasks that wait at the same time.

| | ms |
|---|---:|
| Fogu | 216 |
| Python | 368 |
| Bun | 130 |

## Message passing

`pingpong`: two tasks exchanging messages.

| | ms |
|---|---:|
| Fogu | 129 |
| Python | 1389 |
| Bun | 95 |
