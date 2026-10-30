# Design note: `fogu:http` (and `fogu:web`)

Sources: [`src/native/http.rs`](../../src/native/http.rs),
[`src/native/web.rs`](../../src/native/web.rs).

## API

```fogu
import http from "fogu:http"
import time from "fogu:time"

http.serve(3000, fn(req) {          // req: method, path, query, headers, body, header(name), json()
    if req.path == "/health" {
        return http.json({ status: "ok" })
    }
    return http.text("not found", { status: 404, headers: { "X-App": "fogu" } })
})
while true { time.delay(100ms) }

// client
const r = http.get("http://127.0.0.1:3000/health")?   // also post/put/delete(url, body)
print(r.status, r.headers["content-type"], r.body, r.json())
```

`text`, `json` and `html` accept an optional second argument
`{ status, headers }`. `fogu:web` builds routing on top of the same server
(`app.get("/users/:id", fn(ctx) …)`, middlewares with `use`, `ctx.params`,
`ctx.store`).

## Threads and `Value`

The server is `hyper` on its own Tokio runtime in a dedicated thread, so it
serves connections concurrently. Fogu handlers, however, can only run on the
main thread (`Value` is not `Send`). The golden rule: **nothing that contains a
`Value` crosses a thread**.

1. The server thread turns each request into a `PlainRequest` (only `Send`
   fields) and sends it, together with a response `Sender`, over a channel.
2. The main thread drains that channel whenever the interpreter calls a value
   (`Interpreter::call_value` checks `take_pending_request` first). This is why
   a server program keeps looping with `time.delay(...)`.
3. The interpreter calls the Fogu handler, converts the returned `Response`
   object into a `PlainResponse` and sends it back. The server task waits with a
   generous timeout (504 if the handler never answers).

A handler that returns nothing answers 404 ("route not found"); a handler error
answers 500 with the message.

## Client

The client is synchronous (`ureq`). Connection failures are operational errors,
so they reach the program as `error` values (`http.get(url)?`).

## Future work

TLS, HTTP/2 and the spec's `http.serve({ port, fetch })` form; serving without
the manual `delay` loop needs the runtime event loop (see CHECKLIST.md, phase H).
