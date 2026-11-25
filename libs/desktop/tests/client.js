// The page API (client/fogu.js) against a fake Tauri: what fogu.window /
// fogu.tray send to the host, window events, the close decision of the page,
// and the frameless helpers. Run with bun (or node): `bun tests/client.js`;
// tests/run.fg runs it when one of them is on the PATH.
const fs = require("fs");
const path = require("path");

const calls = [];
const listeners = {};
class Element {
  constructor(attrs, parent) {
    this.attrs = attrs || {};
    this.parent = parent || null;
    this.tag = (attrs && attrs.tag) || "div";
  }
  matches(sel) {
    return sel.split(",").some((s) => {
      s = s.trim();
      const m = s.match(/^\[([\w-]+)\]$/);
      if (m) return m[1] in this.attrs;
      return s === this.tag;
    });
  }
  closest(sel) {
    for (let e = this; e; e = e.parent) if (e.matches(sel)) return e;
    return null;
  }
  contains(o) {
    for (let e = o; e; e = e.parent) if (e === this) return true;
    return false;
  }
  getAttribute(n) {
    return this.attrs[n];
  }
}
globalThis.window = globalThis;
globalThis.Element = Element;
globalThis.CustomEvent = class { constructor(n) { this.type = n; } };
globalThis.dispatchEvent = () => {};
globalThis.document = {
  readyState: "complete",
  addEventListener: (t, fn) => { listeners[t] = fn; },
};
globalThis.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" } },
  invoke: async (cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === "fogu_ready") return { events: [], window: "main" };
    if (cmd === "fogu_window" && args.op === "get") return { width: 800, height: 600, x: 1, y: 2, title: "T", maximized: true };
    if (cmd === "fogu_window" && args.op === "list") return ["main"];
    if (cmd === "fogu_call" && args.name === "slow") return new Promise((r) => setTimeout(() => r(1), 200));
    if (cmd === "fogu_call" && args.name === "echo") return args.payload;
    return null;
  },
};
eval(fs.readFileSync(path.join(__dirname, "..", "client", "fogu.js"), "utf8"));

let failed = 0;
function eq(label, got, want) {
  const g = JSON.stringify(got), w = JSON.stringify(want);
  if (g === w) {
    console.log("ok  ", label);
  } else {
    failed++;
    console.log("FAIL", label, "\n  got ", g, "\n  want", w);
  }
}
const tick = () => new Promise((r) => setTimeout(r, 5));
const sent = () => calls.filter((c) => c[0] === "fogu_window").map((c) => c[1]);

