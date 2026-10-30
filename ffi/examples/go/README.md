# Fogu in Go (cgo)

`fogu/fogu.go` wraps `libfogu_rt` (the Fogu runtime as a shared library, C API
in `fogu.h`) for Go programs; `main.go` is the example.

```sh
mkdir lib
curl -fsSLo lib/fogu.h https://fogulang.microaitec.com/download/latest/fogu.h
curl -fsSLo lib/libfogu_rt.so https://fogulang.microaitec.com/download/latest/libfogu_rt-linux-x64.so
CGO_CFLAGS="-I$PWD/lib" CGO_LDFLAGS="-L$PWD/lib -lfogu_rt" go build -o embed .
LD_LIBRARY_PATH=$PWD/lib ./embed
```

Inside the Fogu repository, add `GOWORK=off` (the root `go.work` lists only the server).

On Windows (MinGW gcc for cgo), save `fogu_rt-windows-x64.dll` as `lib/fogu_rt.dll`,
build with the same variables and keep `fogu_rt.dll` next to the `.exe`.

```go
rt, _ := fogu.New()
defer rt.Close()
rt.Eval(`fn add(a, b) { return a + b }`)
v, _ := rt.Call("add", 1, 2)                       // json.RawMessage("3")
rt.Set("config", map[string]any{"retries": 3})
rt.On("done", func(d json.RawMessage) { … })       // Fogu.host.emit("done", …)
rt.Emit("job", map[string]int{"id": 7})            // Fogu.signal.on("job", …)
rt.Spawn(`import http from "fogu:http"; http.serve(8080, fn(req) { return "hi" })`)
```

A runtime runs on its own thread; its globals, `fg` tasks, servers and signal
handlers stay alive between calls until `Close()`. `On` handlers run on that
thread: keep them short or pass the data to a goroutine.
