# Fogu networking: servers and tasks

AMD Ryzen 7 5700U with Radeon Graphics · 16 cores · median of 1 rounds · fogu 0.7.5

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

## UDP echo

64 sockets, 64-byte datagrams, each waits for its echo, 5 s.

| | messages/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Fogu | 32401 | 1.94 | 3.4 | 0 |
| Fogu (previous) | 3642 | 17.77 | 20.22 | 0 |
| Go | 182316 | 0.23 | 1.78 | 0 |
| Bun | 157099 | 0.29 | 1.96 | 0 |
| Python (asyncio) | 25392 | 2.45 | 4.25 | 0 |

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