(async () => {
  await tick();
  calls.length = 0;
  await fogu.window.setSize(640, 480);
  await fogu.window.toggleMaximize();
  await fogu.window.of("tool").setAlwaysOnTop();
  await fogu.window.setMinSize();
  eq("setters", sent(), [
    { op: "set", target: null, args: { width: 640, height: 480 } },
    { op: "set", target: null, args: { action: "toggleMaximize" } },
    { op: "set", target: "tool", args: { alwaysOnTop: true } },
    { op: "set", target: null, args: { minSize: null } },
  ]);
  eq("label", [fogu.window.label, fogu.window.of("tool").label], ["main", "tool"]);
  eq("getters", [await fogu.window.size(), await fogu.window.position(), await fogu.window.isMaximized(), await fogu.window.list()], [
    { width: 800, height: 600 }, { x: 1, y: 2 }, true, ["main"],
  ]);

  // events: only the matching window/type; "*" gets all
  calls.length = 0;
  const seen = [];
  fogu.window.on("resized", (e) => seen.push("resized " + e.width));
  fogu.window.on("*", (e) => seen.push("any " + e.type));
  fogu.__dispatch("desktop:window", { window: "main", type: "resized", width: 5 });
  fogu.__dispatch("desktop:window", { window: "tool", type: "resized", width: 6 });
  fogu.__dispatch("desktop:window", { window: "main", type: "focus" });
  eq("window events", seen, ["resized 5", "any resized", "any focus"]);
  eq("listen", sent(), [
    { op: "listen", target: null, args: { events: ["resized"] } },
    { op: "listen", target: null, args: { events: ["*"] } },
  ]);

  // the page decides about closing
  calls.length = 0;
  let allow = false;
  await fogu.window.onCloseRequested(async () => allow);
  fogu.__dispatch("desktop:window", { window: "main", type: "closeRequested", decides: "page" });
  await tick();
  allow = true;
  fogu.__dispatch("desktop:window", { window: "main", type: "closeRequested", decides: "page" });
  await tick();
  eq("close decision", sent(), [
    { op: "interceptClose", target: null, args: { on: true } },
    { op: "set", target: "main", args: { action: "destroy" } },
  ]);

  // tray events
  const tray = [];
  fogu.tray.on("menu", (e) => tray.push(e.item));
  fogu.__dispatch("desktop:tray", { tray: "main", type: "click" });
  fogu.__dispatch("desktop:tray", { tray: "main", type: "menu", item: "quit" });
  eq("tray events", tray, ["quit"]);

  // frameless: drag region, double click, buttons inside it, window buttons
  calls.length = 0;
  const bar = new Element({ "data-fogu-drag": "" });
  const title = new Element({}, bar);
  const btn = new Element({ tag: "button", "data-fogu-window": "close" }, bar);
  const ev = (target, detail) => ({ button: 0, target, detail, preventDefault() {} });
  listeners.mousedown(ev(title, 1));
  listeners.mousedown(ev(bar, 2));
  listeners.mousedown(ev(btn, 1));
  listeners.click(ev(btn, 1));
  listeners.click(ev(new Element({ "data-fogu-window": "maximize" }), 1));
  await tick();
  eq("frameless", sent().map((a) => a.args.action), ["startDragging", "toggleMaximize", "close", "toggleMaximize"]);

  // fogu.call: timeout, abort, default timeout, bytes both ways
  const err = (p) => p.then(() => "resolved", (e) => e.name + ": " + e.message);
  eq("call timeout", await err(fogu.call("slow", null, { timeout: 20 })), 'TimeoutError: fogu.call("slow") timed out after 20 ms');
  const ac = new AbortController();
  const aborted = err(fogu.call("slow", null, { signal: ac.signal }));
  ac.abort();
  eq("call abort", (await aborted).split(":")[0], "AbortError");
  fogu.callTimeout = 10;
  eq("default timeout", await err(fogu.call("slow")), 'TimeoutError: fogu.call("slow") timed out after 10 ms');
  fogu.callTimeout = 0;
  eq("no timeout", await fogu.call("slow", null, { timeout: 1000 }), 1);
  calls.length = 0;
  const back = await fogu.call("echo", { img: new Uint8Array([0, 1, 255]), list: [new Uint8Array([7]).buffer], n: 2 });
  eq("bytes sent", calls[0][1].payload, { img: { $bytes: "AAH/" }, list: [{ $bytes: "Bw==" }], n: 2 });
  eq("bytes received", [back.img instanceof Uint8Array, Array.from(back.img), Array.from(back.list[0]), back.n], [true, [0, 1, 255], [7], 2]);
  const got = [];
  fogu.on("blob", (d) => got.push(d));
  fogu.__dispatch("blob", { $bytes: "aGk=" });
  eq("bytes in events", got.map((d) => Array.from(d)), [[104, 105]]);
  eq("plain objects untouched", fogu.__fromWire({ $bytes: "x", other: 1 }), { $bytes: "x", other: 1 });

  // menus, shortcuts, notifications, reload
  calls.length = 0;
  const menuSeen = [], scSeen = [], reloads = [];
  fogu.menu.on((e) => menuSeen.push(e.menu + ":" + e.item));
  await fogu.menu.set([{ text: "File", items: [{ role: "quit" }] }]);
  await fogu.menu.set(null, { window: "tool" });
  await fogu.shortcut.register("CmdOrCtrl+K", (e) => scSeen.push("cb " + e.state));
  fogu.shortcut.on((e) => scSeen.push("any " + e.shortcut));
  await fogu.notify("Saved", "3 files");
  await fogu.shortcut.unregister();
  fogu.onReload((e) => reloads.push(e.entry));
  fogu.__dispatch("desktop:menu", { menu: "app", item: "quit", type: "menu" });
  fogu.__dispatch("desktop:shortcut", { shortcut: "CmdOrCtrl+K", state: "pressed" });
  fogu.__dispatch("desktop:shortcut", { shortcut: "CmdOrCtrl+K", state: "released" });
  fogu.__dispatch("desktop:reloaded", { entry: "main.fg" });
  eq("menu/shortcut/notify ops", sent().map((a) => [a.op, a.args]), [
    ["menu", { window: null, menu: [{ text: "File", items: [{ role: "quit" }] }] }],
    ["menu", { window: "tool", menu: null }],
    ["shortcut", { shortcut: "CmdOrCtrl+K" }],
    ["notify", { title: "Saved", body: "3 files" }],
    ["shortcut", { shortcut: null, off: true }],
  ]);
  eq("menu events", menuSeen, ["app:quit"]);
  eq("shortcut events", scSeen, ["any CmdOrCtrl+K", "any CmdOrCtrl+K"]);
  eq("reload events", reloads, ["main.fg"]);

  // accelerators: parsing (the keydown matching runs only on Windows)
  eq("parseAccel", ["CmdOrCtrl+Shift+K", "Alt+F4", "Ctrl+Plus", "Ctrl++", "F12", "CmdOrCtrl+Up", "Bogus+X"].map(fogu.__parseAccel), [
    { ctrl: true, shift: true, alt: false, meta: false, key: "K" },
    { ctrl: false, shift: false, alt: true, meta: false, key: "F4" },
    { ctrl: true, shift: false, alt: false, meta: false, key: "+" },
    { ctrl: true, shift: false, alt: false, meta: false, key: "+" },
    { ctrl: false, shift: false, alt: false, meta: false, key: "F12" },
    { ctrl: true, shift: false, alt: false, meta: false, key: "ARROWUP" },
    null,
  ]);

  if (failed) {
    console.log(failed + " failed");
    process.exit(1);
  }
  console.log("client: all passed");
})();
