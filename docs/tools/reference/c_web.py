"""Standard library reference: HTTP, web, JWT, WebSocket and sockets."""

W = "Web & network"

SECTIONS = [
    {
        "id": "mod-http",
        "group": W,
        "title": "fogu:http",
        "import": 'import http from "fogu:http"',
        "intro": "A minimal HTTP server (one handler for every request) and an HTTP client. For routing, middleware, "
        "static files, uploads, SSE and WebSocket use <a class=\"underline\" href=\"#mod-web\">fogu:web</a>.",
        "blocks": [
            {
                "h3": "Client",
                "api": [
                    ("http.get(url, opts?) · http.delete(url, opts?) · http.head(url, opts?)", "Response · error", "Network failures are error values; HTTP 4xx/5xx are normal responses (<code>res.ok</code> is false)."),
                    ("http.post(url, body?, opts?) · http.put(…) · http.patch(…)", "Response · error", "<code>body</code>: a str (text/plain), a map or list (JSON), bytes (octet-stream)."),
                    ("http.request({ method, url, ...opts })", "Response · error", ""),
                    ("opts: { headers, query, json, form, multipart, body, timeout, redirects = true, basicAuth: [user, pass], bearer }", "", "<code>form</code>: urlencoded fields. <code>multipart</code>: a map of fields — text values, <code>http.file(path)</code> (streamed from disk), <code>{ data, filename, type }</code> bytes, or a list (a repeated field). Map keys are sent in alphabetical order."),
                    ("http.file(path, { filename, type })", "HttpFile · error", "A file to upload; the type comes from the extension."),
                    ("http.download(url, path, opts?)", "int · error", "Streams the body to a file; returns the bytes written (non-2xx is an error)."),
                    ("res.status · res.ok · res.url · res.headers · res.body", "", "<code>headers</code>: the first value of each (lower case)."),
                    ("res.json() · res.bytes() · res.header(name) · res.cookies()", "", "<code>bytes()</code>: the body as a <a class=\"underline\" href=\"#values\">buffer</a>; <code>header</code> joins repeated values."),
                ],
            },
            {
                "h3": "Server",
                "api": [
                    ("http.serve(port | \"ip:port\", fn(req))", "nil", "Requests are answered as soon as they arrive while the program is alive — waiting in <code>Fogu.signal.wait()</code>, <code>time.delay</code>, on a channel… — and each handler runs in its own task, so one that waits does not hold the others. The address is an int port (on 127.0.0.1) or <code>\"0.0.0.0:8080\"</code>, <code>\"[::1]:8080\"</code>, <code>\":8080\"</code>."),
                    ("req.method · req.path · req.query · req.headers · req.body", "", "<code>query</code> is the raw query string."),
                    ("req.header(name) · req.json() · req.form() · req.bytes()", "", "<code>form()</code>: urlencoded and multipart bodies."),
                    ("http.text(body, opts?) · http.json(value, opts?) · http.html(body, opts?)", "Response", "<code>opts: { status, headers }</code>."),
                ],
                "example": r'''
import http from "fogu:http"
import time from "fogu:time"

http.serve(8080, (req) => {
    if req.path == "/hello" {
        return http.json({ hello: "world", method: req.method })
    }
    return http.text("not found", { status: 404 })
})
while true { time.delay(50ms) }
''',
                "title": "server.fg",
            },
            {
                "example": r'''
import http from "fogu:http"
const r = http.get("http://127.0.0.1:8080/hello")?
print(r.status, r.headers["content-type"], r.json())
print(http.get("http://127.0.0.1:8080/nope")?.status)
const [_, err] = http.get("http://127.0.0.1:1/")
print(err != nil)
''',
                "title": "client.fg",
                "output": "200 application/json {hello: world, method: GET}\n404\ntrue",
            },
            {
                "text": "Forms, file uploads, headers and downloads (here against a <a class=\"underline\" href=\"#mod-web\">fogu:web</a> app in the same program):",
                "example": r'''
import http from "fogu:http"
import web from "fogu:web"
import fs from "fogu:fs"

fs.writeText("report.txt", "hello file\n")?
const app = web.app()
app.post("/echo", (ctx) => ({ type: ctx.headers["content-type"].split(";")[0], body: ctx.body }))
app.post("/upload", (ctx) => {
    const f = ctx.req.form()?
    return { title: f.title, tags: f.tags, file: f.file.filename, text: f.file.text(), avatar: f.avatar.bytes().len() }
})
app.listen(18471, { quiet: true })

let done = false
fg fn() {
    const base = "http://127.0.0.1:18471"
    print(http.post(base + "/echo", { name: "Ana" }, { query: { page: 2 } })?.json())
    print(http.post(base + "/echo", nil, { form: { user: "ana", pass: "a b&c" } })?.json())
    const up = http.post(base + "/upload", nil, { multipart: {
        title: "report", tags: ["a", "b"],
        file: http.file("report.txt"),
        avatar: { data: [1, 2, 3, 4], filename: "a.png" },
    } })?
    print(up.ok, up.json())
    done = true
}()
while !done { Fogu.sleep(10ms) }
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-web",
        "group": W,
        "title": "fogu:web",
        "import": 'import web from "fogu:web"',
        "intro": "A web framework: routes with parameters, middleware, static files and SPA builds, uploads, "
        "downloads, Server-Sent Events and WebSocket — on one port. Requests are served as soon as they arrive while "
        "the program waits (<code>Fogu.signal.wait()</code>, <code>time.delay</code>, a channel), each in its own task: "
        "a handler that waits on a query or an HTTP call does not hold the others.",
        "blocks": [
            {
                "h3": "App",
                "api": [
                    ("web.app()", "App", "Methods return the app, so calls can be chained."),
                    ("app.get/post/put/patch/delete/head/options/all(path, ...middlewares, handler)", "App", "<code>/users/:id</code> parameters, <code>*</code> wildcard."),
                    ("app.use(fn) · app.use(prefix, fn)", "App", "Middleware for every request, or under a path prefix."),
                    ("app.after((ctx, res) => …)", "App", "After the handler; returning a value replaces the response."),
                    ("app.onError((ctx, err) => response)", "App", "For runtime errors and returned <code>error</code> values (default: 500)."),
                    ("app.static(dir) · app.static(prefix, dir, { spa, index, maxAge, dotfiles })", "App", "Files served by the server thread: ETag/304, <code>Cache-Control</code>; <code>spa: true</code> serves <code>index.html</code> for unknown HTML routes (React, Vue, Svelte builds)."),
                    ("app.ws(path, ...middlewares, (ws, ctx) => …, { protocols })", "App", "WebSocket routes; the handler runs in its own task with a <a class=\"underline\" href=\"#mod-websocket\">WebSocket</a> on the upgraded HTTP connection."),
                    ("app.set(key, value) · app.store", "App · map", "Shared state (<code>ctx.store</code>)."),
                    ("app.listen(port | \"ip:port\", { host = \"127.0.0.1\", maxBody = 10 MB, quiet })", "nil · error", "<code>app.listen(\"0.0.0.0:3000\")</code> (or <code>host: \"0.0.0.0\"</code>) accepts other machines; IPv6 as <code>\"[::1]:3000\"</code>. A host in the string wins over <code>host</code>."),
                    ("app.handler()", "fn", "Calls the app without a network (tests): <code>h({ method, path, headers, body })</code> → Response."),
                    ("app.routes()", "list", ""),
                ],
            },
            {
                "h3": "Context and responses",
                "api": [
                    ("ctx.method · ctx.path · ctx.params · ctx.query · ctx.headers · ctx.body", "", "<code>query</code> is a map; header names are lower case."),
                    ("ctx.req.json() · ctx.req.form() · ctx.req.bytes()", "", "<code>form()</code>: fields as str, files as <code>UploadedFile</code>."),
                    ("ctx.set.status · ctx.set.headers", "", "Adjust the final response from middleware or handlers."),
                    ("ctx.store", "map", "The app's shared map."),
                    ("handler result", "", "A str (text or HTML), any value (JSON), a response, or nil (204)."),
                    ("web.json/text/html(body, { status, headers })", "Response", ""),
                    ("web.redirect(url, status = 302)", "Response", ""),
                    ("web.file(path, { download, contentType, status })", "Response · error", "Sent by the server thread (binary safe)."),
                    ("web.sse({ retry, headers })", "SseStream", "Return it from the handler; <code>send(data, { event, id })</code>, <code>comment(text)</code>, <code>close()</code>, field <code>open</code>."),
                    ("web.cors({ origin, methods, headers, credentials, maxAge })", "middleware", "Answers preflight requests."),
                    ("web.logger()", "middleware", "<code>GET /path 200 0.4ms</code>."),
                    ("UploadedFile: filename, contentType, size, text(), bytes(), json(), save(path)", "", "<code>save(\"uploads/\")</code> keeps a sanitized file name."),
                ],
                "example": r'''
import web from "fogu:web"
import time from "fogu:time"

const app = web.app()
app.use(web.logger())
app.use("/api", (ctx) => {
    if ctx.headers["x-key"] != "secret" { return web.json({ error: "forbidden" }, { status: 403 }) }
})
app.get("/api/users/:id", (ctx) => ({ id: ctx.params.id, page: ctx.query.page ?? "1" }))
app.post("/api/upload", (ctx) => {
    const form = ctx.req.form()?
    return { saved: form.file.save("uploads/")?, size: form.file.size }
})
app.get("/events", (ctx) => {
    const stream = web.sse()
    fg fn() {
        for i in 1..4 { stream.send({ n: i }, { event: "tick" }); time.delay(1s) }
        stream.close()
    }()
    return stream
})
app.ws("/chat", (ws, ctx) => {
    while true {
        const m = ws.next()?
        if m == nil { break }
        ws.send(`echo: ${m.data}`)
    }
})
app.static("/", "dist", { spa: true, maxAge: 1d })
app.listen(3000)?
while true { time.delay(50ms) }
''',
                "title": "app.fg",
            },
            {
                "text": "Without a network, through <code>app.handler()</code>:",
                "example": r'''
import web from "fogu:web"
const app = web.app()
app.use((ctx) => { ctx.set.headers["x-served-by"] = "fogu" })
app.get("/users/:id", (ctx) => ({ id: ctx.params.id, sort: ctx.query.sort }))
app.get("/old", (ctx) => web.redirect("/new"))
app.get("/fail", (ctx) => error("boom"))
app.onError((ctx, err) => web.json({ error: err.message }, { status: 500 }))
const h = app.handler()
const show = (path) => {
    const r = h({ method: "GET", path: path })
    print(r.status, r.body, r.headers["x-served-by"], r.headers["location"])
}
show("/users/7?sort=name")
show("/old")
show("/fail")
show("/missing")
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-topcoat",
        "group": W,
        "title": "fogu:topcoat",
        "import": 'import tc from "fogu:topcoat"',
        "intro": "Server-rendered reactive pages for <a class=\"underline\" href=\"#mod-web\">fogu:web</a>, in the model of "
        "<a class=\"underline\" href=\"https://github.com/tokio-rs/topcoat\">Topcoat</a>: HTML is built on the server; "
        "<b>signals</b> hold browser state; <code>@event</code> handlers, <code>:attr</code> bindings and live text update "
        "in the browser without a round-trip; <b>shards</b> re-render on the server when their inputs change and are swapped "
        "in place (keeping focus and input state); <b>procedures</b> are server functions called from the browser. The "
        "browser runtime (~18 KB, no build step) and the stylesheets are served by every fogu:web app under <code>/_tc/</code>. "
        "Two UI kits: <code>tc.ui</code> (the look of Topcoat UI) and <code>tc.shadcn</code> (the components of "
        "<a class=\"underline\" href=\"https://github.com/hughdbrown/shadcn-rs\">shadcn-rs</a>), plus Tailwind CSS.",
        "blocks": [
            {
                "h3": "Pages and markup",
                "api": [
                    ("tc.h(tag, attrs?, ...children)", "Html", "Children: strings (escaped), <code>Html</code>, lists, numbers, signals and expressions (live text); <code>nil</code>/<code>false</code> render nothing. Attributes: <code>true</code> → bare, <code>false</code>/<code>nil</code> → left out, <code>class</code> may be a list or a <code>{ name: on }</code> map, <code>style</code> a map."),
                    ("@event: \"js\" · :attr: \"js\" | signal", "", "<code>@click</code> runs JavaScript statements (<code>e</code> is the event, signals are in scope by name, <code>await</code> works); modifiers <code>.prevent .stop .once .self .enter .escape .debounce[.ms]</code>. <code>:value</code>, <code>:hidden</code>, <code>:class</code>, <code>:disabled</code>… follow an expression. Written as <code>tc-on:click</code> / <code>tc-bind:hidden</code> in raw HTML."),
                    ("tc.page({ title, head, body, lang = \"en\", theme = \"light\" | \"dark\" | \"system\", ui = true | \"shadcn\" | false, tailwind, icon, bodyClass })", "Html", "A whole document with the runtime, the stylesheet and the Fogu favicon (<code>icon</code>: another URL, or <code>false</code>). The theme the user picked with <code>ui.themeToggle()</code> is applied before the first paint. Return an <code>Html</code> from a handler: it is sent as HTML with the page's signals."),
                    ("tc.raw(html) · tc.fragment(...children) · tc.escape(str) · html.html", "Html · str", ""),
                    ("tc.script() · tc.styles() · tc.tailwind(true | href)", "Html", "For documents written by hand."),
                ],
            },
            {
                "h3": "Signals, shards and procedures",
                "api": [
                    ("tc.signal(ctx, name, initial)", "Signal", "Browser state named <code>name</code> (a JS identifier). <code>s.value</code> / <code>s.get()</code> read it on the server and make it <b>tracked</b>: a change in the browser renders the page (or the shard that read it) again with the new value. <code>s.peek()</code> reads without tracking. Values sent by the browser are user input: validate them."),
                    ("in the browser: s.get() · set(v) · update(f) · increment(n) · decrement(n) · toggle() · push(x)", "", "Plus <code>call(name, ...args)</code>, <code>navigate(url)</code>, <code>toast(title, { description, type, duration })</code>, <code>await ask(message, { title, confirm, cancel, danger })</code> (a confirmation dialog → bool), <code>copy(text, label)</code>, <code>toggleTheme()</code> and <code>tc.signals</code>."),
                    ("tc.expr(js, initial?)", "Expr", "A browser expression: live text, an <code>:attr</code> value or a shard prop."),
                    ("tc.shard(name, fn(ctx, props)) · shard.render(ctx, props) · app.shard(shard)", "Shard · Html", "Props are fixed values or signals/expressions; when one changes, <code>POST /_tc/s/&lt;name&gt;</code> renders it again and the browser morphs it in. The first page that renders a shard registers it; <code>app.shard</code> does it up front. Check access inside the shard: page middleware scoped to a path does not run for it."),
                    ("app.procedure(name, fn(ctx, ...args))", "App", "<code>await call(name, ...args)</code> in the browser; an <code>error</code> value (or <code>return nil, error(…)</code>) rejects the call with its message."),
                    ("ui.link(children, href) · <a tc-link>", "Html", "Navigation without a full reload (prefetched on hover, history kept)."),
                    ("protocol", "", "Topcoat's frames, one JSON object per line (<code>application/x-ndjson</code>): <code>snapshot</code> (a rerun: <code>POST</code> of the signals to the page URL with <code>x-topcoat: rerun</code>), <code>swap</code>, <code>redirect</code>, <code>error</code>. <code>ctx.tc</code>: <code>{ signals, input, rerun, page }</code>."),
                ],
            },
            {
                "h3": "UI kits and Tailwind",
                "api": [
                    ("tc.ui.button(children, { variant, size, href }) · badge · card(content, { title, description, footer, action }) · stat(label, value, { hint })", "Html", "Topcoat UI's look (its neutral theme tokens, plain CSS: <code>/_tc/ui.css</code>). Variants <code>secondary outline ghost destructive link</code>; badges also <code>success warning</code>; sizes <code>sm lg icon</code>. Extra options become attributes (<code>@click</code>, <code>class</code>…)."),
                    ("tc.ui.icon(name, { size = 16 }) · logo({ size, text }) · sparkline(values, { width, height, area }) · empty(title, { description, icon, action }) · live(text) · themeToggle()", "Html", "35 built-in stroke icons (<code>activity alert arrow-right check chevron-right clock copy database edit eye flame hash home key layers lock logout message moon network plus radio refresh search send server settings shield sun terminal trash users x zap</code>); <code>stat(label, value, { icon, tone, trend: [numbers], hint })</code> draws a trend line; tones <code>primary success warning destructive info</code>. <code>shell</code> nav items take an <code>icon</code>, and <code>footer</code> goes under the nav; <code>grid(children, { min })</code>, <code>table(…, { header: false })</code>."),
                    ("tc.ui.input · textarea · select(options, { value }) · checkbox · switch · label · field(label, control, { description, error })", "Html", ""),
                    ("tc.ui.table(columns, rows, { empty, caption }) · alert(title, text, { variant }) · progress(value) · skeleton · spinner · separator · kbd · tabs · link · shell({ brand, nav, title, actions }, content) · row · stack · grid · container", "Html", "<code>columns</code>: names or <code>{ key, label, align }</code>; rows: maps or lists; cells may be <code>Html</code>."),
                    ("tc.shadcn.button · badge · card · stat · alert · input · textarea · select · checkbox · switch(signal) · label · field · table", "Html", "The markup and class names of shadcn-rs, styled by its stylesheet (<code>page({ ui: \"shadcn\" })</code>). Button variants <code>default primary secondary destructive outline ghost link</code>, sizes <code>xs sm md lg xl</code>."),
                    ("tc.shadcn.tabs(signal, [{ value, label, content }]) · dialog(signal, { title, description, footer, trigger }, content) · accordion([{ title, content, open }], { multiple }) · dropdown(trigger, items) · tooltip(trigger, text) · pagination(page, pages, { href }) · progress · skeleton · separator · avatar · kbd · breadcrumb · shell · toaster", "Html", "Interactive parts run in the browser: tabs and dialogs follow a signal; <code>pagination</code> takes a signal or an <code>href</code> with <code>{page}</code>."),
                    ("tc.page({ tailwind: true }) · tc.page({ tailwind: \"/app.css\" }) · tc.tailwindTheme", "", "<code>true</code> loads Tailwind v4's browser build (development) with the theme tokens as utilities (<code>bg-primary</code>, <code>text-muted-foreground</code>, <code>border-border</code>, <code>dark:</code>); for production build a stylesheet with the Tailwind CLI (<code>@import \"tailwindcss\"; @source \"./**/*.fg\";</code> plus <code>tc.tailwindTheme</code>) and pass its URL."),
                ],
                "example": r'''
import web from "fogu:web"
import tc from "fogu:topcoat"
import json from "fogu:json"
const h = tc.h
const ui = tc.ui

const results = tc.shard("results", fn(ctx, props) {
    const found = ["apple", "banana", "mango"].filter(fn(f) { return f.contains(props.query ?? "") })
    return ui.table(["fruit"], found.map(fn(f) { return { fruit: f } }), { empty: "Nothing found" })
})
const app = web.app()
app.procedure("double", fn(ctx, n) { return n * 2 })
app.get("/", fn(ctx) {
    const count = tc.signal(ctx, "count", 0)
    const query = tc.signal(ctx, "query", "")
    return tc.page({ title: "Demo", body: ui.container([
        h("p", nil, "Count: ", count),
        ui.button("+1", { "@click": "count.increment()" }),
        ui.button("double on the server", { variant: "outline", "@click": "count.set(await call('double', count.get()))" }),
        ui.input({ placeholder: "Search", ":value": query, "@input": "query.set(e.target.value)" }),
        results.render(ctx, { query: tc.expr("query.get()", query.peek()) }),
    ])})
})

// what the browser does, without a network:
const call = app.handler()
print(call({ method: "GET", path: "/" }).body.contains("data-tc-text=\"count.get()\""))
const swap = json.parse(call({ method: "POST", path: "/_tc/s/results", body: "{\"props\":{\"query\":\"an\"}}" }).body)
print(swap.t, swap.html.contains("banana"), swap.html.contains("apple"))
print(call({ method: "POST", path: "/_tc/p/double", body: "[21]" }).body)
// app.listen(3000) and Fogu.signal.wait() to serve it
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-jwt",
        "group": W,
        "title": "fogu:jwt",
        "import": 'import jwt from "fogu:jwt"',
        "intro": "JSON Web Tokens with HMAC (HS256/HS384/HS512). A token that fails verification is an error value.",
        "blocks": [
            {
                "api": [
                    ("jwt.sign(payload, secret, { algorithm, expiresIn, notBefore, issuer, audience, subject, jwtid, header, noTimestamp })", "str", "<code>iat</code> is added; durations or seconds for times."),
                    ("jwt.verify(token, secret, { algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration })", "map · error", "The claims."),
                    ("jwt.decode(token)", "map · error", "<code>{ header, payload }</code> without verifying."),
                    ("jwt.middleware(secret, { property = \"user\", optional, …verify options })", "middleware", "For <code>fogu:web</code>: <code>Authorization: Bearer</code> → <code>ctx.user</code>, otherwise 401."),
                ],
                "example": r'''
import jwt from "fogu:jwt"
const token = jwt.sign({ sub: "42", role: "admin" }, "s3cret", { expiresIn: 1h, issuer: "fogu" })
const claims = jwt.verify(token, "s3cret", { issuer: "fogu" })?
print(claims.sub, claims.role, claims.exp - claims.iat)
print(jwt.decode(token)?.header)
const [_, e1] = jwt.verify(token, "wrong")
const [_, e2] = jwt.verify(jwt.sign({ a: 1 }, "s3cret", { expiresIn: -10 }), "s3cret")
print(e1, e2)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-socketio",
        "group": W,
        "title": "fogu:socketio",
        "import": 'import socketio from "fogu:socketio"',
        "intro": "Socket.IO (protocol v5) servers and clients, compatible with the official JavaScript, Python and Go "
        "libraries. The server is socketioxide (WebSocket and long polling); the client speaks Engine.IO v4 over WebSocket; it runs standalone or on the port of a "
        "<a class=\"underline\" href=\"#mod-web\">fogu:web</a> app. Handlers run as <code>fg</code> tasks in the order the "
        "messages arrived; keep the program alive meanwhile (<code>Fogu.signal.wait()</code>, a loop…).",
        "blocks": [
            {
                "h3": "Server",
                "api": [
                    ("socketio.server({ path = \"/socket.io\", pingInterval, pingTimeout, ackTimeout, maxPayload })", "SocketIoServer", ""),
                    ("io.on(\"connection\", fn(socket)) · io.of(\"/ns\").on(\"connection\", fn)", "nil", "Namespaces."),
                    ("io.listen(port | \"host:port\") · app.listen(port, { socketio: io })", "int · nil", "Standalone, or on a fogu:web app (same port)."),
                    ("io.emit(event, ...args) · io.to(room).emit(…) · io.except(room) · io.of(ns).emit(…) · io.close()", "", "Several arguments travel as several arguments."),
                    ("socket.on(event, fn(...args, ack?)) · socket.on(\"disconnect\", fn(reason)) · socket.off(event)", "nil", "<code>ack</code> is passed when the client asked for one: <code>ack(value)</code>."),
                    ("socket.emit(event, ...args) · socket.emitWithAck(event, data, { timeout })", "nil · value", ""),
                    ("socket.join(room) · leave(room) · to(room).emit(…) · broadcast.emit(…) · disconnect()", "", "<code>to</code>/<code>broadcast</code> exclude the socket itself."),
                    ("socket.id · ns · rooms · auth · handshake · connected", "", "<code>auth</code>: what the client sent; <code>handshake</code>: headers and query."),
                ],
            },
            {
                "h3": "Client",
                "api": [
                    ("socketio.connect(url, { namespace, auth, headers, path = \"/socket.io\", reconnect = true, timeout = 10s })", "SocketIoClient · error", "Returns once connected. Like the JavaScript client, the url's path is the namespace (<code>http://host:3000/admin</code>). A refused connection (auth middleware) is an error value."),
                    ("c.on(event, fn(...args)) · c.on(\"connect\" | \"disconnect\" | \"error\", fn) · c.off(event)", "nil", "Handlers registered right after <code>connect</code> see the first messages."),
                    ("c.emit(event, ...args) · c.emitWithAck(event, data, { timeout = 5s }) · c.disconnect()", "", ""),
                ],
                "example": r'''
import socketio from "fogu:socketio"
const log = []
const io = socketio.server()
io.on("connection", fn(socket) {
    socket.join("lobby")
    socket.on("chat", fn(msg, ack) {
        io.to("lobby").emit("chat", "echo:" + msg)
        if ack != nil { ack({ ok: true }) }
    })
    socket.on("disconnect", fn(reason) { log.push("bye") })
})
const port = io.listen("127.0.0.1:0")

const got = []
const c = socketio.connect("http://127.0.0.1:" + str(port), { auth: { token: "t1" } })?
c.on("chat", fn(m) { got.push(m) })
c.emit("chat", "hi")
print(c.emitWithAck("chat", "ack me", { timeout: 2s })?)
Fogu.sleep(200ms)
c.disconnect()
Fogu.sleep(200ms)
print(got, log)
io.close()
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-grpc",
        "group": W,
        "title": "fogu:grpc",
        "import": 'import grpc from "fogu:grpc"',
        "intro": "gRPC servers and clients without code generation: the messages are described by a "
        "<a class=\"underline\" href=\"#mod-protobuf\">fogu:protobuf</a> schema (<code>.proto</code> compiled at run time) and "
        "travel as maps. Unary, server streaming, client streaming and bidirectional methods (the requests of a "
        "bidirectional call are all received before the handler runs). Each request runs as its own <code>fg</code> task.",
        "blocks": [
            {
                "api": [
                    ("grpc.server(schema)", "GrpcServer", ""),
                    ("srv.handle(method, fn(req, call)) · srv.handle({ method: fn }) · srv.service(\"pkg.Service\", { Method: fn })", "nil", "Methods: <code>\"pkg.Service/Method\"</code>, <code>\"pkg.Service.Method\"</code> or a unique <code>\"Method\"</code>. Client streaming: <code>req</code> is a list."),
                    ("call.send(msg) · call.metadata · call.peer · call.method · call.cancelled", "", "<code>send</code> for server streaming (or return a list)."),
                    ("srv.listen(port | \"host:port\", { cert, key })", "int · error", "The port. <code>cert</code>/<code>key</code>: PEM files for TLS."),
                    ("srv.serve() · srv.close() · srv.port · srv.address · srv.methods", "", "<code>serve</code> runs until <code>close</code> (start it with <code>fg</code>)."),
                    ("return nil, grpc.status(\"NOT_FOUND\", \"msg\")", "", "Fails the call with that code (any other error is <code>UNKNOWN</code>); <code>grpc.codes</code> lists them."),
                    ("grpc.connect(url, schema, { timeout, metadata, ca, domain })", "GrpcClient", "<code>https://</code> uses TLS (system roots or <code>ca</code>). Connects on the first call."),
                    ("c.call(method, req, { timeout, metadata })", "value · error", "A list for server streaming; a list of requests for client streaming. Failures: <code>grpc: NOT_FOUND: msg</code>, <code>grpc: DEADLINE_EXCEEDED: …</code>."),
                    ("c.stream(method, req, opts) → s.next() · s.close() · s.done · s.received", "GrpcStream", "One message at a time (<code>nil</code> at the end)."),
                ],
                "example": r'''
import grpc from "fogu:grpc"
import protobuf from "fogu:protobuf"
const schema = protobuf.parse("syntax = \"proto3\"; package demo;
message HelloRequest { string name = 1; }
message HelloReply { string message = 1; }
message Num { int32 n = 1; }
service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
  rpc Count (Num) returns (stream Num);
}")?
const srv = grpc.server(schema)
srv.handle("demo.Greeter/SayHello", fn(req, call) {
    if req.name == "" { return nil, grpc.status("INVALID_ARGUMENT", "name is required") }
    return { message: "Hello, " + req.name }
})
srv.handle("Count", fn(req, call) { for i in 0..req.n { call.send({ n: i }) } })
const port = srv.listen("127.0.0.1:0")?
fg srv.serve()

const c = grpc.connect("127.0.0.1:" + str(port), schema)?
print(c.call("SayHello", { name: "Ana" })?)
print(c.call("SayHello", { name: "" }))
print(c.call("Count", { n: 3 })?)
srv.close()
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-security",
        "group": W,
        "title": "fogu:security",
        "import": 'import security from "fogu:security"',
        "intro": "Security and network auditing: TLS and certificates (rustls, x509-parser, rcgen), port scanning, DNS "
        "(own resolver: UDP with EDNS0, TCP fallback), HTTP security headers, password strength (zxcvbn), TOTP two-factor codes, secret scanning, "
        "Ed25519 signatures, IP ranges (ipnet) and rate limiting. Scan only hosts you are allowed to test.",
        "blocks": [
            {
                "api": [
                    ("security.tls(host, { port = 443, sni, ca, alpn, timeout = 5s })", "map · error", "<code>{ valid, error, protocol, cipher, alpn, daysLeft, handshakeMs, chain }</code> — the chain is inspected even when it is not trusted."),
                    ("security.parseCert(pem | der) · security.selfSigned(names, { days = 365 })", "map · { cert, key }", "<code>subject commonName issuer names serial notBefore notAfter daysLeft expired selfSigned ca signature keyType keyBits sha256</code>."),
                    ("security.scan(host, ports = \"common\", { timeout = 500ms, concurrency = 200, banner })", "list · error", "Ports: an int, a list, a range or <code>\"22,80,8000-8100\"</code>. → <code>[{ port, service, banner }]</code>."),
                    ("security.dns(name, type = \"A\", { server, timeout = 3s }) · security.reverseDns(ip, opts?)", "list · error", "<code>A AAAA MX TXT NS CNAME SOA CAA SRV PTR…</code> → <code>[{ type, value, ttl }]</code>. Servers: <code>server</code>, <code>$FOGU_DNS</code>, the system's, then 1.1.1.1/8.8.8.8."),
                    ("security.headers(url, { timeout })", "map · error", "HSTS, CSP, X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy → <code>{ grade, score, present, missing, warnings }</code> (leaky <code>server</code>, unsafe CSP, cookies without Secure/HttpOnly/SameSite…)."),
                    ("security.passwordStrength(pw, [userInputs])", "map", "<code>{ score 0..4, guesses, crackTime, crackTimeOnline, warning, suggestions }</code>."),
                    ("security.totpSecret() · totp(secret, opts) · verifyTotp(secret, code, { window = 1 }) · otpauthUrl(secret, account, { issuer })", "", "RFC 6238 (SHA-1, 6 digits, 30 s); the URL goes into a QR code for authenticator apps."),
                    ("security.scanSecrets(text) · security.scanFiles(path)", "list", "AWS keys, GitHub/GitLab/Slack/Stripe/OpenAI/Anthropic tokens, private keys, JWTs, passwords in URLs, <code>password = \"…\"</code> → <code>[{ file?, rule, line, column, match }]</code> (redacted)."),
                    ("security.keypair() · sign(privateKey, msg) · verify(publicKey, msg, sig)", "", "Ed25519, base64 keys and signatures."),
                    ("security.constantTimeEq(a, b)", "bool", ""),
                    ("security.cidr(c) · inCidr(ip, c | [c]) · isPrivate(ip) · hosts(c, { limit })", "", ""),
                    ("security.rateLimiter(ratePerSecond, burst) → allow(key?, cost?) · remaining(key) · reset(key?)", "RateLimiter", "A token bucket per key (e.g. the client IP)."),
                ],
                "example": r'''
import security from "fogu:security"
const pair = security.selfSigned(["localhost", "127.0.0.1"], { days: 30 })
const cert = security.parseCert(pair.cert)?
print(cert.commonName, cert.names, cert.daysLeft, cert.selfSigned)
print(security.passwordStrength("password123").score, security.passwordStrength("correct horse battery staple 9!").score)
const secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
print(security.totp(secret, { time: 59, digits: 8 }), security.verifyTotp(secret, "287082", { time: 59 }))
for s in security.scanSecrets("aws_key = \"AKIAIOSFODNN7EXAMPLE\"") { print(s) }
const k = security.keypair()
print(security.verify(k.publicKey, "hello", security.sign(k.privateKey, "hello")))
print(security.cidr("192.168.1.77/24").network, security.inCidr("10.1.2.3", "10.0.0.0/8"), security.isPrivate("8.8.8.8"))
const rl = security.rateLimiter(1, 2)
print(rl.allow("ip"), rl.allow("ip"), rl.allow("ip"))
''',
                "run": True,
            },
            {
                "example": r'''
import security from "fogu:security"
const t = security.tls("example.com")?
print(t.valid, t.protocol, t.daysLeft > 0, t.chain[0].issuer)
print(security.scan("127.0.0.1", "22,80,443,5432")?)
print(security.dns("gmail.com", "MX")?[0].type)
const h = security.headers("https://github.com")?
print(h.grade, h.missing.map(fn(m) { return m.header }))
''',
                "output": "true TLSv1.3 true C=US, O=SSL Corporation, CN=Cloudflare TLS Issuing ECC CA 3\\n[{banner: nil, port: 22, service: ssh}]\\nMX\\nB [permissions-policy]",
                "output_title": "output (with network access)",
            },
        ],
    },
    {
        "id": "mod-websocket",
        "group": W,
        "title": "fogu:websocket",
        "import": 'import websocket from "fogu:websocket"',
        "intro": "WebSocket client (<code>ws://</code>, <code>wss://</code>) and a standalone server. The same "
        "connection object is used by <code>app.ws</code> in <code>fogu:web</code>. Each connection runs on the shared "
        "runtime (no thread per connection): <code>next</code> and <code>accept</code> wait cooperatively and wake up "
        "when a message or a client arrives.",
        "blocks": [
            {
                "api": [
                    ("websocket.connect(url, { headers, protocols, timeout })", "WebSocket · error", "A rejected handshake (401, 404…) is an error value."),
                    ("websocket.listen(port, { host })", "Server · error", "<code>port 0</code> picks a free port; <code>server.accept(timeout?)</code> → WebSocket, fields <code>port url</code>."),
                    ("ws.send(data)", "nil · error", "str → text frame, <a class=\"underline\" href=\"#values\">buffer</a> or list of bytes → binary frame, other values → JSON text."),
                    ("ws.next(timeout?)", "Message · nil", "<code>{ type: \"text\" | \"binary\", data, text, bytes, size, json() }</code>: <code>data</code> is a str for text frames and a buffer for binary ones, <code>text</code> always a str, <code>bytes</code> always a buffer. nil on timeout or when closed."),
                    ("ws.tryNext() · ws.ping() · ws.close(code = 1000, reason)", "", "<code>tryNext</code>: a queued message or nil, without waiting."),
                    ("ws.open · ws.closeCode · ws.closeReason · ws.path · ws.url · ws.headers · ws.protocol · ws.pending · ws.error", "", "<code>pending</code>: messages queued; <code>error</code>: why the connection failed, or nil."),
                ],
                "example": r'''
import websocket from "fogu:websocket"
const server = websocket.listen(0)?
fg fn() {
    const conn = server.accept()?
    while true {
        const m = conn.next()?
        if m == nil { break }
        if m.type == "binary" {
            conn.send(m.data)                   // a buffer: echoed as a binary frame
        } else {
            conn.send(`echo ${conn.path}: ${m.data}`)
        }
    }
}()
const ws = websocket.connect(server.url + "/room?x=1")?
ws.send("hello")
print(ws.next(2s)?.data)
ws.send({ kind: "json" })
print(ws.next(2s)?.data)
ws.send(bytes([1, 2, 255]))
const m = ws.next(2s)?
print(m.type, m.data, m.size, typeof(m.text))
ws.close()
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-net",
        "group": W,
        "title": "fogu:net",
        "import": 'import net from "fogu:net"',
        "intro": "TCP and UDP sockets on the shared runtime: an operation that waits (<code>accept</code>, reads, writes, "
        "<code>recv</code>, <code>tcpConnect</code>) lets the other <code>fg</code> tasks run. Data is read as a "
        "<a class=\"underline\" href=\"#values\">buffer</a> (<code>read readExact readAll recv</code>) or as text "
        "(<code>readLine readText</code>); writes take a str, a buffer or a list of byte ints. Network failures are error values. "
        "Timeouts are durations or int milliseconds.",
        "blocks": [
            {
                "api": [
                    ("net.tcpListen(\"host:port\")", "TcpListener · error", "Port 0 picks a free one. <code>accept(timeout?)</code> → TcpStream, <code>for conn in listener</code>, <code>setTimeout(d)</code>, <code>close()</code>; fields <code>addr localAddr port</code>."),
                    ("net.tcpConnect(\"host:port\", timeout?)", "TcpStream · error", ""),
                    ("stream.read(n = 64 KiB)", "buffer · nil · error", "What arrived (up to <code>n</code> bytes); <code>nil</code> at the end of the stream."),
                    ("stream.readExact(n) · stream.readAll()", "buffer · error", "Exactly <code>n</code> bytes · everything up to the end of the stream."),
                    ("stream.readLine() · stream.readText(n)", "str · nil · error", "A line without <code>\\n</code>/<code>\\r\\n</code> · up to <code>n</code> bytes as text; <code>nil</code> at the end of the stream."),
                    ("stream.write(data) · stream.writeLine(data)", "int · error", "Bytes written."),
                    ("stream.setTimeout(d) · stream.close()", "", "<code>setTimeout</code> bounds every later operation (a timeout is an error value)."),
                    ("stream.peer · stream.localAddr · stream.open", "", ""),
                    ("net.udpBind(\"host:port\")", "UdpSocket · error", "<code>sendTo(addr, data)</code>, <code>recvFrom(n?)</code> → <code>{ data, text, from, len }</code> (<code>data</code> is a buffer), <code>connect(addr)</code>, <code>send(data)</code>, <code>recv(n?)</code> → buffer, <code>setTimeout(d)</code>, <code>close()</code>; field <code>addr</code>."),
                    ("net.resolve(host)", "list · error", "IP addresses."),
                ],
                "example": r'''
import net from "fogu:net"
const server = net.tcpListen("127.0.0.1:0")?
fg fn() {
    while true {
        const conn = server.accept()?
        fg fn() {                               // one task per connection
            while true {
                const data = conn.read()?       // a buffer · nil at end of stream
                if data == nil { break }
                conn.write(data)?
            }
            conn.close()
        }()
    }
}()

const c = net.tcpConnect(server.addr, 2s)?
c.setTimeout(2s)                                // bounds every later operation
c.write("one\ntwo\n")?
print(c.readLine()?, "|", c.readLine()?)
c.write(bytes([0, 255, 10]))?
const raw = c.readExact(3)?
print(typeof(raw), raw.hex(), raw[1])
c.close()

const a = net.udpBind("127.0.0.1:0")?
const b = net.udpBind("127.0.0.1:0")?
a.sendTo(b.addr, "hello udp")?
const p = b.recvFrom()?
print(p.text, p.len, typeof(p.data), p.from == a.addr)
''',
                "run": True,
            }
        ],
    },
]
