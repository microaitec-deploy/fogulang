# desktop

Build native desktop apps with a **Fogu backend** and a **web frontend**. The
window and webview come from Rust + [Tauri 2](https://tauri.app). Your logic
is a Fogu program (`main.fg`) that can use the whole standard library (files,
SQL, HTTP, processes…). The UI is plain HTML/CSS/JS or a Vite project (React,
Vue, Svelte, TypeScript) with hot reload while you develop. The page calls
Fogu functions with `await fogu.call(...)`, and events travel both ways. One
command turns the app folder into **one executable**, and the people who run
it need neither Fogu nor Rust.

Use it when you want a small, self-contained desktop tool (an editor, a
dashboard, an internal admin app, a front end for a script) with a modern web
UI and no Electron-sized runtime. It also gives you a **tray icon** with a
menu, **window control** from Fogu and from the page (size, position,
maximize, fullscreen, always on top, several windows, window events, asking
before close), **frameless windows** with your own title bar, and an
**Android/iOS build** of the same app.

- [Quick start](#quick-start)
- [Examples](#examples)
- [API reference](#api-reference)
- [Building and shipping](#building-and-shipping)
- [Mobile: Android and iOS](#mobile-android-and-ios)
- [Vite UIs and the dev loop](#vite-uis-and-the-dev-loop)
- [How it works](#how-it-works)
- [Troubleshooting / FAQ](#troubleshooting--faq)
- [Limitations](#limitations)

## Quick start

```sh
fogu desktop create myapp     # fogu.json + main.fg + web/ (Vite + React + TypeScript)
cd myapp
fogu add desktop              # optional: pins the desktop library in fogu.json
fogu desktop dev              # opens the window on the Vite dev server (hot reload)
fogu desktop build            # dist/myapp: one executable (dist/myapp.exe on Windows)
```

If the library is not installed yet, the first `fogu desktop ...` command
installs it. To install it ahead of time, run `fogu add -g desktop`. An app is
the **generic host** (the Tauri window with the Fogu runtime, one per platform)
with the app appended to it, so `dev` starts and `build` finishes in seconds
and **neither needs Rust, cargo or the Tauri CLI**: the host comes prebuilt
with the library for Windows x64, Linux x64 and macOS (Apple Silicon and
Intel), and the build
writes the app's own name, version and icon into the executable itself (see
[Building and shipping](#building-and-shipping)). Rust is only needed for a
platform or a set of standard library areas without a prebuilt host, for a
debug host (`--debug`, `dev --cargo`) and for `--installer`; the host is then
compiled once and cached in `~/.fogu/desktop/hosts/` (see
[Requirements](#requirements-per-os)).

The backend registers **commands**. The page calls them and gets their return
values back. The backend also sends **events**, and the page listens for them:

```fogu
// main.fg
import desktop from "desktop"

const app = desktop.app({ name: "counter", title: "Counter", identifier: "dev.fogu.examples.counter" })
app.createBrowser({ width: 360, height: 240 })   // the main window

let count = 0
app.command("increment", fn(by) {   // await fogu.call("increment", 1)
    count += by ?? 1
    app.emit("count", count)          // fogu.on("count", fn) in the page
    return count                      // the value of the page's promise
})

app.run()                             // show the window (returns at once)
```

```html
<!-- ui/index.html: window.fogu is injected by the host, nothing to import -->
<button onclick="fogu.call('increment', 1)">+1</button>
<span id="n">0</span>
<script>
  fogu.on("count", (n) => (document.getElementById("n").textContent = n));
</script>
```

Here is the real output of `fogu desktop create myapp`. The path on the first
line is the absolute path of the new folder:

```text
created /path/to/myapp (react template)

next:
  cd myapp
  fogu add desktop                      # pins the desktop lib in fogu.json (optional)
  fogu desktop dev                      # the window on the Vite dev server (HMR)
  fogu desktop build                    # dist/myapp  (--target windows from Linux/WSL)
```

```text
myapp/.gitignore
myapp/fogu.json
myapp/main.fg
myapp/web/index.html
myapp/web/package.json
myapp/web/src/App.tsx
myapp/web/src/fogu-desktop.d.ts
myapp/web/src/fogu-desktop.js
myapp/web/src/main.tsx
myapp/web/src/style.css
myapp/web/tsconfig.json
myapp/web/vite.config.ts
```

The other templates are `--template vanilla`, `ts`, `vue` and `svelte`.

## Examples

Each example is a small app under [`examples/`](examples). All of them **need
a desktop (GUI)** to show their window. Each `main.fg` also runs without a
window (`fogu main.fg`) and exercises its commands. The outputs shown below
come from those runs. To try an example in its window:

```sh
cd examples/02-commands
fogu desktop build --debug     # a plain ui/ folder: build a debug window (has devtools)
./dist/commands                # dist\commands.exe on Windows
```

`fogu desktop dev` runs any of them with hot reload: edit `main.fg` and the
program reloads in place (the window stays), edit a file of `ui/` and the
page reloads (a Vite UI gets HMR instead).

| # | Folder | Shows |
| --- | --- | --- |
| 1 | [`01-counter`](examples/01-counter) | The smallest app: one command, one event |
| 2 | [`02-commands`](examples/02-commands) | Payloads, return values, errors, runtime errors |
| 3 | [`03-events`](examples/03-events) | Events both ways, `"*"` listeners, `fogu.once`, changing the window |
| 4 | [`04-dialogs`](examples/04-dialogs) | Open, save as, pick a folder, message, confirm |
| 5 | [`05-background-task`](examples/05-background-task) | A long job with progress events and cancel |
| 6 | [`06-settings`](examples/06-settings) | Settings JSON in the app's data folder, remembering the window size |
| 7 | [`hello`](examples/hello) | A complete todo app: all of the above, with tests |
| 8 | [`vite-react`](examples/vite-react) | Vite + React + TypeScript calling commands with types |
| 9 | [`07-tray-windows`](examples/07-tray-windows) | A tray icon with a menu (check items, a submenu, quit), close-to-tray, window control from Fogu and from the page, a second window |
| 10 | [`08-frameless`](examples/08-frameless) | A window without the system title bar: its own bar, drag region and buttons; Fogu asks before closing |
| 11 | [`09-native`](examples/09-native) | A menu bar, a global shortcut, notifications, a worker for CPU-bound work, window opacity, bytes across the boundary, state kept across hot reloads |
| 12 | [`10-app-in-code`](examples/10-app-in-code) | The counter again, with the app and its windows defined in `main.fg` (`desktop.app`, `app.createBrowser`, `app.run`) instead of `fogu.json` |
| 13 | [`11-heroui`](examples/11-heroui) | A task list with a React UI built with [HeroUI](https://heroui.com) 3 and Tailwind CSS 4: cards, a text field, checkboxes, chips, a progress bar, a dark mode switch and toasts for the errors of Fogu commands |

### 1. The smallest app

Start here: one window, one command and one event cover the whole bridge.
The app folder contains `fogu.json`, `main.fg` and `ui/index.html`. Every
example describes its app with `desktop.app` and opens its windows with
`app.createBrowser`; `fogu.json` keeps only what the build and the host need
before the program runs (see [The app in code](#the-app-in-code)).

```json
{
  "name": "counter",
  "version": "0.1.0",
  "entry": "main.fg",
  "desktop": {
    "title": "Counter",
    "identifier": "dev.fogu.examples.counter",
    "ui": "ui",
    "visible": false
  }
}
```

```fogu
// main.fg
import desktop from "desktop"

const app = desktop.app({ name: "counter", title: "Counter", identifier: "dev.fogu.examples.counter" })
app.createBrowser({ width: 360, height: 240 })    // the main window

let count = 0

// await fogu.call("increment", 1) in the page
app.command("increment", fn(by) {
    count += by ?? 1
    app.emit("count", count)          // fogu.on("count", fn) in the page
    return count                      // the value of the page's promise
})

// the definition is complete: show the window (returns at once)
if !app.run() {
    // without a window (plain `fogu main.fg`) there is no page: try the command here
    print("increment(2) →", desktop.call("increment", 2)?)
    print("increment()  →", desktop.call("increment")?)
}
```

```html
<output id="n">0</output>
<button id="plus">+1</button>
<script>
  const n = document.getElementById("n");
  document.getElementById("plus").onclick = () => fogu.call("increment", 1);
  fogu.on("count", (value) => (n.textContent = value));
</script>
```

```text
$ fogu main.fg
increment(2) → 2
increment()  → 3
```

`app.run()` returns `false` without the host (plain `fogu main.fg`), which keeps a
window-less code path. That path lets you check your backend from the
terminal or in CI.

### 2. Commands: payloads, results and errors

Commands are the request/response channel. Whatever the page passes arrives
as a Fogu value, and whatever you return goes back as JSON. Errors are values,
so `return nil, error("...")` rejects the page's promise with that message.

```fogu
// examples/02-commands/main.fg (excerpt)
const app = desktop.app({ name: "commands", title: "Notes (commands)", identifier: "dev.fogu.examples.commands" })
app.createBrowser({ width: 520, height: 560 })

app.command("notes.add", fn(p) {
    if typeof(p) != "map" || typeof(p.title) != "str" || p.title.trim() == "" {
        return nil, error("a note needs a title")     // → the promise rejects
    }
    const tags = match typeof(p.tags) {
        "list" => p.tags.filter((t) => typeof(t) == "str")
        _ => []
    }
    const note = { id: nextId, title: p.title.trim(), tags: tags }
    nextId += 1
    notes.push(note)
    return note                                       // → the promise resolves
})

app.command("notes.list", fn() { return notes })      // no payload: fn() is fine

// a runtime error (int("abc") would stop a normal script) only rejects this call
app.command("parseNumber", fn(text) { return int(text) })
```

```js
async function run(name, payload) {
  try {
    return await fogu.call(name, payload);
  } catch (e) {
    showError(e.message); // "a note needs a title"
  }
}
const note = await run("notes.add", { title: "Buy milk", tags: ["home"] });
```

```text
$ fogu main.fg
{id: 1, tags: [home], title: Buy milk}
{id: 2, tags: [work, urgent], title: Ship v1}
[{id: 2, tags: [work, urgent], title: Ship v1}]
true
[{id: 2, tags: [work, urgent], title: Ship v1}]
notes.add → error: a note needs a title
notes.remove → error: no note 42
parseNumber → error: main.fg:56: runtime error: cannot convert 'abc' to int
missing → error: desktop: no command 'missing'
parseNumber("7") → 7
```

Each call runs in its own task, so a slow command (a network request or a
dialog) does not block other calls.

### 3. Events both ways

Events are fire-and-forget messages and they have no reply. Use them for
anything the backend wants to push (progress, clocks, notifications) and for
UI happenings the backend should react to.

```fogu
// examples/03-events/main.fg (excerpt)
const app = desktop.app({ name: "events", title: "Events", identifier: "dev.fogu.examples.events" })
const win = app.createBrowser({ width: 560, height: 520 })

app.on("set-title", fn(title) {                          // fogu.emit("set-title", "…")
    win.setTitle(str(title ?? "Events"))                 // change the native window
    app.emit("notice", `Window title set to "${title}"`)
})

app.on("ping", fn() {                                    // events without data
    app.emit("pong", { ms: time.now() - started })
})

if app.run() {
    fg fn() {                                            // Fogu → page on its own
        loop {
            time.delay(1s)
            app.emit("clock", { uptime: (time.now() - started) / 1000 })
        }
    }()
}
```

```js
fogu.on("clock", (d) => (uptime.textContent = d.uptime));
fogu.on("*", (name, data) => console.log(name, data)); // every event
fogu.emit("set-title", "My window");

const reply = fogu.once("pong"); // a promise of the next "pong"
fogu.emit("ping");
const { ms } = await reply;
```

Events sent before the page has loaded are queued and delivered once it is
ready, so it is safe to emit from the top level. When there is no window,
`desktop.capture` shows what would reach the host. That is how this example
tests itself:

```text
$ fogu main.fg
desktop:log page loaded: headless
desktop:event {data: Hello from Fogu! The page said hello first., name: notice}
desktop:window {title: My events, window: main}
desktop:event {data: Window title set to "My events", name: notice}
```

### 4. Native dialogs

Use the OS file pickers and message boxes from Fogu, then read and write the
chosen files with `fogu:fs`. Dialogs wait for the user without blocking other
tasks. Call them from commands, `app.on` handlers or `fg` tasks, never
from the top level.

```fogu
// examples/04-dialogs/main.fg (excerpt): a tiny text editor
const app = desktop.app({ name: "dialogs", title: "Text editor (dialogs)", identifier: "dev.fogu.examples.dialogs" })
app.createBrowser({ width: 760, height: 560 })

const textFiles = { "Text": ["txt", "md", "fg", "json"], "All files": ["*"] }

app.command("file.open", fn() {
    const file = app.dialog.open({ title: "Open a text file", filters: textFiles })?
    if file == nil {
        return nil                                  // cancelled
    }
    return { path: file, name: path.basename(file), text: fs.readText(file)? }
})

app.command("file.save", fn(p) {
    let target = p?.path
    if target == nil || p?.saveAs == true {
        target = app.dialog.save({ title: "Save as", fileName: p?.name ?? "untitled.txt", filters: textFiles })?
        if target == nil {
            return nil
        }
    }
    fs.writeText(target, p?.text ?? "")?
    return { path: target, name: path.basename(target) }
})

app.command("confirm.discard", fn() {
    return app.dialog.confirm("Discard the unsaved changes?", {
        title: "Unsaved changes", kind: "warning", ok: "Discard", cancel: "Keep editing",
    })
})
```

Other options are `{ folder: true }` to pick a folder and `{ multiple: true }`
to get a list of paths. Use `desktop.dialog.message(text, { kind: "error" })`
to show a message box. Without the host a dialog returns an error value
instead of hanging:

```text
$ fogu main.fg
file.open → desktop: dialog.open needs the desktop host (build and run the app)
```

### 5. A background task with progress

Long work must not hold up the UI. The command starts an `fg` task and returns
its id straight away. The task reports progress with events and checks a
cancel flag that the page sets with an event.

```fogu
// examples/05-background-task/main.fg (excerpt)
fn runJob(id, total) {
    let sum = 0
    for i in 0..total {
        if cancelled[str(id)] == true {
            cancelled.remove(str(id))
            app.emit("job.done", { id: id, status: "cancelled", result: nil })
            return nil
        }
        sum += step(i)                      // real work; waiting calls let other tasks run
        app.emit("job.progress", { id: id, done: i + 1, total: total, percent: (i + 1) * 100 / total })
    }
    app.emit("job.done", { id: id, status: "finished", result: sum })
    return nil
}

app.command("job.start", fn(p) {
    const id = nextId
    nextId += 1
    fg runJob(id, p?.steps ?? 20)           // runs in the background
    return id                               // the page gets the id immediately
})

app.on("job.cancel", fn(id) { cancelled[str(id)] = true })
```

```js
const id = await fogu.call("job.start", { steps: 30 });
fogu.on("job.progress", ({ id, percent }) => (bars.get(id).value = percent));
fogu.on("job.done", ({ id, status, result }) => finish(id, status, result));
cancelButton.onclick = () => fogu.emit("job.cancel", id);
```

```text
$ fogu main.fg
started job 1
desktop:event {data: {done: 1, id: 1, percent: 25, total: 4}, name: job.progress}
desktop:event {data: {done: 2, id: 1, percent: 50, total: 4}, name: job.progress}
desktop:event {data: {done: 3, id: 1, percent: 75, total: 4}, name: job.progress}
desktop:event {data: {done: 4, id: 1, percent: 100, total: 4}, name: job.progress}
desktop:event {data: {id: 1, result: 14, status: finished}, name: job.done}
started job 2
desktop:event {data: {done: 1, id: 2, percent: 10, total: 10}, name: job.progress}
desktop:event {data: {done: 2, id: 2, percent: 20, total: 10}, name: job.progress}
desktop:event {data: {done: 3, id: 2, percent: 30, total: 10}, name: job.progress}
desktop:event {data: {id: 2, result: nil, status: cancelled}, name: job.done}
```

The run cancels job 2 after 250 ms, so the number of progress lines before the
cancel can vary by one. Fogu tasks are cooperative. A CPU-bound loop should
call `time.delay(0)` now and then so that other calls and events keep flowing.

### 6. Saving data: settings and the window size

Each new build replaces the program folder, so keep the user's files in
`app.dataDir()`. That is a per-user folder named after the app's
`identifier`. This example stores settings as JSON, validates changes, pushes
them to every view and remembers the window size.

```fogu
// examples/06-settings/main.fg (excerpt)
const app = desktop.app({ name: "settings", title: "Settings", identifier: "dev.fogu.examples.settings" })
const file = path.join(app.dataDir(), "settings.json")
const defaults = { theme: "light", fontSize: 14, window: nil }

fn load() {                                    // a missing or damaged file → the defaults
    const [text, err] = fs.readText(file)
    if err != nil {
        return defaults.merge({})
    }
    const [data, perr] = json.parse(text)
    if perr != nil || typeof(data) != "map" {
        return defaults.merge({})
    }
    return defaults.merge(data)                // new keys get their default
}

let settings = load()

// the window opens at the size saved last time
const saved = settings.window ?? {}
const win = app.createBrowser({ width: saved.width ?? 520, height: saved.height ?? 460, maximized: saved.maximized ?? false })

app.command("settings.set", fn(changes) {
    for key, value in changes {
        if !validate(key, value) {
            return nil, error(`invalid setting ${key}: ${value}`)
        }
    }
    settings = settings.merge(changes)
    fs.writeText(file, json.stringify(settings))?
    app.emit("settings", settings)
    return settings
})

app.command("window.remember", fn() {
    const w = win.state()?                     // asks the host: only in commands/handlers/tasks
    settings.window = { width: int(w.width), height: int(w.height), maximized: w.maximized }
    fs.writeText(file, json.stringify(settings))?
    return settings.window
})

app.run()
```

```text
$ fogu main.fg
loaded: {fontSize: 14, theme: light, window: nil}
set:    {fontSize: 16, theme: dark, window: nil}
bad:    invalid setting fontSize: 99
file:   settings.json {"fontSize":16,"theme":"dark","window":null}
reload: {fontSize: 16, theme: dark, window: nil}
```

| OS | `app.dataDir()` (also `desktop.dataDir()`) |
| --- | --- |
| Windows | `%APPDATA%\<identifier>` |
| Linux | `~/.local/share/<identifier>` |
| macOS | `~/Library/Application Support/<identifier>` |
| no host (`fogu main.fg`) | `.desktop-data/` next to the program |

For more than a settings file, `fogu:sql` (SQLite) or `fogu:kv` work the same
way. Put the database file in `app.dataDir()`.

### 7. A complete app: the todo list

[`examples/hello`](examples/hello) puts it all together. It has the commands
`list`, `add`, `toggle`, `remove`, `export` (a native *Save as* dialog) and
`about`. It pushes the events `todos`, `toast` and `uptime` (the last one from
a background task) and handles a `clear-done` event from the page. The list
is saved as JSON in `app.dataDir()`. The logic lives in `todos.fg` as plain
Fogu, separate from the window, and `todos.test.fg` tests it.

```text
$ cd examples/hello
$ fogu main.fg
commands: [about, add, export, list, remove, toggle]
list: {items: [], stats: {done: 0, left: 0, total: 0}}
$ fogu test
...
2 passed — 1 file(s) [38ms]
```

```sh
fogu desktop build                    # dist/hello
fogu desktop build --target windows   # dist/hello.exe, from Linux/WSL
```

Keeping the logic in modules that take plain values and return
`value, error`, as `todos.fg` does, makes it testable with `fogu test`. A
window cannot be tested that way, because `fogu test` stops the signal task
that the library starts on import (see [Tests](#tests)).

### 8. Vite + React + TypeScript

For a real frontend, use a Vite project. [`examples/vite-react`](examples/vite-react)
is exactly what `fogu desktop create myapp --template react` makes.
`fogu desktop dev` gives you hot reload, and `fogu desktop build` runs
`vite build` and embeds the result.

```ts
// web/src/App.tsx (excerpt)
import { useEffect, useState } from "react";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

type Greeting = { message: string };

/** A Fogu event as React state; on() returns its own cleanup. */
function useFoguEvent<T>(name: string, initial: T): T {
  const [value, setValue] = useState<T>(initial);
  useEffect(() => on<T>(name, setValue), [name]);
  return value;
}

export default function App() {
  const count = useFoguEvent<number>("count", 0);
  const [answer, setAnswer] = useState("");

  async function greet(name: string) {
    try {
      const r = await call<Greeting>("greet", name); // Promise<Greeting>
      setAnswer(r.message);
    } catch (err) {
      setAnswer((err as Error).message);              // "type a name first"
    }
  }
  // <button onClick={() => void call<number>("increment", 1)}>+1</button>
  // <button onClick={() => void emit("reset")}>Reset</button>
  // isDesktop() is false in a plain browser tab: calls reject, listeners are no-ops
}
```

```fogu
// main.fg
const app = desktop.app({ name: "vite-react", title: "Fogu Vite React", identifier: "dev.fogu.examples.vite-react" })
app.createBrowser({ width: 960, height: 720, minWidth: 480, minHeight: 480 })

app.command("greet", fn(name) {
    const who = str(name ?? "").trim()
    if who == "" {
        return nil, error("type a name first")
    }
    return { message: `Hello, ${who}! This answer comes from Fogu ${Fogu.version} on ${Fogu.os}.` }
})
```

```text
$ cd examples/vite-react
$ fogu main.fg
commands: [greet, increment]
greet: {message: Hello, Fogu! This answer comes from Fogu 0.7.2 on linux.}
increment: 2
```

```sh
fogu desktop dev                       # bun install (first time), Vite + a debug window with HMR
fogu desktop build                     # tsc && vite build → embedded → dist/vite-react
fogu desktop build --target windows    # dist/vite-react.exe from Linux/WSL
```

### 9. A tray icon, window control and a second window

[`examples/07-tray-windows`](examples/07-tray-windows): closing the window
hides it in the tray (`closeToTray: true` in `desktop.app`), a click on the
icon brings it back, and the menu has a check item, a submenu and Quit.

```fogu
import desktop from "desktop"

const app = desktop.app({ name: "tray-windows", title: "Tray and windows", identifier: "dev.fogu.examples.tray-windows", closeToTray: true })
const win = app.createBrowser({ width: 560, height: 460, minWidth: 420, minHeight: 360 })

let paused = false
const tray = app.tray({
    tooltip: "Tray and windows",           // no icon: the app's icon (or icon: "icons/tray.png")
    clickAction: "show",                   // done by the host, no round trip to Fogu
    menu: [
        { text: "Show", action: "show" },
        "-",
        { id: "pause", text: "Pause updates", checked: false, onClick: fn(e) { paused = e.checked } },
        { text: "Window", items: [
            { text: "Center", onClick: fn() { win.center() } },
            { text: "About…", onClick: fn() { openAbout() } },
        ] },
        "-",
        { text: "Quit", action: "quit" },
    ],
})

fn openAbout() {
    // a second window on another page of the UI (ui/about.html)
    const about = app.createBrowser({ label: "about", url: "about.html", width: 360, height: 240, resizable: false })
    if about.ready() == true {                   // waits until the host has made it
        about.emit("about", { fogu: Fogu.version })   // an event for that window's page only
    }
    return nil
}

app.command("size", fn(p) {
    win.setSize(p.width, p.height)
    win.center()
    return true
})
win.on("resize", fn(e) { app.emit("size", e) })
tray.setTooltip("Ready")
app.run()
```

The page drives its own window with `fogu.window`:

```js
await fogu.window.toggleMaximize();
await fogu.window.setAlwaysOnTop(true);
const { width, height } = await fogu.window.size();
fogu.window.on("moved", (p) => console.log(p.x, p.y));
fogu.tray.on("click", () => console.log("tray icon clicked"));
await fogu.window.of("about").close();
```

### 10. A frameless window

[`examples/08-frameless`](examples/08-frameless) has no system title bar:
`decorations: false` in `createBrowser`, and `"transparent": true` in `fogu.json` (a window
is transparent from its creation, and the host opens the main window before the program
runs). The page draws a bar: the
attribute `data-fogu-drag` makes it move the window (a double click maximizes),
and `data-fogu-window` buttons minimize, maximize and close it, with no script:

```html
<div class="bar" data-fogu-drag>
  <b>My app</b>
  <div>
    <button data-fogu-window="minimize">–</button>
    <button data-fogu-window="maximize">☐</button>
    <button data-fogu-window="close">✕</button>
  </div>
</div>
```

Buttons, links and inputs inside the bar stay clickable (add
`data-fogu-no-drag` to any other element that must not drag). Tauri's own
`data-tauri-drag-region` works too. `fogu desktop create myapp --template
frameless` starts a Vite app with such a bar. Fogu decides whether the window
may close:

```fogu
const win = app.createBrowser({ width: 520, height: 360, decorations: false })

win.onCloseRequested(fn(e) {
    if !dirty {
        return true
    }
    return app.dialog.confirm("Discard the changes?")?   // false keeps the window open
})
```

### 11. Native features: menus, shortcuts, notifications, workers

[`examples/09-native`](examples/09-native) puts the newer parts of the API in
one window:

```fogu
import desktop from "desktop"

const app = desktop.app({ name: "native", title: "Native features", identifier: "dev.fogu.examples.native" })
const win = app.createBrowser({ width: 640, height: 520, minWidth: 480, minHeight: 400 })

let clicks = desktop.kept("clicks", 0)                 // survives a hot reload
const primes = desktop.worker("primes.fg", { threads: 2 })

app.menu([
    { text: "File", items: [
        { text: "Notify me", accelerator: "CmdOrCtrl+N", onClick: fn() { app.notify("Fogu", "Hello from the menu") } },
        "-",
        { role: "quit" },
    ] },
    { text: "Edit", items: [{ role: "undo" }, { role: "redo" }, "-", { role: "cut" }, { role: "copy" }, { role: "paste" }] },
    { text: "View", items: [
        { id: "ghost", text: "See-through window", checked: false, onClick: fn(e) {
            win.set({ opacity: match e.checked {
                true => 0.75
                _ => 1.0
            } })
        } },
    ] },
])

app.shortcut("CmdOrCtrl+Shift+F9", fn(e) {          // also while the app is in the background
    win.show()
    app.emit("shortcut", e.shortcut)
})

app.command("click", fn() {
    clicks += 1
    desktop.keep("clicks", clicks)
    return clicks
})

// CPU-bound: runs in the worker's own runtime, so other commands are not held up
app.command("primes", fn(limit) { return primes.call("count", limit) })

// bytes: a Uint8Array from the page arrives as a buffer, a buffer goes back as a Uint8Array
app.command("reverse", fn(data) {
    const out = []
    for i in 0..len(data) {
        out.push(data[len(data) - 1 - i])
    }
    return bytes(out)
})

app.run()
```

`primes.fg` is a plain module whose top-level `count(limit)` the worker runs.
In the page: `await fogu.call("reverse", new Uint8Array([1, 2, 3]))` resolves
to a `Uint8Array`, and `fogu.call("primes", 2e6, { timeout: 10000 })` gives
up after 10 s.

### 12. The app defined in code

The window of an app can be described in `fogu.json` (`"desktop"`) or in the
program itself, as every example above does. This is
[example 1](#1-the-smallest-app) written both ways.

With the window in `fogu.json` (it keeps working, and a value given in code wins over it):

```json
{
  "name": "counter",
  "version": "0.1.0",
  "entry": "main.fg",
  "desktop": {
    "title": "Counter",
    "identifier": "dev.fogu.examples.counter",
    "width": 360,
    "height": 240,
    "ui": "ui"
  }
}
```

```fogu
import desktop from "desktop"

let count = 0
desktop.command("increment", fn(by) {
    count += by ?? 1
    desktop.emit("count", count)
    return count
})
```

In code ([`examples/10-app-in-code`](examples/10-app-in-code)):

```json
{
  "name": "counter-in-code",
  "version": "0.1.0",
  "entry": "main.fg",
  "desktop": {
    "title": "Counter",
    "identifier": "dev.fogu.examples.counter-in-code",
    "visible": false
  }
}
```

```fogu
import desktop from "desktop"

const app = desktop.app({ name: "counter-in-code", title: "Counter", identifier: "dev.fogu.examples.counter-in-code" })

// the main window: what fogu.json "desktop" would say
const win = app.createBrowser({ width: 360, height: 240, minWidth: 300, minHeight: 200, resizable: true, center: true })

let count = 0
win.on("close", fn(e) { desktop.log("the counter stopped at", count) })

app.command("increment", fn(by) {
    count += by ?? 1
    app.emit("count", count)
    win.setTitle(`Counter — ${count}`)
    return count
})

// a second window, opened from the menu
app.menu([
    { text: "Help", items: [
        { text: "About", onClick: fn(e) {
            app.createBrowser({ label: "about", title: "About", width: 320, height: 160, resizable: false, url: "about.html" })
        } },
    ] },
])

app.run()      // shows the main window; returns at once (false without the host)
```

`fogu main.fg` (no window) runs the command once:

```text
increment(2) → 2
increment()  → 3
```

What stays in `fogu.json`, and why, is in [The app in code](#the-app-in-code).

### 13. React with HeroUI and Tailwind CSS

[`examples/11-heroui`](examples/11-heroui) is a task list whose page is React
with [HeroUI](https://heroui.com) 3 components styled by Tailwind CSS 4. Fogu
keeps the list (saved in `app.dataDir()`), checks every change and pushes the
new list to the page; an error value of a command becomes a HeroUI toast.

```fogu
// main.fg (excerpt)
const app = desktop.app({ name: "heroui", title: "Fogu Tasks", identifier: "dev.fogu.examples.heroui" })
app.createBrowser({ width: 880, height: 680, minWidth: 520, minHeight: 520 })

app.command("tasks.add", fn(p) {
    const title = str(p?.title ?? "").trim()
    if title == "" {
        return nil, error("write what needs to be done")   // → a toast in the page
    }
    const tag = match p?.tag {
        "work" | "home" | "study" => p.tag
        _ => "other"
    }
    const task = { id: state.next, title: title, tag: tag, done: false, created: time.now() }
    state.next += 1
    state.items.push(task)
    changed()?                                           // saves, then app.emit("tasks", …)
    return task
})
```

Tailwind CSS 4 runs as a Vite plugin (`@tailwindcss/vite`, no
`tailwind.config`), and HeroUI's styles are one import:

```css
/* web/src/style.css */
@import "tailwindcss";
@import "@heroui/styles";
@custom-variant dark (&:where(.dark, .dark *));
```

```tsx
// web/src/App.tsx (excerpt)
import { Button, Card, Checkbox, Chip, Label, toast } from "@heroui/react";
import { call, on } from "./fogu-desktop.js";

async function run<T>(name: string, payload?: unknown) {
  try {
    return await call<T>(name, payload);
  } catch (err) {
    toast.danger((err as Error).message);      // the error value of the Fogu command
  }
}

<Checkbox isSelected={t.done} onChange={() => void run("tasks.toggle", t.id)}>
  <Checkbox.Content>
    <Checkbox.Control><Checkbox.Indicator /></Checkbox.Control>
    <Label>{t.title}</Label>
  </Checkbox.Content>
</Checkbox>
```

```text
$ cd examples/11-heroui && fogu main.fg
commands: [tasks.add, tasks.list, tasks.remove, tasks.toggle]
tasks: 1 of 2 done, 50 %
empty title → write what needs to be done
```

The versions are pinned in `web/package.json` (`@heroui/react` 3.2.7,
`tailwindcss` 4.3.5). `fogu desktop dev` installs them on the first run;
`fogu desktop build` runs `vite build` and embeds the result.

## API reference

### Fogu side: `import desktop from "desktop"`

| Function | What it does |
| --- | --- |
| `desktop.command(name, fn(payload))` | Registers a command for `fogu.call(name, payload)`. The return value goes back as JSON. `return nil, error("msg")` or a runtime error rejects the page's promise with `msg`. `fn()` without a parameter also works. `command(name, nil)` removes the command. |
| `desktop.emit(name, data?)` → bool | Sends an event to the page, received by `fogu.on(name, fn(data))` (it is also a Tauri event `name`). Events sent before the page is ready are queued. Returns `false` when there is no host. |
| `desktop.on(name, fn(data))` · `desktop.off(name)` | Handles `fogu.emit(name, data)` from the page. Each call runs in an `fg` task. |
| `desktop.window(opts)` | Changes a window: `"main"`, or the one named by `window`. Options: `title width height x y minWidth minHeight maxWidth maxHeight resizable fullscreen maximized minimized alwaysOnTop center visible focus decorations shadow skipTaskbar closable minimizable maximizable contentProtected`; `minSize: nil` / `maxSize: nil` remove a limit; `action`: `minimize unminimize maximize unmaximize toggleMaximize toggleFullscreen center show hide focus close destroy startDragging reload`. App-wide: `closeToTray`, `minimizeToTray`. |
| `desktop.windowState(label = "main")` → map · error | Returns `{ label, title, width, height, x, y, maximized, minimized, fullscreen, visible, focused, decorated, resizable, alwaysOnTop, scaleFactor }` (logical pixels). |
| `desktop.app(opts?)` → app | The app defined in code: `app.createBrowser(opts)` (a window), `app.run()`, and the functions of this table as `app.command`, `app.tray`, `app.menu`… See [The app in code](#the-app-in-code). |
| `desktop.getWindow(label = "main")` → handle · `desktop.mainWindow` | A window handle, see [Window handles](#window-handles). |
| `desktop.createWindow(label, opts?)` → handle · error | Opens another window on a page of the UI: `opts.url` is a route (`"settings.html"`, `"/settings"`, `"#/about"`) or an `http(s)` URL; the other options are those of `fogu.json` `desktop` (`title width height x y decorations transparent alwaysOnTop resizable…`). A window with that label that is already open comes to the front. Waits for the host. |
| `desktop.windows()` → [labels] · error | The open windows, sorted. |
| `desktop.emitTo(label, name, data?)` | Like `emit`, for the page of one window only (`emit` goes to every window). |
| `desktop.onWindowEvent(type, fn(event))` | Window events of every window, see [Window events](#window-events). |
| `desktop.onCloseRequested(fn(event))` | Decides whether the main window closes: return `false` to keep it open (it may ask with a dialog first). `nil` restores the default. |
| `desktop.tray(opts?)` → tray handle | Creates a tray icon, or changes the one with that `id`. See [Tray icons](#tray-icons). |
| `desktop.removeTray(id = "main")` | Removes a tray icon. |
| `desktop.dialog.open(opts?)` → path · [paths] · nil | Native open dialog. Options: `{ title, directory, filters: { "Images": ["png", "jpg"] }, multiple, folder }`. Returns `nil` when cancelled. |
| `desktop.dialog.save(opts?)` → path · nil | Native save dialog. Options: `{ title, directory, fileName, filters }`. |
| `desktop.dialog.message(text, opts?)` · `desktop.dialog.confirm(text, opts?)` → bool | Message box with `{ title, kind: "info" \| "warning" \| "error" }`. `confirm` also takes `ok` and `cancel` labels and returns whether OK was pressed. |
| `desktop.menu(items, opts?)` | The app menu (the macOS menu bar; on Windows and Linux the menu bar of every window without its own), or a window's own menu with `{ window: "main" }`. See [Menus](#menus). `nil` removes it. |
| `desktop.menuChecked(item, window?)` → bool | Whether a check item of a menu is checked. |
| `desktop.shortcut(accel, fn(event), opts?)` · `desktop.removeShortcut(accel?)` | A global shortcut (`"CmdOrCtrl+Shift+K"`), also while the app is in the background. See [Global shortcuts and notifications](#global-shortcuts-and-notifications). |
| `desktop.notify(title, body?)` · `desktop.notify({ title, body, icon })` | A system notification. |
| `desktop.worker(file, { threads })` → worker | A module in a Fogu runtime of its own (its own thread) for CPU-bound work: `worker.call(fnName, ...args)` → value · error, `worker.stop()`. See [Workers](#workers). |
| `desktop.keep(key, value)` · `desktop.kept(key, default?)` | State that survives a hot reload of the Fogu code (`fogu desktop dev`). See [Hot reload](#hot-reload-and-kept-state). |
| `desktop.toWire(value)` · `desktop.fromWire(value)` | How values cross the page boundary: buffers ↔ `{ "$bytes": base64 }` (done for you in commands and events). |
| `desktop.eval(js, label = "main")` | Runs JavaScript in the page of a window (`"*"`: every window). |
| `desktop.quit(code = 0)` | Closes the application. |
| `desktop.log(...values)` | Writes a line to the host's log: stderr and `<local data dir>/fogu-desktop.log`. A release build on Windows has no console, so `print` output goes nowhere. |
| `desktop.dataDir()` → str | A per-user folder for the app's files (see [example 6](#6-saving-data-settings-and-the-window-size)). It always exists. |
| `desktop.embedded` | `true` inside the executable, `false` under `fogu main.fg`. |
| `desktop.call(name, payload?)` → value · error | Runs a command the way the page would (for tests, or running without a window). |
| `desktop.commandNames()` | The registered commands, sorted. |
| `desktop.capture(fn(name, data))` | For tests: receive the messages meant for the host (`nil` restores the host). See `tests/bridge.fg`. |
| `desktop.version` | The library version. |

Rules:

- **The top level of `main.fg` must return.** Register commands and handlers,
  start `fg` tasks for background work, and let it end. The host serves calls
  once the top level has run, and calls made in the meantime wait. Do not end
  it with `Fogu.signal.wait()` or an endless loop outside a task.
- **Dialogs, `windowState()`, `createWindow`, `windows()`, the window
  getters, `worker.call` and `update.*` wait for the host.** Call them from
  commands, `desktop.on` handlers, window/tray/menu handlers or `fg` tasks, not
  from the top level. `desktop.window`, the window setters, `tray`, `menu`,
  `shortcut`, `notify`, `emit`, `eval`, `log`, `keep` and `kept` need no answer
  and work anywhere, also at the top level.
- **The program's own `.fg` sources are never written to disk** in a built
  app: the host keeps them in memory (`fogu::runtime::vfs`) over the folder
  where the bundle's other files (assets, `fogu.json`, packages) are
  extracted. Imports, `fogu.json` and `Fogu.asset(...)` work as usual. The next
  build replaces that folder, so keep user data in `desktop.dataDir()`.
- The host sets `FOGU_DESKTOP=1`, `FOGU_DESKTOP_DATA_DIR` and
  `FOGU_DESKTOP_PROGRAM_DIR` in the environment. The library reads them with
  `process.env(name)` each time it needs them, so a test can point it
  somewhere else with `process.setEnv` while it runs (`tests/env.test.fg`):

  ```fogu
  import process from "fogu:process"
  import desktop from "desktop"

  process.setEnv("FOGU_DESKTOP_DATA_DIR", "/tmp/my-test-data")
  print(desktop.dataDir())        // /tmp/my-test-data
  ```

### The app in code

`desktop.app(opts)` describes the app in the program instead of `fogu.json`
`"desktop"`; [example 12](#12-the-app-defined-in-code) shows one app written
both ways. `opts`: `name`, `title` (the default title of its windows),
`identifier`, `version`, `icon` (the default tray icon), `closeToTray`,
`minimizeToTray`.

| Function | What it does |
| --- | --- |
| `app.createBrowser(opts?)` → window handle | A window. The first one is the main window (label `"main"`); the others get `opts.label` (default `window-2`, `window-3`…) and open `opts.url`: a route of the UI (`"about.html"`, `"/settings"`, `"#/help"`) or an `http(s)` URL. The other options are the window keys of [`fogu.json`](#fogujson): `title width height x y minWidth minHeight maxWidth maxHeight resizable fullscreen maximized decorations transparent alwaysOnTop center visible skipTaskbar shadow closable minimizable maximizable contentProtected opacity`. It returns at once, also at the top level; the handle is a [window handle](#window-handles) plus `ready()`. |
| `win.ready()` → true · error | Waits until the host has created the window (call it from a command, a handler or an `fg` task). What is set on the handle before that (`setSize`, `setTitle`, `hide`…) is applied when the window exists, so most programs never call it. |
| `win.on("close" \| "closed" \| "resize" \| "move" \| "focus" \| "blur" \| …, fn(event))` | The [window events](#window-events), also under these short names. |
| `app.tray(opts)` · `app.menu(items, opts?)` · `app.shortcut(accel, fn)` · `app.notify(…)` | As `desktop.tray` (with the app's `icon` and `title` as the default icon and tooltip), `desktop.menu`, `desktop.shortcut`, `desktop.notify`. |
| `app.command(name, fn)` · `app.on(name, fn)` · `app.off(name)` · `app.emit(name, data?)` | As `desktop.command`, `on`, `off`, `emit`. |
| `app.getWindow(label)` · `app.windows()` · `app.dialog` · `app.log(…)` · `app.dataDir()` · `app.quit(code?)` | As the functions of `desktop`. |
| `app.run()` → bool | The definition is complete: shows the main window (unless it was created with `visible: false`) and **returns at once** — the host keeps the app alive and the top level of `main.fg` must still end. `false` without the host (`fogu main.fg`). |
| `app.name` · `app.title` · `app.identifier` · `app.version` | What was given to `desktop.app`. |

**What code decides and what `fogu.json` must still say.** `fogu.json` keeps
working exactly as before, and a value given in code wins over it. Two things
happen before the first line of the program runs, so code cannot decide them:

| Needed by | When | Read from `fogu.json` |
| --- | --- | --- |
| `fogu desktop build` | at build time | `name` (the executable's name), `version`, `entry`, `desktop.title` (the name shown by the OS: version information, `.app`, `.desktop` entry, installer), `desktop.identifier`, `desktop.ui` / `vite` / `outDir`, `desktop.features`, `desktop.workers`, `desktop.update`, `desktop.beforeBuild`, and the files in `icons/` |
| the host | when the app starts | `desktop.identifier` (it names the data folders) and the main window, which the host opens from `"desktop"` — or from its defaults (960×680, titled with the app's name) — before the program runs |

So an app defined in code still has a `fogu.json` with `name`, `version` and,
under `"desktop"`, `title` and `identifier`; everything else can move to
`main.fg`. `app.run()` writes a line to the log when `identifier` in code
differs from the one the app runs with.

The options of the **main** window given to `createBrowser` are applied to
the window the host has already opened: all of them but `transparent` and
`url` (a window is transparent, or not, from its creation; the main window
opens `index.html`), which the main window takes from `fogu.json` only — the
other windows take them from code. Because the window exists first, it would
be seen for a moment at the size of `fogu.json` (or the default). Put
`"desktop": { "visible": false }` in `fogu.json` and `app.run()` shows it once
it is set up, as the example does.

### Window handles

`desktop.getWindow(label)` (and `desktop.mainWindow`, `desktop.createWindow`,
`app.createBrowser`) return a map of functions for one window. Setters return at once (they need
no answer); getters wait for the host and return `value, error`.

| Function | What it does |
| --- | --- |
| `w.label` | The window's label (`"main"` for the first window) |
| `w.set(opts)` | `desktop.window(opts)` for this window |
| `w.state()` · `w.size()` · `w.position()` · `w.title()` | The state (as `windowState`), `{ width, height }`, `{ x, y }`, the title |
| `w.isMaximized()` · `isMinimized()` · `isFullscreen()` · `isVisible()` · `isFocused()` | bool · error |
| `w.setSize(w, h)` · `setPosition(x, y)` · `center()` | Logical pixels |
| `w.setMinSize(w, h)` · `w.setMaxSize(w, h)` | Size limits; without arguments the limit is removed |
| `w.setTitle(t)` · `setResizable(b)` · `setDecorations(b)` · `setAlwaysOnTop(b)` · `setFullscreen(b)` · `setSkipTaskbar(b)` | `b` defaults to `true` |
| `w.minimize()` · `unminimize()` · `maximize()` · `unmaximize()` · `toggleMaximize()` · `toggleFullscreen()` | |
| `w.show()` · `hide()` · `focus()` | `show` also unminimizes and focuses |
| `w.close()` · `w.destroy()` | `close` asks first (`onCloseRequested`, close-to-tray); `destroy` does not |
| `w.startDragging()` · `w.reload()` | Move the window with the mouse (from a mousedown); reload the page |
| `w.eval(js)` · `w.emit(name, data?)` | JavaScript / an event for this window's page |
| `w.on(type, fn(event))` · `w.off(type)` | This window's events, see below. Short names: `close` = `closeRequested`, `closed` = `destroyed`, `resize` = `resized`, `move` = `moved` |
| `w.onCloseRequested(fn(event))` | Return `false` to keep the window open; `nil` restores the default |

### Window events

`w.on(type, fn)`, `desktop.onWindowEvent(type, fn)` (every window) and, in the
page, `fogu.window.on(type, fn)` receive `{ window, type, … }`:

| Type | Data |
| --- | --- |
| `resized` | `width`, `height` (logical; a minimized window on Windows reports 0×0) |
| `moved` | `x`, `y` |
| `focus` · `blur` | |
| `closeRequested` | `prevented` (whether the window stays open), `decides` (`"program"`, `"page"` or `"none"`) |
| `destroyed` | |
| `scaleChanged` | `scaleFactor` |
| `themeChanged` | `theme` (`"light"` / `"dark"`; not on Linux) |
| `fileDrop` | `paths`: files dropped on the window |
| `*` | every type |

The host only forwards the types somebody listens to. **Closing**: when Fogu
registered `onCloseRequested` for a window, Fogu decides (its handler runs,
and the window is destroyed unless it returned `false`); otherwise, when the
page registered `fogu.window.onCloseRequested`, the page decides; otherwise
the main window hides in the tray when `closeToTray` is on and a tray icon
exists; otherwise it closes. Closing the last window ends the app.

### Tray icons

```fogu
const t = desktop.tray({
    id: "main",                        // several trays need several ids
    icon: "icons/tray.png",            // .png or .ico; relative to the app folder; default: the app icon
    tooltip: "My app",
    title: "3",                        // text next to the icon (macOS, some Linux panels)
    menu: [
        { text: "Open", action: "show" },               // action: show | hide | toggle | quit (host side)
        { id: "sync", text: "Sync now", onClick: fn(e) { sync() } },
        { id: "dark", text: "Dark mode", checked: true, onClick: fn(e) { setDark(e.checked) } },
        { text: "More", items: [{ text: "About", enabled: false }] },   // a submenu
        "-",                                            // a separator
        { text: "Quit", action: "quit", accelerator: "CmdOrCtrl+Q" },
    ],
    onClick: fn(e) { desktop.mainWindow.show() },      // also onDoubleClick, onRightClick, onMenu
    clickAction: "toggle",             // or done by the host: "show" | "toggle" | "hide" (also doubleClickAction)
    showMenuOnLeftClick: false,        // default: the menu opens on a right click
    closeToTray: true,                 // the main window hides instead of closing (also minimizeToTray)
})
t.setTooltip("Syncing…")
t.setMenu([...])                       // replaces the menu; t.setMenu(nil) removes it
t.setIcon("icons/busy.png") · t.setTitle(text) · t.setVisible(false) · t.update({ … })
t.on("menu", fn(e) { desktop.log(e.item, e.checked) })   // "click" "doubleClick" "rightClick" "middleClick" "menu" "*"
t.isChecked("dark")                    // the check item's state, kept up to date
t.remove()
```

- Menu item ids default to their text. Events are `{ tray, type, item,
  checked }`; the page receives them too: `fogu.tray.on("menu", fn)`.
- `desktop.tray` needs no answer, so it works at the top level of `main.fg`.
  A bad icon path is written to the log (`desktop.log` file) and sent to the
  page as the event `desktop:error`.
- Put tray icons in the app's `icons/` folder: `fogu desktop build` embeds it.
- **Linux:** the icon shows its menu, but clicks on the icon itself are not
  delivered (a limitation of the Linux tray protocol). A tray with `onClick`
  or `clickAction` therefore gets a first menu item, "Open" (change it with
  `linuxClickItem: "Show window"`, or `false` for none), that fires the same
  click event and action. The tray needs `libayatana-appindicator3` (or
  `libappindicator3`) at run time.
- **Windows 11** hides new tray icons in the overflow (^) until the user pins them.

### Menus

```fogu
desktop.menu([
    { text: "File", items: [
        { text: "Open…", accelerator: "CmdOrCtrl+O", onClick: fn() { openFile() } },
        { id: "autosave", text: "Autosave", checked: true },
        "-",
        { role: "quit" },
    ] },
    { text: "Edit", items: [{ role: "undo" }, { role: "redo" }, "-", { role: "copy" }, { role: "paste" }] },
], { onMenu: fn(e) { desktop.log("menu", e.item, e.checked) } })

desktop.menu([{ text: "Tools", items: [{ text: "Run" }] }], { window: "editor" })   // one window's own menu
desktop.menuChecked("autosave")                                                   // a check item's state
```

The items are the tray menu's (`text id enabled checked accelerator items
action onClick`, `"-"` a separator) plus `{ role }` for the system items:
`copy cut paste selectAll undo redo minimize maximize fullscreen hide
hideOthers showAll closeWindow quit services about separator`. On macOS the
top-level items must be submenus, and an Edit menu with the clipboard roles is
what makes Cmd+C/Cmd+V work in the page. A click goes to `onClick`, to
`onMenu`, and to the pages as `fogu.menu.on(fn)` with `{ menu, item, checked }`
(`menu`: the window's label, or `"app"`). The page can set a menu too:
`await fogu.menu.set(items, { window })` (without functions: listen with
`fogu.menu.on`).

### Global shortcuts and notifications

```fogu
desktop.shortcut("CmdOrCtrl+Shift+Space", fn(e) { desktop.mainWindow.show() })
desktop.shortcut("F8", fn(e) { desktop.log(e.state) }, { released: true })      // also the releases
desktop.removeShortcut("F8")                                                     // nil: all of them
desktop.notify("Export finished", "report.pdf is in Downloads")
```

A global shortcut works while the app is in the background. When another
program owns the combination, the error goes to the log and to the pages'
`desktop:error` event. Pages register their own with
`fogu.shortcut.register(accel, fn)` and hear every shortcut with
`fogu.shortcut.on(fn)`. Notifications use the system's (Windows toast,
macOS Notification Center, Linux notification daemon); `fogu.notify(title,
body)` shows one from the page. On Windows a toast of an app that is not
installed (no `--installer`) is attributed to the host's identity.

### Workers

The Fogu program is one cooperative thread: a CPU-bound loop in a command
holds up the other commands and events until it ends. A **worker** runs a
module in a Fogu runtime of its own, on its own thread:

```fogu
// stats.fg — a plain module; its top-level functions are what a worker runs
fn histogram(values, bins) { ... }

// main.fg
const stats = desktop.worker("stats.fg", { threads: 4 })        // 4 runtimes, calls go round robin
desktop.command("histogram", fn(p) { return stats.call("histogram", p.values, 20) })
```

- Arguments and results cross as JSON (a worker shares nothing with the
  program). An error value or a runtime error in the function comes back as
  the error of `call`.
- The runtimes start on the first `call` (so `desktop.worker(...)` works at
  the top level) and stop with `worker.stop()` or when the app quits.
- A worker module may `import desktop from "desktop"` and use `desktop.emit`
  (to the pages, e.g. progress) and `desktop.log`.
- Worker files are bundled automatically when the path is a literal
  (`desktop.worker("stats.fg")`); list computed ones in `fogu.json`
  `"desktop": { "workers": ["jobs/a.fg"] }`.
- Without the host (`fogu main.fg`, tests) `call` runs the function in this
  program through `fogu:vm`, so the code stays testable.

### Hot reload and kept state

Under `fogu desktop dev`, a change of a `.fg` file or `fogu.json` reloads the
program **in place**: a new Fogu runtime runs `main.fg` again while the
windows and pages stay. Calls in flight are rejected with "the Fogu program
was reloaded", and the pages get the event `desktop:reloaded`
(`fogu.onReload(fn)`). The new program starts from scratch except for what
the old one kept:

```fogu
let todos = desktop.kept("todos", [])          // the kept value, or the default
fn save() { desktop.keep("todos", todos) }     // call it after every change
```

Kept values cross as JSON and last until the app quits (a new run starts
empty). Trays, menus and shortcuts set by the new program replace the old
ones. A change in a plain `ui/` folder reloads the pages; a Vite UI keeps
its HMR. `fogu desktop dev --restart` restarts the whole window instead.

### Self-update

```sh
fogu desktop keygen --endpoint https://example.com/myapp/latest.json   # once: .fogu-update.key + fogu.json desktop.update
fogu desktop build                                                      # dist/myapp.exe + dist/latest.json (signed)
```

Upload `dist/<name>` and `dist/latest.json` side by side to the endpoint (any
static host). Builds for other platforms of the same version add their entry
to the same `latest.json`. In the app:

```fogu
import update from "desktop/update.fg"

desktop.command("checkUpdate", fn() { return update.check() })   // nil, or { version, current, notes, url, size }
desktop.command("installUpdate", fn() { return update.install() })   // download, verify, replace, restart
```

| Function (`import update from "desktop/update.fg"`) | What it does |
| --- | --- |
| `update.check()` → map · nil · error | Fetches `desktop.update.endpoint`; `nil` when the app is up to date, else `{ version, current, notes, date, url, sha256, signature, size }` for this platform. Waits for the network: call it from a command or a task. |
| `update.install(update?, { restart })` → bool · error | Downloads the update (`check()`'s answer, or checks now), verifies the SHA-256 and the signature, replaces the executable and restarts (`restart: false` leaves the restart to you). `false` when there is nothing to install. |
| `update.compareVersions(a, b)` → int | `1` when `a` is newer, `-1` older, `0` equal (`"1.2.10"` > `"1.2.9"`; a pre-release is older than its release). |
| `update.platform()` → str | The key of this platform in `latest.json`: `windows-x86_64`, `linux-x86_64`, `macos-aarch64`… |
| `update.keypair()` · `update.sign(file, privateKey, url)` | The build side (what `fogu desktop keygen` and `build` use): an Ed25519 key pair; a `latest.json` entry `{ url, sha256, signature, size }` for an executable. |

`install` checks the download's SHA-256 and its Ed25519 signature against
`desktop.update.pubkey` in `fogu.json` before anything is replaced; then the
running executable is renamed to `<exe>.old` (removed at the next start), the
new one takes its place and the app restarts. A single-file app carries the
program and the UI, so everything updates together. Keep
`.fogu-update.key` secret (keygen adds it to `.gitignore`); on CI pass the
private key as `FOGU_DESKTOP_UPDATE_KEY`. `latest.json`:
`{ version, notes, date, platforms: { "windows-x86_64": { url, sha256,
signature, size }, … } }`; `notes` and `date` come from
`desktop.update.notes` / `FOGU_DESKTOP_UPDATE_NOTES` and
`FOGU_DESKTOP_UPDATE_DATE`. `update.fg` is a separate module because it
needs `fogu:http` and `fogu:security` (the host's default `network` area).

### Binary data and call timeouts

The page boundary is JSON, but bytes cross both ways at any depth: a Fogu
buffer arrives in the page as a `Uint8Array`, and a `Uint8Array`,
`ArrayBuffer` or `DataView` sent by the page arrives as a buffer (on the wire
`{ "$bytes": "<base64>" }`). Functions, struct methods and channels still do
not cross: give the page an id and a command instead.

`fogu.call(name, payload, { timeout: 5000 })` rejects with a `TimeoutError`
after 5 s, and `{ signal }` takes an `AbortSignal` (`AbortError`);
`fogu.callTimeout = 10000` sets a default for every call. The Fogu command
itself keeps running (a Fogu task cannot be killed from outside); its late
answer is dropped.

### Page side: `window.fogu`

The host injects `client/fogu.js` into every page before the page's own
scripts run. It is also injected into the Vite dev server's page under
`fogu desktop dev`. There is nothing to include or import.

| Function | What it does |
| --- | --- |
| `await fogu.call(name, payload?, { timeout, signal }?)` | Runs the Fogu command and resolves to its value. It rejects with an `Error` whose message is the Fogu error's message, a `TimeoutError` after `timeout` ms, or an `AbortError` when `signal` aborts. Bytes cross as `Uint8Array` ↔ buffer. |
| `fogu.callTimeout` | The default timeout of `fogu.call` in ms (`0`: none). |
| `fogu.menu.on(fn)` · `await fogu.menu.set(items, { window })` | Menu bar clicks `{ menu, item, checked }`; set a menu from the page. |
| `await fogu.shortcut.register(accel, fn)` · `unregister(accel?)` · `on(fn)` | Global shortcuts. |
| `await fogu.notify(title, body?)` | A system notification. |
| `fogu.onReload(fn)` → unsubscribe | After a hot reload of the Fogu code (`fogu desktop dev`). |
| `fogu.on(name, fn)` → unsubscribe | Listens for events from `desktop.emit(name, data)`. `"*"` receives `(name, data)` for every event. |
| `fogu.off(name, fn?)` | Removes a listener, or all the listeners of `name`. |
| `await fogu.once(name)` | Resolves with the next event of that name. |
| `await fogu.emit(name, data?)` | Fires `desktop.on(name, fn)` in Fogu. |
| `window.addEventListener("fogu:ready", …)` | Fires once the host has delivered the queued events. |
| `fogu.window.*` | This window: the same functions as a [Fogu window handle](#window-handles) (`setSize`, `toggleMaximize`, `state`, `on`, `onCloseRequested`…), all returning promises. `fogu.window.label` is its label. |
| `fogu.window.of(label)` · `await fogu.window.create(label, opts)` · `await fogu.window.list()` | Another window; a new window (as `desktop.createWindow`); the open windows. |
| `fogu.tray.on(type, fn)` | Tray events (`click`, `doubleClick`, `rightClick`, `menu`, `*`). `fogu.tray.set(opts)` / `remove(id)` change trays from the page (no functions in the menu: listen to `menu`). |
| `data-fogu-drag` (or `fogu-drag`) | On any element: dragging it moves the window, a double click toggles maximize. Interactive children (buttons, links, inputs, `data-fogu-no-drag`) are excluded. |
| `data-fogu-window="minimize\|maximize\|close\|fullscreen\|hide"` | On a button: does that to the window. `close` asks first, like the system's close button. |

If the program fails to start (for example, a syntax error in `main.fg`),
every call rejects with the error and the page shows it in a red box. The
Tauri globals (`window.__TAURI__`) are available too, with the `core:default`
permissions (`runtime/capabilities/default.json`).

### TypeScript and the ES module

`client/fogu-desktop.js` and `client/fogu-desktop.d.ts` wrap `window.fogu` as
a typed ES module. `fogu desktop create` copies them to `web/src/`. For an
existing project, copy them by hand.

```ts
import { call, on, off, once, emit, ready, isDesktop, appWindow, tray, menu, shortcut, notify, onReload } from "./fogu-desktop.js";

const r = await call<{ message: string }>("greet", "Ada");   // Promise<T>
const stop = on<number>("count", (n) => console.log(n));    // returns the unsubscribe function
await emit("reset");
await ready();   // the queued events have been delivered
isDesktop();     // false in a plain browser tab: calls reject, listeners are no-ops
await appWindow.setSize(800, 600);                         // fogu.window, typed (FoguWindows)
tray.on("menu", (e) => console.log(e.item));
const bytes = await call<Uint8Array>("thumbnail", fileBytes, { timeout: 5000 });   // Uint8Array both ways
menu.on((e) => console.log(e.menu, e.item));
await shortcut.register("CmdOrCtrl+K", () => openPalette());
```

The `.d.ts` also declares the global (`window.fogu: FoguDesktop` and the
`fogu:ready` event), so code that uses `window.fogu` directly type-checks.

### CLI

```sh
fogu desktop create <dir> [--template vanilla|ts|react|vue|svelte|frameless] [--title "My App"] [--identifier com.example.myapp] [--install] [--force]
fogu desktop dev   [dir] [--port 5173] [--host 127.0.0.1] [--target windows|native] [--features a,b] [--cargo] [--no-launch] [--no-watch] [--restart] [--rebuild-host]
fogu desktop build [dir] [--target <triple>|windows] [--debug] [--zip] [--nsis] [--no-stamp] [--features a,b] [--cargo] [--rebuild-host] [--installer]
fogu desktop build [dir] --android|--ios [--apk] [--aab] [--mobile-target aarch64,…] [--debug] [--features a,b] [--check] [--reinit]
fogu desktop ci [dir] [--os windows,macos,linux] [--installer] [--features a,b] [--force]   # a GitHub Actions workflow that builds every OS
fogu desktop keygen [dir] [--endpoint URL] [--force]   # the signing keys of self-updates
fogu desktop update        # install the latest desktop library
fogu desktop path          # where the library is
```

`[dir]` defaults to the current folder. `create` defaults to the `react`
template (`frameless` is a vanilla Vite app in a window without the system
title bar) and needs a new or empty folder (or `--force`). The templates ship
with the library, so creating an app needs no network. Installing the
packages does, either with `--install` or on the first `dev`/`build`.

### `fogu.json`

```
my-app/
  fogu.json        name, version, entry, and "desktop" (below)
  main.fg          the backend (and any other .fg modules, data files…)
  ui/index.html    the frontend (plain files), or web/ for a Vite project
  icons/           optional: icon.png (RGBA, 512x512 or more) and/or icon.ico — the app's icon;
                   --installer also wants 32x32.png and 128x128.png
```

| `desktop` key | Default | Meaning |
| --- | --- | --- |
| `title` | `name` | Window title and product name (what the OS shows for the executable) |
| `company`, `copyright` | `author` of fogu.json, none | The company and copyright written into the executable's version information (Windows), `Info.plist` (macOS) and the NSIS installer |
| `identifier` | `dev.fogu.app.<name>` | Reverse-DNS id. It names the data folders (and the WebView2 profile), so give each app its own. |
| `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`, `x`, `y`, `center`, `resizable`, `maximized`, `fullscreen`, `alwaysOnTop`, `visible`, `skipTaskbar`, `shadow`, `closable`, `minimizable`, `maximizable`, `contentProtected` | 960×680, resizable, centered | The main window at start (also the options of `desktop.createWindow`) |
| `decorations` | `true` | `false`: no system title bar and border (a [frameless window](#10-a-frameless-window)) |
| `transparent` | `false` | `true`: the page's transparent areas show the desktop (rounded corners, custom shapes). On macOS this uses the NSWindow private API (`macos-private-api`, enabled for macOS builds): Apple's App Store rejects apps that use it, so an App Store build needs `--installer` with `"macOSPrivateApi": false` |
| `opacity` | `1` | The whole window's opacity, `0`–`1` (also `desktop.window({ opacity })`, `fogu.window.set({ opacity })`). Native per OS: a layered window on Windows, `NSWindow.alphaValue` on macOS, `gtk_widget_set_opacity` on Linux (needs a compositor). |
| `closeToTray`, `minimizeToTray` | `false` | Closing / minimizing the main window hides it while a tray icon exists |
| `ui` | `"ui"` | The UI folder: plain files with an `index.html`, or a Vite project |
| `vite` | detected | `true`: the `ui` folder is a Vite project (it is built and its `outDir` embedded). `false`: never treat it as one. Absent: it is a Vite project when the folder has a `vite.config.*` or `vite` in `package.json`. |
| `outDir` | what Vite resolves | The Vite build's output folder, relative to `ui`. The build runs the Vite config through Vite itself (`resolveConfig`, with bun or node), so computed values work; this key overrides it. |
| `workers` | found in the sources | Worker modules to bundle besides those named by a literal `desktop.worker("x.fg")` |
| `update` | none | `{ endpoint, pubkey, notes }` for [self-update](#self-update) (`fogu desktop keygen` writes it) |
| `macOSPrivateApi` | `true` | `false`: an `--installer` build for macOS without the private API (no transparent windows; for the App Store) |
| `beforeBuild` | none | A command run in the app folder before building, e.g. `"bun run build"` |
| `features` | `["network"]` | The Fogu standard library areas of the host: `network database messaging ops ai media rpc` or `full`. The base modules (`fs json path process time regex crypto csv yaml vm`…) are always there. Used by `build` and by `dev` (`--features` on the command line overrides it). Both look for a prebuilt host that has the areas and compile one with cargo only when there is none; both warn when the app imports a module of an area that is not listed (see below). |

When the program imports a `fogu:` module whose area the host does not have,
the host stops with `unknown built-in module: 'fogu:sql'`. `dev` and `build`
say which area that is before they start:

```text
warning: main.fg imports fogu:sql, which is in the "database" area: the host does not have it (its areas: network)
warning: add the area: "desktop": { "features": ["network","database"] } in fogu.json, or --features network,database
```

Packages: the app's `fogu_modules/` folder (vendored packages) is embedded
with the program. Packages that live only in the global store (`~/.fogu`) are
not, so vendor them into `fogu_modules/<name>` first. The desktop library
itself is always embedded.

## Building and shipping

```sh
fogu desktop build                     # dist/<name>: the prebuilt host + the app, in seconds, no Rust
fogu desktop build --target windows    # dist/<name>.exe from Linux/macOS/WSL, with its icon and name
fogu desktop build --zip               # also dist/<name>-<version>-<platform>.zip (.tar.gz on Linux)
fogu desktop build --nsis              # also a Windows installer (NSIS), from any OS
fogu desktop build --debug             # the debug host: console + devtools (right click → Inspect); needs Rust
fogu desktop build --cargo             # compile the host instead of using a prebuilt one (needs Rust)
fogu desktop build --rebuild-host      # compile the generic host again (after a Fogu update)
fogu desktop build --installer         # the Tauri CLI's installers (.msi, .deb, .rpm, .AppImage, .dmg); needs Rust
```

Real output of a Windows build made on Linux, with no Rust installed
(`examples/hello` renamed, plus `--zip --nsis`):

```text
host: the prebuilt release host for windows-x64 (areas: network): /home/ana/.fogu/fogu_modules/desktop/hosts/windows-x64/fogu-desktop.exe
identity: Notas Çã 1.4.2 (icon, name and version written into the executable)
building notas 1.4.2 (com.example.notas): the app appended to the release host
> fogu build --compile main.fg --runtime /home/ana/notas/dist/.host/fogu-desktop.exe -o /home/ana/notas/dist/.bundle --embed ui/** --embed .fogu-desktop.json --embed icons/**   (in /home/ana/notas)
+ fogu.json
  + fogu_modules/desktop/main.fg
  …
  + main.fg
  + todos.fg
  + 5 embedded file(s) (12.3 KB)
built /home/ana/notas/dist/.bundle/notas (42.3 MB, 55 file(s), 1242ms)
executable: /home/ana/notas/dist/notas.exe
archive:    /home/ana/notas/dist/notas-1.4.2-windows-x86_64.zip
installer script: /home/ana/notas/dist/notas-setup.nsi (NSIS is not installed: get it from https://nsis.sourceforge.io or `apt install nsis`, then `makensis notas-setup.nsi`)
```

### Where the host comes from

The *generic host* is the Tauri window with the Fogu runtime and no app
inside. `build` and `dev` look for one in this order and say which they use:

1. **the cache**, `~/.fogu/desktop/hosts/<lib version>/<target>/<profile>-<areas>/`:
   hosts compiled on this machine before;
2. **a prebuilt host** (a release build): `$FOGU_DESKTOP_HOSTS` if it is set
   (your own builds, a team share, a CI cache), then `hosts/` of the library
   — the package comes with `windows-x64`, `linux-x64`, `macos-arm64` and
   `macos-x64`.
   The folder name says what the host has: `<os>-<arch>/` the default areas
   (`network`), `<os>-<arch>-database+network/` those areas,
   `<os>-<arch>-full/` all of them; the smallest host that has every area the
   app asks for is taken;
3. **cargo**: `runtime/` compiled with `FOGU_DESKTOP_GENERIC=1`, then cached
   (1). This is the only step that needs Rust and the webview development
   packages ([Requirements](#requirements-per-os)). It happens when there is
   no prebuilt host for the platform and areas, for a debug host
   (`build --debug`), with `--cargo` and with `--rebuild-host`. Without cargo
   the command stops and says what it looked for:

```text
desktop build: the desktop host must be compiled (no prebuilt host for linux-arm64 with the areas network), which needs Rust, and cargo is not installed. Prebuilt hosts: none for linux-arm64 in /home/ana/.fogu/fogu_modules/desktop/hosts. Use the areas of a prebuilt host ("desktop.features" in fogu.json), put a host in $FOGU_DESKTOP_HOSTS/linux-arm64/, or install Rust (https://rustup.rs)
```

`fogu desktop dev` prefers a debug host from the cache (it has a console and
the devtools) and otherwise runs on the prebuilt release host:

```text
host: the prebuilt release host for windows-x64 (areas: network): /home/ana/.fogu/fogu_modules/desktop/hosts/windows-x64/fogu-desktop.exe
note: this host is a release build: no devtools (right click → Inspect); `fogu desktop dev --cargo` compiles the debug host (needs Rust)
```

### The app's name and icon

A built app carries its identity from `fogu.json` — `name`, `version`,
`desktop.title`, `desktop.identifier`, `desktop.company`, `desktop.copyright`
— and its icon from `icons/icon.ico` or `icons/icon.png`. Nothing is compiled
for it:

| Target | What the build writes |
| --- | --- |
| **Windows** | Into `dist/<name>.exe` itself: the icon (from `icons/icon.ico`, else made from `icons/icon.png` in 16, 24, 32, 48, 64 and 256 pixels) and the version information — `FileDescription` and `ProductName` = the title, `CompanyName`, `FileVersion` / `ProductVersion` = the version, `InternalName`, `OriginalFilename` = `<name>.exe`, `LegalCopyright`. Explorer, the task bar's menu, the Task Manager and the file's Properties show them. `--no-stamp` leaves the host as it is. |
| **macOS** | `dist/<Title>.app`: `Contents/MacOS/<name>`, `Contents/Info.plist` (`CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`, `CFBundleExecutable`, `CFBundleIconFile`, the versions) and `Contents/Resources/<name>.icns` made from `icons/icon.png`. `dist/<name>` stays next to it. |
| **Linux** | `dist/share/applications/<name>.desktop` (`Name=`, `Icon=`, `Exec=`) and `dist/share/icons/hicolor/<size>/apps/<name>.png` (32, 48, 128, 256). Copy `share/` over `~/.local/share` (or `/usr/share`) and the executable to a folder on the `PATH`. |

The Windows part is done by `tools/pe.fg`, a resource editor for `.exe` files
written in Fogu, so it works the same when the Windows app is built on Linux
or macOS. You can use it on its own:

```fogu
import pe from "desktop/tools/pe.fg"

pe.stamp("fogu-desktop.exe", "notas-host.exe", {
    icon: "icons/icon.ico",
    version: { title: "Notas", name: "notas", version: "1.4.2", company: "ACME Lda" },
})?
const info = pe.inspect("notas-host.exe")?
print(info.strings.ProductName, info.fileVersion, len(info.icons))    // Notas 1.4.2.0 6
```

The identity is written into a **copy of the host first**, and the app is
appended to that copy: changing the resources of an executable drops what is
appended to it, so the order matters (`pe.stamp` refuses an executable that
already has an app). The checksum field of the `.exe` is set to 0 (Windows
checks it only for drivers) and a code signature of the host would be
dropped: sign `dist/<name>.exe`, the final file.

### Packages to hand out

| Flag | Result | Needs |
| --- | --- | --- |
| `--zip` | `dist/<name>-<version>-<platform>.zip` with the executable (the `.app` on macOS); for Linux a `.tar.gz` with the executable and `share/` | nothing |
| `--nsis` | `dist/<name>-setup.nsi`, a per-user Windows installer (no administrator rights: `%LOCALAPPDATA%\Programs\<name>`, Start menu and desktop shortcuts, an entry in "Apps" with an uninstaller) and, when NSIS is installed, `dist/<name>-<version>-setup.exe` | `makensis` on the PATH to compile it (`apt install nsis`, `brew install makensis`, or [nsis.sourceforge.io](https://nsis.sourceforge.io) on Windows); without it only the script is written |
| `--installer` | The Tauri CLI's bundles: `.msi` and `.exe` setup, `.deb`, `.rpm`, `.AppImage`, `.dmg` | Rust, the Tauri CLI (`cargo install tauri-cli --version "^2"`), the toolchain of the target |

### Notes

- **How an executable is made.** The app is appended to a copy of the host by
  `fogu build --compile --runtime <host>`: the program, the packages it imports
  and the built UI (`--embed`), in the same bundle format as any standalone
  Fogu executable. At start the host extracts the bundle once (to the temp
  folder, by its hash), runs the program from there and serves the UI over its
  own `fogu://` protocol.
- The output is `<app-dir>/dist/<name>` (`<name>.exe` for Windows) as a single
  file, also when it is cross-compiled from Linux.
- **The app is running while you build.** Windows does not let a running
  `.exe` be overwritten. The build says so before it starts
  (`warning: dist/notas.exe is in use…`), then renames the running copy to
  `<name>.exe.old` and writes the new one in its place; if even that fails the
  new executable is `dist/<name>.new.exe`. With `--installer` the installers
  are copied first, so they are there whatever happens to the `.exe`.
- **Windows built from Linux (`--target windows` = `x86_64-pc-windows-gnu`):**
  the GNU toolchain cannot link the WebView2 loader statically, so the host
  embeds `WebView2Loader.dll` and delay-loads it: on first start it uses a copy
  next to the `.exe` if there is one, otherwise it extracts its own to
  `%LOCALAPPDATA%\fogu\desktop\webview2\` (see `runtime/webview2/README.md`).
  Nothing has to be shipped with the `.exe`.
- Compiling a host builds Tauri and the Fogu runtime, which takes several
  minutes, once. Installed from the server, that build downloads the Fogu
  sources (`<server>/download/<version>/fogu-src.tar.gz`, into `~/.fogu/src/`;
  or set `FOGU_SRC` to a checkout of `lang/`) and keeps the Rust build in
  `~/.fogu/desktop/<version>/runtime/`.
- `--installer` stages the app into the crate and compiles it in with the
  Tauri CLI (`cargo tauri build … -- --no-default-features` when
  `desktop.features` is set). Only one such build at a time per library
  folder, because the app is staged in `runtime/.bundle/`.
- A Vite UI is built first. That needs **bun** (or Node.js/npm) on the build
  machine. The executable does not.
- Debug builds have no debug info (`[profile.dev] debug = false`), because
  with it the Windows/Tauri crates need several GB of RAM to compile.
- **The whole desktop package is inside the executable.** `fogu build
  --compile` embeds every `.fg` file of a package the program imports, so the
  library's own tools, tests and examples travel with each app: 52 files,
  about 290 KB of text, of which the app uses `main.fg`, `update.fg` and
  `fogu.json` (46 KB). The library cannot leave the rest out: that needs a
  `"files"` / `"exclude"` list for packages in Fogu itself.

### Requirements per OS

Building an app needs **nothing but Fogu** when a prebuilt host fits (Windows
x64, Linux x64, macOS arm64 and macOS x64 with the default areas). The "to compile a host"
column is for the other cases: another platform or set of areas, `--debug`,
`--cargo`, `--rebuild-host`, `--installer`.

| OS | To compile a host | To run the executable |
| --- | --- | --- |
| **Windows** (MSVC) | Rust (`stable-x86_64-pc-windows-msvc`), Visual Studio Build Tools (C++ workload) | The **Microsoft Edge WebView2 Runtime**: preinstalled on Windows 11 and up-to-date Windows 10, otherwise the [Evergreen runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (or ship its bootstrapper with `--installer`) |
| **Windows from Linux** (`--target windows`) | `rustup target add x86_64-pc-windows-gnu`, `mingw-w64` (`x86_64-w64-mingw32-gcc`, `windres`) | The WebView2 Runtime (the `.exe` carries its loader) |
| **Linux** | Rust, `pkg-config`, and the WebKitGTK 4.1 / GTK 3 / libsoup 3 dev packages:<br>Debian/Ubuntu 22.04+: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libssl-dev build-essential pkg-config`<br>Fedora: `sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel librsvg2-devel`<br>Arch: `sudo pacman -S webkit2gtk-4.1 gtk3 libsoup3 openssl librsvg base-devel` | `libwebkit2gtk-4.1-0` and `libgtk-3-0` (present on desktop installs); for tray icons `libayatana-appindicator3-1` (or `libappindicator3-1`) |
| **macOS** | Rust, Xcode Command Line Tools (`xcode-select --install`) | macOS 10.15+ |

### macOS

Since 0.7.0 the package has prebuilt hosts for Apple Silicon (`macos-arm64`)
and Intel (`macos-x64`, macOS 10.15+): `fogu desktop dev` and
`fogu desktop build` work on a Mac with nothing but Fogu, and `build` makes
`dist/<Title>.app`. The hosts are built on GitHub's macOS runners (a macOS
executable can only be built on a Mac); the run that built them also made
example 01 with the fresh arm64 host, without Rust, and started it.

One limit to know: such an app is the host with the app appended to it, so the
host's ad-hoc signature no longer covers the whole file — it runs (also on
Apple Silicon), but `codesign -v` rejects it and it cannot be notarized. That
is fine for your own machines and for testers; for an app you distribute
signed and notarized, build it compiled on a Mac (`fogu desktop build --cargo`
or `--installer`, which need Rust and the Xcode Command Line Tools) and sign
that. An app downloaded from the internet without notarization is stopped by
Gatekeeper until the user opens it from the context menu ("Open").

## Mobile: Android and iOS

Tauri 2 also runs on Android and iOS, and so does the same app: the Fogu
program, its packages and the UI go into an APK/AAB or IPA together with the
Fogu runtime compiled for the device.

```sh
fogu desktop build --android --check     # only check the prerequisites (and say how to install them)
fogu desktop build --android             # dist/*.apk and dist/*.aab (aarch64)
fogu desktop build --android --apk --mobile-target aarch64,x86_64   # an APK for phones and the emulator
fogu desktop build --android --debug     # a debuggable build
fogu desktop build --ios                 # dist/*.ipa (on a Mac)
```

How it works: `tools/mobile.fg` copies `runtime/` to
`~/.fogu/desktop/<lib version>/mobile/<identifier>/runtime`, makes the crate a
library for the mobile linkers (`crate-type = ["staticlib", "cdylib",
"rlib"]`; `src/lib.rs` has `run()` marked `#[tauri::mobile_entry_point]`),
selects the app's `features` (plus `vendored-openssl` on Android, which has no
system OpenSSL for the Fogu runtime), runs `cargo tauri android init` (or
`ios init`) once, and then `cargo tauri android build` (`ios build`) with the
app staged by `runtime/build.rs` exactly as for `--installer`: the UI is
Tauri's embedded frontend and the program is extracted on first start to the
app's data folder. `icons/icon.png` (1024×1024 is best) becomes the launcher
icon (`cargo tauri icon`). `--reinit` makes the Gradle/Xcode project again.

Prerequisites (checked by the command, with the fix for each one missing):

| Platform | Needs |
| --- | --- |
| Both | Rust, the Tauri CLI 2 (`cargo install tauri-cli --version "^2" --locked`), the Rust targets (`rustup target add aarch64-linux-android` — `armv7-linux-androideabi`, `i686-linux-android`, `x86_64-linux-android` for `--mobile-target armv7/i686/x86_64`; `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` for iOS) |
| Android | A JDK 17 (`JAVA_HOME`; Android Studio's bundled JBR works), the Android SDK (`ANDROID_HOME` or `ANDROID_SDK_ROOT`: SDK Platform, Platform-Tools, Build-Tools, Command-line Tools) and the NDK (`NDK_HOME`, or the newest `<sdk>/ndk/<version>` is used). Building the vendored OpenSSL also needs `perl` and `make`. |
| iOS | macOS with Xcode (opened once) and CocoaPods (`brew install cocoapods`); for a device build, a signing team: `"desktop": { "ios": { "developmentTeam": "ABCDE12345" } }` or `APPLE_DEVELOPMENT_TEAM` |

On a phone there is one window and no tray: `desktop.tray`, `createWindow`
and the window setters do nothing there (requests return an error such as
"not supported on mobile"), folder pickers are not available, and `fogu.call`,
events, the other dialogs, `dataDir()` and the whole Fogu standard library of
the selected features work as on the desktop.

**Status:** the command, its checks and the work crate are tested
(`tests/mobile.fg`), but no Android or iOS package has been built yet (the
machine this was written on has no Android SDK/NDK, JDK or Mac). The Fogu
runtime has not been compiled for `aarch64-linux-android` yet either: it
needs the NDK's C compiler for its C dependencies (`mimalloc`, the vendored
OpenSSL), which the Tauri CLI provides to cargo during the build. Treat the
first mobile build as experimental and report what breaks.

## Vite UIs and the dev loop

`fogu desktop dev` runs an app in place with hot reload. With a Vite UI it:

1. installs the packages if needed (`bun install`, or the package manager that
   matches the lockfile);
2. finds the **generic host** for the platform and the areas of
   `fogu.json` `"desktop.features"` (or `--features`), without Rust when it
   can: a host from the cache, else a prebuilt one, else — or with `--cargo`
   — a debug host compiled with cargo once
   (see [Where the host comes from](#where-the-host-comes-from)); nothing is
   built per app. A prebuilt host is a release build: no devtools, and on
   Windows no console — `dev` then shows the program's log file
   (`desktop.log`, runtime errors) in the terminal;
3. starts `vite --host 127.0.0.1 --port 5173 --strictPort`, so UI edits are
   applied by HMR (React Fast Refresh, Vue/Svelte HMR);
4. starts the host with `FOGU_DESKTOP_PROGRAM_DIR` = the app folder and
   `FOGU_DESKTOP_DEV_URL` = `http://127.0.0.1:5173/`: the Fogu program runs
   **in place** (no copy, no build) and the window loads the dev page, where
   `window.fogu` is injected as usual. Unless you pass `--no-watch`, a change
   of a `.fg` file or `fogu.json` reloads the program in place: the window
   stays, and `desktop.keep`/`kept` carry state across (see
   [Hot reload](#hot-reload-and-kept-state); `--restart` restarts the window
   instead);
5. stops Vite and exits when you close the window or press Ctrl+C.

**WSL** (WSL has no WebKitGTK, so the window is a Windows program): the
default target under WSL is `windows`. dev copies the host to
`%LOCALAPPDATA%\fogu-desktop-dev\<name>\<name>.exe` — a Windows host always
runs as a copy named after the app, with the app's icon and version
information, so the Task Manager and the task bar's menu name the app and not
"Fogu Desktop" — and starts it from WSL with the app folder as a `\\wsl$` path
(the variables reach the Windows program through `WSLENV`). The terminal
shows the host's output: `desktop.log` lines and runtime errors (and `print`
with a debug host). The Windows window reaches Vite
through WSL's localhost forwarding. If it cannot connect, set
`localhostForwarding=true` (or `networkingMode=mirrored`) under `[wsl2]` in
`%USERPROFILE%\.wslconfig` and run `wsl --shutdown`. `--no-launch` serves
without starting the window and prints the command to start the host.

```sh
# one-time under WSL (the first line only to compile a host: --cargo, --debug)
rustup target add x86_64-pc-windows-gnu && sudo apt install mingw-w64
curl -fsSL https://bun.sh/install | bash
```

What `fogu desktop build` does with a Vite UI:

1. **Packages**: when `node_modules` is missing, it installs them with the
   package manager of the lockfile (`bun.lock`, `package-lock.json`,
   `pnpm-lock.yaml`, `yarn.lock`). Without a lockfile it uses bun, else npm.
   Under WSL, Windows programs on the PATH are ignored.
2. **Build**: it runs the `build` script of `package.json` (so `tsc && vite
   build` type-checks first), or `vite build` when there is none.
3. **Bundle**: it bundles `outDir/` with the program: the value Vite itself
   resolves from `vite.config.*` (it is run with bun or node, so `path.resolve`,
   `process.env` or a function config work); when the config cannot be run,
   the string written in it, else `dist`; `"desktop.outDir"` overrides both.
   The Vite sources are left out. The UI folder must be inside the app folder.

**Asset paths.** The page is served from the root of the host's origin
(`fogu://localhost`, `http://fogu.localhost` on Windows; `tauri://localhost`
in `--installer` builds), so `base: "/"` and `base: "./"` (the templates) both
work. A sub-path such as `base: "/app/"` would break every asset, so the build
uses `--base ./` instead and says so. A path that is not a file is answered
with `index.html`, so routers with HTML5 history survive a reload too.

A plain `ui/` folder needs no dev server: `fogu desktop dev` starts the host
with `FOGU_DESKTOP_UI_DIR` = that folder, the host serves it, and a change in
it reloads the pages (a change of the Fogu code reloads the program, as
above).

## How it works

```
 ┌──────────────────────────── my-app executable (one file) ─────────────────────────────┐
 │  WebView (WebView2 / WKWebView / WebKitGTK)        Tauri (Rust, main thread)           │
 │  ┌───────────────────────────────────┐            ┌────────────────────────────────┐  │
 │  │ ui/index.html  (embedded assets,  │  invoke    │ commands: fogu_call            │  │
 │  │ tauri://localhost)                │ ─────────▶ │           fogu_emit            │  │
 │  │ window.fogu  (client/fogu.js,     │            │           fogu_ready           │  │
 │  │ injected before page scripts)     │ ◀───────── │ webview.eval(fogu.__dispatch)  │  │
 │  └───────────────────────────────────┘   events   └──────┬──────────────▲──────────┘  │
 │                                       Fogu.signal events │ (JSON)       │ (JSON)       │
 │  Fogu runtime thread (fogu::embed::Runtime, 64 MB stack) ▼              │              │
 │  ┌──────────────────────────────────────────────────────────────────────┴───────────┐ │
 │  │ main.fg + modules (extracted on first run)        Fogu.host.emit(name, json) ──▶ │ │
 │  │   desktop:call {id,name,payload} → your fn(payload) as an fg task → desktop:result│ │
 │  │   desktop:ui:<name>              → desktop.on(name, fn)                           │ │
 │  │   desktop.emit / window / eval / quit / log  → host messages                      │ │
 │  │   desktop.dialog.* → desktop:request ⇄ desktop:reply (the task waits)             │ │
 │  └──────────────────────────────────────────────────────────────────────────────────┘ │
 └────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Fogu runs on its own thread.** Fogu values are not `Send`, so the
  interpreter lives on the thread that `fogu::embed::Runtime` starts.
  Everything that crosses threads is JSON text.
- **UI → Fogu** uses `Runtime::emit(name, json)`, which fires the program's
  `Fogu.signal.on(name, fn)` handlers, each in its own `fg` task. The library
  installs handlers for `desktop:call`, `desktop:ui:<name>` and
  `desktop:reply`.
- **Fogu → host** uses `Fogu.host.emit(name, data)`. A dispatcher thread
  resolves the pending `fogu.call`, evaluates `fogu.__dispatch(...)` in the
  page, changes the window and opens dialogs.
- **No sockets, no ports.** Any local process could call a localhost server,
  so the bridge uses Tauri's IPC instead.
- **Each call is a task** and runs through `fogu:vm`, so a runtime error in a
  command rejects that one `fogu.call` and the program keeps running.
- **Several runtimes.** Workers are more `fogu::embed::Runtime`s, each on its
  thread; every runtime passes its own id to the `Fogu.host.emit` callback, so
  a request (a dialog, a worker call) is answered in the runtime that made it.
  A hot reload replaces the program's runtime and keeps the windows.
- **Packaging.** The host looks for the app in this order (`runtime/src/main.rs`):
  1. a bundle appended to its own executable (`fogu desktop build`): the
     `fogu build --compile` format, read through
     `fogu::toolkit::build::embedded_in_memory`: the program's `.fg` files go
     into the in-memory file system, the rest is extracted once to the temp
     folder by its hash; `.fogu-desktop.json` inside it names the UI folder
     and the app's identifier (the data folders);
  2. the folder `FOGU_DESKTOP_PROGRAM_DIR` (`fogu desktop dev`): the program
     runs in place; the window loads `FOGU_DESKTOP_DEV_URL` or the UI folder;
     with `FOGU_DESKTOP_RELOAD=1` the host watches the sources and reloads;
  3. an app staged by `runtime/build.rs` into `runtime/.bundle/` and embedded
     with `include_dir!` (`--installer` builds): `ui/` becomes Tauri's
     `frontendDist`, the rest of the app plus this library (as
     `fogu_modules/desktop`) is extracted on the first run to the app's local
     data folder (`program-<hash>`), except the app's `.fg` sources, which
     stay in memory.
  In 1 and 2 the UI is served by the host's `fogu://` protocol from a folder;
  the executable built with `FOGU_DESKTOP_GENERIC=1` has an empty 3 and is the
  generic host.

### Library contents

| Path | What it is |
| --- | --- |
| `main.fg`, `fogu.json` | The Fogu library: `import desktop from "desktop"` |
| `client/fogu.js` | The page API (`window.fogu`), injected by the host into every page |
| `client/fogu-desktop.js`, `client/fogu-desktop.d.ts` | The same API as an ES module with TypeScript types |
| `update.fg` | Self-update: `import update from "desktop/update.fg"` |
| `runtime/` | The host: a Rust crate on Tauri 2 that embeds the Fogu runtime (`src/lib.rs`: the app, the `fogu://` protocol, hot reload; `bridge.rs`: Fogu ⇄ host, workers; `window.rs`: windows, their events, dialogs, opacity, notifications; `tray.rs`: tray icons and menu items; `menu.rs`: menu bars; `shortcut.rs`: global shortcuts; `update.rs`: replacing the executable) |
| `create.fg`, `dev.fg`, `build.fg`, `ci.fg`, `keygen.fg` | `fogu desktop create`, `dev`, `build` (also `--android`/`--ios`), `ci` and `keygen` |
| `templates/` | The `create` templates: `vanilla`, `ts`, `react`, `vue`, `svelte`, `frameless` |
| `tools/` | Shared code: `project.fg` (fogu.json, Vite detection, outDir/base, bun/npm), `build.fg` (the desktop build: finding the host, appending the app), `pe.fg` (the icon and version information of a Windows `.exe`), `package.fg` (`.app`, `.desktop` entry, `.zip`, NSIS) and `mobile.fg` (the Android/iOS build and its checks) |
| `hosts/` | In the installed package: the prebuilt generic hosts, `<os>-<arch>[-<areas>]/fogu-desktop(.exe)` |
| `examples/` | The [examples](#examples) |
| `tests/` | The library's tests |

### Tests

```sh
cd libs/desktop
fogu tests/run.fg                     # unit tests, fogu.json/Vite handling, the .exe resource editor, host lookup and packaging,
                                      # create templates, bridge.fg/events.fg/app.fg vs .out, client.js (the page API, with bun
                                      # or node), mobile.fg (mobile checks), the todo example's tests
FOGU_DESKTOP_HOSTS=/path/to/hosts fogu tests/run.fg   # also build-e2e.fg: a whole Windows build from <hosts>/windows-x64/fogu-desktop.exe
cd tests/embed && cargo run --release # examples/hello through fogu::embed as the host drives it (no window needed)
```

`tests/bridge.fg` is a plain script rather than a `fogu test` file because
`fogu test` stops the `Fogu.signal` task that the library starts on import, so
its handlers would never fire under the test runner. To test your own app,
keep the logic in plain modules and test those with `fogu test` (as
`examples/hello/todos.test.fg` does). Drive the commands with
`desktop.call(...)`, or with `desktop.capture` plus `Fogu.signal.emit` in a
plain script (as the examples' `fogu main.fg` runs do).

## Troubleshooting / FAQ

**`fogu.call` rejects with "not running inside a fogu-desktop window".**
The page was opened in a normal browser tab (e.g. `http://localhost:5173`
directly). Use the app window from `fogu desktop dev` or a build.
`isDesktop()` tells the two apart.

**A red "Fogu program error" box appears in the window.** `main.fg` failed to
start (a syntax error, a failed import, an error reaching `?` at the top
level). Run `fogu check main.fg` and `fogu main.fg` in the terminal to see the
full error.

**`print` shows nothing.** A release build on Windows has no console. Use
`desktop.log(...)`, which writes to `<local data dir>/fogu-desktop.log`, or
build with `--debug` to get a console.

**`unknown built-in module: 'fogu:sql'` in the window or the log.** The host
has only the standard library areas of `"desktop.features"` (default:
`network`). `dev` and `build` name the area in a warning; add it to
`fogu.json`: `"desktop": { "features": ["network", "database"] }`.

**"the desktop host must be compiled … and cargo is not installed".** There is
no prebuilt host for that platform and set of areas, or you asked for a
compiled one (`--debug`, `--cargo`, `--rebuild-host`). The message lists the
prebuilt hosts; see [Where the host comes from](#where-the-host-comes-from).

**The `.exe` has the right icon in Explorer but the window and the task bar
button show the Fogu icon.** The file's icon is in the executable; the icon
of a window is set by the host, which still uses its own (see
[Limitations](#limitations)).

**`dist/<name>.exe.old` appeared.** The app was running during the build; see
[Building and shipping](#notes). Delete it after closing the app.

**A dialog returns "needs the desktop host".** You ran `fogu main.fg` without
a window. Guard window-only code with `desktop.embedded`.

**The window freezes / other calls wait.** A command runs a long CPU loop
without yielding. Move the work into an `fg` task (see
[example 5](#5-a-background-task-with-progress)) and call `time.delay(0)` now
and then.

**The app hangs at start.** The top level of `main.fg` never returns (it ends
with `Fogu.signal.wait()`, a `loop` outside a task, or a dialog). Put loops in
`fg` tasks and dialogs in commands.

**`fogu desktop dev` says the UI is not a Vite project.** `dev` needs a Vite
UI. For a plain `ui/` folder, use `fogu desktop build --debug`.

**The `.exe` built from Linux does not start (`0xC0000135`, "WebView2Loader.dll
was not found").** It was built with desktop 0.3.0 or older, whose Windows
host needed the DLL next to it. Update (`fogu desktop update`) and build again:
since 0.3.1 the `.exe` carries the loader.

**`fogu desktop dev`: "fogu_call not allowed. Plugin not found".** The window
shows the Vite dev server, which Tauri treats as a remote origin, and the host
of desktop 0.3.1 or older did not grant the bridge's commands to it. Update
(`fogu desktop update`); 0.3.2 declares them (`runtime/capabilities/`). With
an older cached host, add `--rebuild-host` once.

**I cannot see the tray icon.** Windows 11 puts new tray icons in the
overflow menu (^) until the user drags them to the taskbar. On Linux the tray
needs `libayatana-appindicator3-1`. A wrong `icon` path is reported in the log
file and as the page event `desktop:error`.

**`fogu.window…` rejects with "fogu_window not allowed" or "unknown command".**
The window runs a host of desktop 0.3.x, which had no window API. Update the
library (hosts are cached per library version) or build with `--rebuild-host`.

**A frameless window cannot be moved.** Give an element `data-fogu-drag` (or
`data-tauri-drag-region`), and make sure the mousedown does not land on a
button, link or input inside it (those are excluded on purpose).

**My data disappeared after an update.** Files written next to the program
are lost, because each build replaces that folder. Write to
`desktop.dataDir()`.

**A package from `fogu add` is missing in the built app.** Only the app's
`fogu_modules/` is embedded. Vendor global packages into `fogu_modules/<name>`.

**Where is my app's data?** Call `desktop.dataDir()`. It is named after
`desktop.identifier` in `fogu.json`.

## Limitations

What is left, and why:

- **The Fogu program is one cooperative thread.** A CPU-bound loop in a
  command still delays the program's other commands and events until it
  yields; move such work to a [worker](#workers) (its own runtime and
  thread). Workers share nothing with the program: values cross as JSON.
- **Functions, struct methods and channels do not cross the page boundary**
  (it is JSON; bytes do cross, see [Binary data](#binary-data-and-call-timeouts)).
  Hand the page an id and a command instead.
- **A timed-out `fogu.call` does not stop the Fogu command**: a Fogu task
  cannot be killed from outside, so it runs to its end and its answer is
  dropped.
- **Hot reload keeps only what the program kept** (`desktop.keep`); the rest
  of its state starts over, as in a new run.
- **The sources are not encrypted.** A built app never writes its `.fg` files
  to disk, but they are inside the executable (as in any interpreted or
  bytecode app), where a determined user can read them.
- **Linux tray icons deliver no clicks** (appindicator only shows a menu): a
  tray with `onClick` or `clickAction` gets a first menu item instead
  (`linuxClickItem`, default "Open"; `false` turns it off) that fires the same
  click.
- **A macOS host must be compiled on a Mac** (Apple's SDK only runs on
  macOS), and so must a Linux host for an architecture or a set of areas
  without a prebuilt one (it needs the WebKitGTK dev packages). `fogu desktop
  ci` writes a GitHub Actions workflow that builds all three on GitHub's
  runners.
- **The window icon and the task bar identity are still the host's.** The
  build gives the *file* its icon and name. The icon of the window (title
  bar, task bar button, Alt+Tab) is set by the host from the icon compiled
  into it, and the host sets no AppUserModelID, so Windows groups the task
  bar button by executable. Both need a change in the host (`runtime/`): take
  the window icon from the app's `icons/icon.png` and set the AppUserModelID
  from the identifier before the first window opens.
- **The main window exists before the program runs**, so an app defined in
  code cannot give it `transparent` or another `url`, and its identifier must
  be in `fogu.json` (see [The app in code](#the-app-in-code)).
- **A prebuilt host has no devtools** (it is a release build): `fogu desktop
  dev --cargo` and `build --debug` compile the debug host, which needs Rust.
- **Transparent windows on macOS use the NSWindow private API**, which the App
  Store rejects; `"macOSPrivateApi": false` with `--installer` leaves it out
  (and transparency with it). Window opacity on Linux needs a compositing
  window manager.
- **Self-update replaces the executable in place.** An app installed where
  the user cannot write (Program Files by an `--installer` setup) cannot
  update itself this way, and on macOS replacing the binary inside a signed
  `.app` breaks its signature: ship those through their installer.
- **What has been run where (0.6.0):** a Windows app built on Linux (WSL)
  from a prebuilt host with no Rust involved — its icon and version
  information checked on Windows 11 (Explorer's icon, `VersionInfo`), the
  window started — and `fogu desktop dev` on the same prebuilt host,
  including an app defined in code (`examples/10-app-in-code`). The `.app`
  bundle, the `.icns` and the Linux `share/` tree are checked by the tests as
  files only: they have not been opened on macOS or a Linux desktop. The NSIS
  script has not been compiled (no `makensis` on that machine).
  `fogu desktop dev` on a Windows console (the log file shown in the
  terminal) has not been run either.
- **What has been run where (before):** everything above on Windows 11 (WebView2):
  workers, hot reload, menus and their accelerators, global shortcuts,
  notifications, opacity, size clamping, bytes, timeouts, in-memory sources
  and a self-update from one signed build to the next. The macOS- and
  Linux-only code (opacity through `NSWindow`/GTK, the Linux tray click item,
  macOS transparency) has not been compiled or run on those systems yet: the
  machine this was written on has no Mac SDK and no GTK. `fogu desktop ci`
  builds them on GitHub's runners. **Mobile builds** have not been built on a
  device or emulator either (no Android NDK or Mac here; see
  [Mobile](#mobile-android-and-ios)).

## License

MIT © 2026 Edmilson Soares. See [LICENSE](LICENSE).
