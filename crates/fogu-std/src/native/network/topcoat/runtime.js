/* Fogu Topcoat runtime — browser side of `fogu:topcoat` (served at /_tc/runtime.js).
 *
 * Follows the model of Topcoat (https://github.com/tokio-rs/topcoat): pages are
 * rendered on the server; signals hold browser state; `tc-on:<event>` handlers,
 * `tc-bind:<attr>` bindings and `data-tc-text` nodes react to them without a
 * round-trip; shards (<tc-shard>) re-render on the server when their inputs
 * change and are swapped in place; procedures are server functions called with
 * `call(name, ...args)`. Server responses use Topcoat's frame protocol: one
 * JSON frame per line (snapshot / swap / redirect / error).
 */
(() => {
  if (window.tc) return;
  const BASE = (document.querySelector('meta[name="tc-base"]')?.content || "").replace(/\/$/, "");
  const signals = new Map();
  let current = null;
  const pending = new Set();
  let flushing = false;

  // ---------- signals and effects ----------

  class Signal {
    constructor(name, v) { this.name = name; this.v = v; this.subs = new Set(); this.owner = null; }
    get() { if (current) { this.subs.add(current); current.deps.add(this); } return this.v; }
    peek() { return this.v; }
    set(v) {
      if (Object.is(v, this.v) && (v === null || typeof v !== "object")) return;
      this.v = v;
      for (const e of this.subs) schedule(e);
      if (this.owner) rerun(this.owner);
    }
    update(f) { this.set(f(this.v)); }
    increment(n = 1) { this.set(this.v + n); }
    decrement(n = 1) { this.set(this.v - n); }
    toggle() { this.set(!this.v); }
    push(x) { this.set([...(this.v || []), x]); }
    toString() { return String(this.get()); }
    toJSON() { return this.v; }
  }

  function effect(fn, el) {
    const e = { fn, el, deps: new Set(), dead: false };
    run(e);
    return e;
  }
  function run(e) {
    for (const d of e.deps) d.subs.delete(e);
    e.deps.clear();
    if (e.dead) return;
    if (e.el && e.started && !e.el.isConnected) { e.dead = true; return; }
    const prev = current;
    current = e;
    try { e.fn(); } catch (err) { console.error("[topcoat]", err); } finally { current = prev; e.started = true; }
  }
  function dispose(e) { e.dead = true; for (const d of e.deps) d.subs.delete(e); e.deps.clear(); }
  function schedule(e) {
    pending.add(e);
    if (!flushing) { flushing = true; queueMicrotask(flush); }
  }
  function flush() {
    flushing = false;
    const list = [...pending];
    pending.clear();
    for (const e of list) run(e);
  }
  function untracked(f) { const prev = current; current = null; try { return f(); } finally { current = prev; } }

  // ---------- expressions ----------

  const helpers = {};
  const scope = new Proxy(Object.create(null), {
    has: (_, k) => typeof k === "string" && (signals.has(k) || k in helpers),
    get: (_, k) => (typeof k === "string" && signals.has(k) ? signals.get(k) : helpers[k]),
  });
  const compiled = new Map();
  function compile(code, statement) {
    const key = (statement ? "s:" : "e:") + code;
    let f = compiled.get(key);
    if (!f) {
      f = statement
        ? new Function("$tc", "e", "with ($tc) { return (async (event) => {\n" + code + "\n})(e) }")
        : new Function("$tc", "e", "with ($tc) { return (\n" + code + "\n) }");
      compiled.set(key, f);
    }
    return f;
  }
  const evaluate = (code, e) => compile(code, false)(scope, e);

  // ---------- server communication ----------

  function snapshotSignals() {
    const out = {};
    for (const [k, s] of signals) out[k] = s.v;
    return out;
  }

  /** Reads the frames of a response (ndjson), or a whole HTML document as a snapshot. */
  async function* frames(res) {
    const type = res.headers.get("content-type") || "";
    if (!type.includes("ndjson")) {
      const text = await res.text();
      if (!res.ok) yield { t: "error", status: res.status, message: text };
      else yield { t: "snapshot", html: text };
      return;
    }
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let rest = "";
    for (;;) {
      const { done, value } = await reader.read();
      rest += decoder.decode(value || new Uint8Array(), { stream: !done });
      const lines = rest.split("\n");
      rest = lines.pop();
      for (const l of lines) if (l.trim()) yield JSON.parse(l);
      if (done) break;
    }
    if (rest.trim()) yield JSON.parse(rest);
  }

  async function apply(res, target) {
    for await (const f of frames(res)) {
      if (f.t === "snapshot") applyDocument(f.html, false);
      else if (f.t === "swap") {
        const el = target instanceof Element ? target : document.querySelector('tc-shard[data-id="' + f.region + '"]');
        if (el) { morphInner(el, f.html); hydrate(el); }
      } else if (f.t === "redirect") return navigate(f.location, true);
      else if (f.t === "error") console.error("[topcoat] server error", f.status, f.message || "");
    }
  }

  async function call(name, ...args) {
    const res = await fetch(BASE + "/_tc/p/" + encodeURIComponent(name), {
      method: "POST",
      headers: { "content-type": "application/json", "x-topcoat": "procedure" },
      body: JSON.stringify(args),
    });
    const out = await res.json().catch(() => ({ error: "bad response (" + res.status + ")" }));
    if (out.error !== undefined) throw new Error(out.error);
    return out.ok;
  }

  // reruns: the page, or a shard, when a signal it read on the server changes
  const queued = new Set();
  const controllers = new Map();
  function rerun(target) {
    if (queued.has(target)) return;
    queued.add(target);
    queueMicrotask(() => { queued.delete(target); target === "page" ? rerunPage() : fetchShard(target); });
  }
  function begin(key) {
    controllers.get(key)?.abort();
    const c = new AbortController();
    controllers.set(key, c);
    return c;
  }
  async function rerunPage() {
    const c = begin("page");
    try {
      const res = await fetch(location.href, {
        method: "POST", signal: c.signal,
        headers: { "content-type": "application/json", "x-topcoat": "rerun" },
        body: JSON.stringify({ signals: snapshotSignals() }),
      });
      await apply(res, "page");
    } catch (e) { if (e.name !== "AbortError") console.error("[topcoat]", e); }
  }
  async function fetchShard(el, props) {
    if (!el.isConnected) return;
    props = props || untracked(() => shardProps(el));
    const c = begin(el);
    el.setAttribute("aria-busy", "true");
    try {
      const res = await fetch(BASE + "/_tc/s/" + encodeURIComponent(el.dataset.name), {
        method: "POST", signal: c.signal,
        headers: { "content-type": "application/json", "x-topcoat": "shard" },
        body: JSON.stringify({ props, signals: snapshotSignals(), path: location.pathname + location.search }),
      });
      if (res.status === 404) return rerunPage(); // the server restarted: render the whole page
      await apply(res, el);
    } catch (e) { if (e.name !== "AbortError") console.error("[topcoat]", e); }
    finally { if (controllers.get(el) === c) { el.removeAttribute("aria-busy"); controllers.delete(el); } }
  }
  function shardProps(el) {
    const spec = JSON.parse(el.dataset.props || "{}");
    const out = {};
    for (const [k, p] of Object.entries(spec)) out[k] = "e" in p ? evaluate(p.e) : p.v;
    return out;
  }

  // ---------- navigation ----------

  const prefetched = new Map();
  function load(url) {
    const hit = prefetched.get(url);
    if (hit && Date.now() - hit.at < 10000) { prefetched.delete(url); return hit.p; }
    return fetch(url, { headers: { "x-topcoat": "nav" } });
  }
  async function navigate(url, push = true) {
    const u = new URL(url, location.href);
    if (u.origin !== location.origin) { location.href = u.href; return; }
    begin("page");
    try {
      const res = await load(u.href);
      const html = await res.text();
      const final = res.redirected ? res.url : u.href;
      if (push) history.pushState({ tc: true }, "", final);
      applyDocument(html, true);
      if (push) { if (u.hash) document.getElementById(u.hash.slice(1))?.scrollIntoView(); else window.scrollTo(0, 0); }
    } catch (e) { location.href = u.href; }
  }
  function linkOf(e) {
    const a = e.target.closest && e.target.closest("a[tc-link], a[data-tc-link]");
    if (!a || a.target === "_blank" || a.hasAttribute("download")) return null;
    const u = new URL(a.href, location.href);
    return u.origin === location.origin ? a : null;
  }
  document.addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = linkOf(e);
    if (!a) return;
    e.preventDefault();
    navigate(a.href, true);
  });
  document.addEventListener("mouseover", (e) => {
    const a = linkOf(e);
    if (!a || prefetched.has(a.href)) return;
    prefetched.set(a.href, { at: Date.now(), p: fetch(a.href, { headers: { "x-topcoat": "nav" } }) });
  });
  window.addEventListener("popstate", () => navigate(location.href, false));

  // ---------- DOM morphing (keeps focus, input state and elements with ids) ----------

  function compatible(a, b) {
    return a.nodeType === b.nodeType && (a.nodeType !== 1 || (a.tagName === b.tagName && (a.id || "") === (b.id || "")));
  }
  function morphChildren(oldParent, newParent) {
    const byId = new Map();
    for (const c of oldParent.children) if (c.id) byId.set(c.id, c);
    let cursor = oldParent.firstChild;
    for (const n of [...newParent.childNodes]) {
      let match = null;
      if (n.nodeType === 1 && n.id && byId.has(n.id) && byId.get(n.id).tagName === n.tagName) match = byId.get(n.id);
      else if (cursor && compatible(cursor, n)) match = cursor;
      if (match) {
        if (match.id) byId.delete(match.id);
        if (match !== cursor) oldParent.insertBefore(match, cursor);
        else cursor = cursor.nextSibling;
        morphNode(match, n);
      } else {
        oldParent.insertBefore(document.importNode(n, true), cursor);
      }
    }
    while (cursor) { const next = cursor.nextSibling; cursor.remove(); cursor = next; }
  }
  function morphNode(o, n) {
    if (o.nodeType !== 1) { if (o.nodeValue !== n.nodeValue) o.nodeValue = n.nodeValue; return; }
    const focused = o === document.activeElement;
    for (const a of [...o.attributes]) if (!n.hasAttribute(a.name)) o.removeAttribute(a.name);
    for (const a of n.attributes) if (o.getAttribute(a.name) !== a.value) o.setAttribute(a.name, a.value);
    const tag = o.tagName;
    if (tag === "INPUT") {
      if (!focused && o.type !== "file") {
        const v = n.getAttribute("value") ?? "";
        if (o.value !== v) o.value = v;
        o.checked = n.hasAttribute("checked");
      }
      return;
    }
    if (tag === "TEXTAREA") { if (!focused && o.value !== n.textContent) o.value = n.textContent; return; }
    if (tag === "SCRIPT" || tag === "STYLE") { if (o.textContent !== n.textContent) o.textContent = n.textContent; return; }
    morphChildren(o, n);
  }
  function morphInner(el, html) {
    const t = document.createElement("template");
    t.innerHTML = html;
    morphChildren(el, t.content);
  }
  function applyDocument(html, reset) {
    const doc = new DOMParser().parseFromString(html, "text/html");
    if (doc.title) document.title = doc.title;
    document.documentElement.className = doc.documentElement.className;
    for (const a of [...document.body.attributes]) if (!doc.body.hasAttribute(a.name)) document.body.removeAttribute(a.name);
    for (const a of doc.body.attributes) document.body.setAttribute(a.name, a.value);
    if (reset) { for (const r of records) unbind(r); signals.clear(); }
    morphChildren(document.body, doc.body);
    hydrate(document.body);
  }

  // ---------- hydration ----------

  const records = new Set();
  const bound = new WeakMap();
  const BOOL = new Set(["hidden", "disabled", "checked", "selected", "readonly", "required", "open", "multiple", "inert"]);

  function signature(el) {
    let s = "";
    for (const a of el.attributes) {
      const n = a.name;
      if (n.startsWith("tc-on:") || n.startsWith("tc-bind:") || n === "data-tc-text" || (el.tagName === "TC-SHARD" && n === "data-props"))
        s += n + "=" + a.value + "\u0000";
    }
    return s;
  }
  function unbind(r) {
    for (const e of r.effects) dispose(e);
    for (const [ev, f, o] of r.listeners) r.el.removeEventListener(ev, f, o);
    bound.delete(r.el);
    records.delete(r);
  }
  function setAttr(el, name, v) {
    if (name === "value" && "value" in el) { if (el !== document.activeElement || el.value !== String(v ?? "")) el.value = v ?? ""; return; }
    if (name === "checked" && "checked" in el) { el.checked = !!v; return; }
    if (name === "class" && v && typeof v === "object") {
      for (const [c, on] of Object.entries(v)) el.classList.toggle(c, !!on);
      return;
    }
    if (name === "style" && v && typeof v === "object") { Object.assign(el.style, v); return; }
    if (BOOL.has(name) || v === false || v == null) {
      if (v) el.setAttribute(name, ""); else el.removeAttribute(name);
      return;
    }
    el.setAttribute(name, v === true ? "" : String(v));
  }
  function bindElement(el) {
    const sig = signature(el);
    const old = bound.get(el);
    if (old) {
      if (old.sig === sig) { for (const e of old.effects) if (!e.dead) run(e); return; }
      unbind(old);
    }
    if (!sig) return;
    const r = { el, sig, effects: [], listeners: [] };
    for (const a of [...el.attributes]) {
      const n = a.name, code = a.value;
      if (n.startsWith("tc-bind:")) {
        const attr = n.slice(8);
        r.effects.push(effect(() => setAttr(el, attr, evaluate(code)), el));
      } else if (n === "data-tc-text") {
        r.effects.push(effect(() => { const v = evaluate(code); const s = v == null ? "" : String(v); if (el.textContent !== s) el.textContent = s; }, el));
      } else if (n.startsWith("tc-on:")) {
        const [ev, ...mods] = n.slice(6).split(".");
        const f = compile(code, true);
        let timer = null;
        const wait = mods.includes("debounce") ? Number(mods[mods.indexOf("debounce") + 1]) || 300 : 0;
        const handler = (e) => {
          if (mods.includes("self") && e.target !== el) return;
          if (mods.includes("enter") && e.key !== "Enter") return;
          if (mods.includes("escape") && e.key !== "Escape") return;
          if (mods.includes("prevent")) e.preventDefault();
          if (mods.includes("stop")) e.stopPropagation();
          const go = () => Promise.resolve(f(scope, e)).catch((err) => console.error("[topcoat]", err));
          if (wait) { clearTimeout(timer); timer = setTimeout(go, wait); } else go();
        };
        const opts = mods.includes("once") ? { once: true } : undefined;
        el.addEventListener(ev, handler, opts);
        r.listeners.push([ev, handler, opts]);
      }
    }
    if (el.tagName === "TC-SHARD") {
      let last;
      r.effects.push(effect(() => {
        const props = shardProps(el);
        const json = JSON.stringify(props);
        if (last !== undefined && json !== last) fetchShard(el, props);
        last = json;
      }, el));
    }
    bound.set(el, r);
    records.add(r);
  }
  const loaded = new WeakSet();
  function loadSignals(root) {
    const list = root.querySelectorAll ? [...root.querySelectorAll("script[data-tc-signals]")] : [];
    if (root.matches && root.matches("script[data-tc-signals]")) list.unshift(root);
    for (const s of list) {
      if (loaded.has(s) && s.dataset.tcLoaded === s.textContent) continue;
      loaded.add(s);
      s.dataset.tcLoaded = s.textContent;
      let data;
      try { data = JSON.parse(s.textContent || "{}"); } catch { continue; }
      const owner = s.closest("tc-shard") || "page";
      for (const [name, d] of Object.entries(data)) {
        let sig = signals.get(name);
        if (!sig) { sig = new Signal(name, d.v); signals.set(name, sig); }
        if (d.t) sig.owner = owner;
      }
    }
  }
  function hydrate(root) {
    loadSignals(root);
    if (root.nodeType === 1) bindElement(root);
    const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
    for (let el = w.nextNode(); el; el = w.nextNode()) bindElement(el);
    for (const r of [...records]) if (!r.el.isConnected) unbind(r);
  }

  // ---------- toasts (shadcn-rs "sonner" markup) ----------

  function toast(title, opts = {}) {
    let box = document.querySelector("[data-tc-toaster]");
    if (!box) {
      box = document.createElement("ol");
      box.className = "sonner sonner-bottom-right";
      box.setAttribute("data-tc-toaster", "");
      document.body.appendChild(box);
    }
    const type = opts.type || "default";
    const li = document.createElement("li");
    li.className = "sonner-toast sonner-toast-" + type;
    li.setAttribute("role", type === "error" ? "alert" : "status");
    const content = document.createElement("div");
    content.className = "sonner-toast-content";
    const t = document.createElement("div");
    t.className = "sonner-toast-title";
    t.textContent = String(title);
    content.appendChild(t);
    if (opts.description) {
      const d = document.createElement("div");
      d.className = "sonner-toast-description";
      d.textContent = String(opts.description);
      content.appendChild(d);
    }
    const close = document.createElement("button");
    close.className = "sonner-toast-close";
    close.type = "button";
    close.setAttribute("aria-label", "Close");
    close.textContent = "\u00d7";
    close.onclick = () => li.remove();
    li.append(content, close);
    box.appendChild(li);
    const ms = opts.duration ?? 4000;
    if (ms > 0) setTimeout(() => li.remove(), ms);
    return li;
  }

  // ---------- theme, confirmation dialog, clipboard ----------

  function toggleTheme(to) {
    const root = document.documentElement;
    const dark = to ? to === "dark" : !root.classList.contains("dark");
    root.classList.toggle("dark", dark);
    try { localStorage.setItem("tc-theme", dark ? "dark" : "light"); } catch {}
    return dark ? "dark" : "light";
  }

  /** ask(message, { title, confirm, cancel, danger }) → Promise<boolean> */
  function ask(message, opts = {}) {
    return new Promise((resolve) => {
      const back = document.createElement("div");
      back.className = "tc-dialog-backdrop";
      const box = document.createElement("div");
      box.className = "tc-dialog";
      box.setAttribute("role", "alertdialog");
      box.setAttribute("aria-modal", "true");
      const h = document.createElement("div");
      h.className = "tc-dialog-title";
      h.textContent = opts.title || "Are you sure?";
      const p = document.createElement("div");
      p.className = "tc-dialog-text";
      p.textContent = String(message ?? "");
      const row = document.createElement("div");
      row.className = "tc-dialog-actions";
      const no = document.createElement("button");
      no.type = "button";
      no.className = "tc-btn tc-btn-outline";
      no.textContent = opts.cancel || "Cancel";
      const yes = document.createElement("button");
      yes.type = "button";
      yes.className = "tc-btn" + (opts.danger ? " tc-btn-destructive" : "");
      yes.textContent = opts.confirm || "Confirm";
      row.append(no, yes);
      box.append(h, p, row);
      back.appendChild(box);
      const prev = document.activeElement;
      const done = (v) => {
        back.remove();
        document.removeEventListener("keydown", key, true);
        if (prev && prev.focus) prev.focus();
        resolve(v);
      };
      const key = (e) => {
        if (e.key === "Escape") { e.preventDefault(); done(false); }
        if (e.key === "Tab") { e.preventDefault(); (document.activeElement === yes ? no : yes).focus(); }
      };
      no.onclick = () => done(false);
      yes.onclick = () => done(true);
      back.onclick = (e) => { if (e.target === back) done(false); };
      document.addEventListener("keydown", key, true);
      document.body.appendChild(back);
      (opts.danger ? no : yes).focus();
    });
  }

  async function copy(text, label = "Copied") {
    try {
      await navigator.clipboard.writeText(String(text));
    } catch {
      const t = document.createElement("textarea");
      t.value = String(text);
      t.style.position = "fixed";
      t.style.opacity = "0";
      document.body.appendChild(t);
      t.select();
      document.execCommand("copy");
      t.remove();
    }
    if (label) toast(label, { type: "success", duration: 1800 });
  }

  const api = {
    signal: (name, v) => { if (!signals.has(name)) signals.set(name, new Signal(name, v)); return signals.get(name); },
    signals, call, navigate, rerun, hydrate, untracked, toast, ask, copy, toggleTheme,
  };
  Object.assign(helpers, { call, navigate, toast, ask, copy, toggleTheme, tc: api });
  window.tc = api;
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => hydrate(document.body));
  else hydrate(document.body);
})();
