# Examples

Run them from the repository root:

```bash
cargo run --release -- examples/basics.fg
# or, with the binary installed:
fogu examples/basics.fg
```

| File | Shows |
| --- | --- |
| [basics.fg](basics.fg) | variables, types, operators, control flow, lists, maps, functions, templates |
| [structs_concurrency.fg](structs_concurrency.fg) | `struct` + `embed`, `match`, `fg` / `chan` / `select` / `timeout` |
| [errors.fg](errors.fg) | errors as values, `?`, destructuring, collection methods |
| [modules.fg](modules.fg) | `import` / `export` (uses [util.fg](util.fg)) and `fogu:*` modules |
| [system.fg](system.fg) | `fogu:system`, `fogu:math`, `fogu:json` |
| [time.fg](time.fg) | `fogu:time`: `Date`, `delay`, `sleep` |
| [cron.fg](cron.fg) | `fogu:cron`: `every`, `schedule`, `tick` |
| [events.fg](events.fg) | `fogu:events`: `EventEmitter` |
| [kv.fg](kv.fg) | `fogu:kv`: embedded key-value store (creates `demo.redb`) |
| [sql.fg](sql.fg), [sql_multi_db.fg](sql_multi_db.fg) | `fogu:sql`: SQLite / PostgreSQL / MySQL |
| [http_server.fg](http_server.fg) + [http_client.fg](http_client.fg) | `fogu:http` (port 3000) |
| [web_server.fg](web_server.fg) | `fogu:web`: routes with parameters (port 3000) |
| [tcp_server.fg](tcp_server.fg) + [tcp_client.fg](tcp_client.fg) | `fogu:net` TCP (port 9000) |
| [udp_server.fg](udp_server.fg) + [udp_client.fg](udp_client.fg) | `fogu:net` UDP (port 9001) |
| [files_processes.fg](files_processes.fg) | `fogu:fs`, `fogu:path`, `fogu:process` and the `Fogu` object |
| [sync.fg](sync.fg) | `fogu:sync`: `Mutex`, `WaitGroup`, `Once`, `Atomic` |
| [test/](test/) | a project with `fogu.json` (`fogu run`, `fogu dev`, `fogu build --compile`) and tests (`fogu test`) |

Servers keep running until Ctrl+C; run the client in another terminal.
