# Fogu in Bun (bun:ffi)

`libfogu_rt` is the Fogu runtime as a shared library: `.so` (Linux), `.dll`
(Windows), `.dylib` (macOS). Its C API is `fogu.h`. `fogu.ts` wraps it for Bun.

```sh
# put the library of your platform next to fogu.ts, e.g.
curl -fsSLO https://fogulang.microaitec.com/download/latest/libfogu_rt-linux-x64.so
bun example.ts
```

| Platform | File |
| --- | --- |
| Linux x64 | `libfogu_rt-linux-x64.so` |
| Linux ARM64 (Raspberry Pi 64-bit) | `libfogu_rt-linux-arm64.so` |
| Windows x64 | `fogu_rt-windows-x64.dll` |
| Windows x86 (32-bit) | `fogu_rt-windows-x86.dll` |
| macOS | `libfogu_rt-macos-arm64.dylib` / `-x64.dylib` (build on a Mac: `cargo ffi`) |

`FOGU_RT_LIB=/path/to/lib` overrides the lookup.

```ts
import { Fogu } from "./fogu.ts";
const fogu = new Fogu();
fogu.eval('fn add(a, b) { return a + b }');
fogu.call("add", 1, 2);          // 3
fogu.set("config", { retries: 3 });
fogu.on("done", (d) => console.log(d));                 // Fogu.host.emit("done", …)
fogu.emit("job", { id: 7 });                            // Fogu.signal.on("job", …)
fogu.spawn('import http from "fogu:http"; http.serve(8080, fn(req) { return "hi" })');
fogu.close();
```

A runtime runs on its own thread: its globals, `fg` tasks, servers and signal
handlers stay alive between calls until `close()`. Values cross as JSON; a Fogu
error becomes a JavaScript `Error`.
