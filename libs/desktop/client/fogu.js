// fogu.js — the frontend side of fogu-desktop. The host injects this script
// into every page before the page's own scripts run, so `window.fogu` is
// always there (no <script> tag or bundler import is needed).
//
//   const total = await fogu.call("add", { a: 1, b: 2 })   // runs desktop.command("add", fn)
//   fogu.on("tick", (data) => { ... })                      // desktop.emit("tick", data)
//   fogu.emit("clicked", { x: 1 })                          // desktop.on("clicked", fn)
//
// fogu.call rejects with an Error whose message is the Fogu error's message.
// fogu.call(name, payload, { timeout: 5000, signal }) gives up after 5 s (or
// when the AbortSignal fires); fogu.callTimeout sets a default for every call.
// Bytes cross both ways: a Fogu buffer arrives as a Uint8Array, and a
// Uint8Array / ArrayBuffer / DataView sent by the page arrives as a buffer.
//
//   await fogu.window.setSize(800, 600)                     // this window (fogu.window.of("x") another)
//   fogu.window.on("resized", (e) => ...)                   // window events
//   fogu.window.onCloseRequested(async () => confirm("Quit?"))
//   fogu.tray.on("menu", (e) => ...)                        // tray events (desktop.tray in Fogu)
//   fogu.menu.on((e) => ...)                                // menu bar clicks (desktop.menu in Fogu)
//   fogu.shortcut.register("CmdOrCtrl+Shift+K", (e) => ...) // a global shortcut
//   fogu.notify("Saved", "3 files")                         // a system notification
//   fogu.onReload(() => ...)                                // the Fogu code was hot-reloaded (dev)
//
// Frameless windows: an element with `data-fogu-drag` (or `fogu-drag`, or
// Tauri's `data-tauri-drag-region`) moves the window when dragged and
// maximizes it on a double click; a button with
// `data-fogu-window="minimize|maximize|close|fullscreen|hide"` does that.
(function () {
  "use strict";
  if (window.fogu && window.fogu.__host) {
    return;
  }
  const listeners = new Map();

  function invoke(cmd, args) {
    const t = window.__TAURI_INTERNALS__;
    if (!t || typeof t.invoke !== "function") {
      return Promise.reject(new Error("fogu: not running inside a fogu-desktop window"));
    }
    return t.invoke(cmd, args).catch(function (e) {
      throw e instanceof Error ? e : new Error(typeof e === "string" ? e : JSON.stringify(e));
    });
  }

  // ---------- values on the wire: bytes as { "$bytes": base64 } ----------

  const BYTES = "$bytes";

  function b64encode(u8) {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
    }
    return btoa(s);
  }

  function b64decode(text) {
    const s = atob(text);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) {
      out[i] = s.charCodeAt(i);
    }
    return out;
  }

  /** A page value for Fogu: binary data → { "$bytes": base64 } (at any depth). */
  function toWire(v) {
    if (v === null || typeof v !== "object") {
      return v;
    }
    if (v instanceof Uint8Array || v instanceof Uint8ClampedArray) {
      return { [BYTES]: b64encode(v) };
    }
    if (v instanceof ArrayBuffer) {
      return { [BYTES]: b64encode(new Uint8Array(v)) };
    }
    if (ArrayBuffer.isView(v)) {
      return { [BYTES]: b64encode(new Uint8Array(v.buffer, v.byteOffset, v.byteLength)) };
    }
    if (Array.isArray(v)) {
      return v.map(toWire);
    }
    if (typeof v.toJSON === "function") {
      return v;
    }
    const o = {};
    for (const k of Object.keys(v)) {
      o[k] = toWire(v[k]);
    }
    return o;
  }

  /** A Fogu value for the page: { "$bytes": base64 } → Uint8Array. */
  function fromWire(v) {
    if (v === null || typeof v !== "object") {
      return v;
    }
    if (Array.isArray(v)) {
      return v.map(fromWire);
    }
    const keys = Object.keys(v);
    if (keys.length === 1 && keys[0] === BYTES && typeof v[BYTES] === "string") {
      return b64decode(v[BYTES]);
    }
    const o = {};
    for (const k of keys) {
      o[k] = fromWire(v[k]);
    }
    return o;
  }

  /** fogu.call: the command's answer, or a rejection on timeout / abort. */
  function callCommand(name, payload, opts) {
    const o = opts || {};
    const timeout = o.timeout != null ? Number(o.timeout) : Number(fogu.callTimeout) || 0;
    const signal = o.signal;
    const label = "fogu.call(" + JSON.stringify(String(name)) + ")";
    if (signal && signal.aborted) {
      return Promise.reject(abortError(label, signal));
    }
    const p = invoke("fogu_call", { name: String(name), payload: payload === undefined ? null : toWire(payload) }).then(fromWire);
    if (!(timeout > 0) && !signal) {
      return p;
    }
    return new Promise(function (resolve, reject) {
      let timer = null;
      const done = function () {
        if (timer !== null) {
          clearTimeout(timer);
        }
        if (signal) {
          signal.removeEventListener("abort", onAbort);
        }
      };
      const onAbort = function () {
        done();
        reject(abortError(label, signal));
      };
      if (timeout > 0) {
        timer = setTimeout(function () {
          done();
          const e = new Error(label + " timed out after " + timeout + " ms");
          e.name = "TimeoutError";
          reject(e);
        }, timeout);
      }
      if (signal) {
        signal.addEventListener("abort", onAbort, { once: true });
      }
      p.then(
        function (v) {
          done();
          resolve(v);
        },
        function (e) {
          done();
          reject(e);
        }
      );
    });
  }

  function abortError(label, signal) {
    if (signal && signal.reason instanceof Error) {
      return signal.reason;
    }
    const e = new Error(label + " was aborted");
    e.name = "AbortError";
    return e;
  }

  function dispatch(name, data) {
    data = fromWire(data);
    for (const key of [name, "*"]) {
      const set = listeners.get(key);
      if (!set) {
        continue;
      }
      for (const fn of Array.from(set)) {
        try {
          key === "*" ? fn(name, data) : fn(data);
        } catch (e) {
          console.error("fogu.on(" + JSON.stringify(key) + ") handler failed:", e);
        }
      }
    }
  }

  function showFatal(message) {
    console.error("fogu:", message);
    const show = function () {
      let box = document.getElementById("__fogu_fatal");
      if (!box) {
        box = document.createElement("pre");
        box.id = "__fogu_fatal";
        box.style.cssText =
          "position:fixed;left:12px;right:12px;bottom:12px;z-index:2147483647;margin:0;padding:12px 14px;" +
          "max-height:45vh;overflow:auto;white-space:pre-wrap;font:12px/1.45 ui-monospace,Menlo,Consolas,monospace;" +
          "background:#2b0b0b;color:#ffd7d7;border:1px solid #ff6b6b;border-radius:8px;box-shadow:0 6px 24px rgba(0,0,0,.35)";
        document.body.appendChild(box);
      }
      box.textContent = "Fogu program error\n\n" + message;
    };
    if (document.body) {
      show();
    } else {
      document.addEventListener("DOMContentLoaded", show);
    }
  }

  // ---------- windows ----------

  function currentLabel() {
    const t = window.__TAURI_INTERNALS__;
    const m = t && t.metadata && t.metadata.currentWindow;
    return (m && m.label) || "main";
  }

  // window event handlers of this page: "<label>\u0000<type>" → Set(fn)
  const winListeners = new Map();
  const closeDeciders = new Map();

  function winOp(op, target, args) {
    return invoke("fogu_window", { op: op, target: target, args: args === undefined ? null : args });
  }

  function onWindowEvent(e) {
    if (!e) {
      return;
    }
    if (e.type === "closeRequested" && e.decides === "page" && e.window === currentLabel()) {
      const decide = closeDeciders.get(e.window);
      Promise.resolve(decide ? decide(e) : true)
        .then(function (ok) {
          if (ok !== false) {
            winOp("set", e.window, { action: "destroy" });
          }
        })
        .catch(function (err) {
          console.error("fogu.window.onCloseRequested handler failed:", err);
        });
    }
    for (const label of [e.window, "*"]) {
      for (const type of [e.type, "*"]) {
        const set = winListeners.get(label + "\u0000" + type);
        if (!set) {
          continue;
        }
        for (const fn of Array.from(set)) {
          try {
            fn(e);
          } catch (err) {
            console.error("fogu.window.on(" + JSON.stringify(type) + ") handler failed:", err);
          }
        }
      }
    }
  }

  /** The window API for the window `label` (null = the calling window). */
  function windowApi(label) {
    const target = label || null;
    const me = function () {
      return label || currentLabel();
    };
    const set = function (opts) {
      return winOp("set", target, opts || {});
    };
    const act = function (a) {
      return set({ action: a });
    };
    const get = function () {
      return winOp("get", target, {});
    };
    const field = function (k) {
      return function () {
        return get().then(function (s) {
          return s[k];
        });
      };
    };
    return {
      get label() {
        return me();
      },
      set: set,
      state: get,
      size: function () {
        return get().then(function (s) {
          return { width: s.width, height: s.height };
        });
      },
      position: function () {
        return get().then(function (s) {
          return { x: s.x, y: s.y };
        });
      },
      title: field("title"),
      isMaximized: field("maximized"),
      isMinimized: field("minimized"),
      isFullscreen: field("fullscreen"),
      isVisible: field("visible"),
      isFocused: field("focused"),
      setSize: function (width, height) {
        return set({ width: width, height: height });
      },
      setPosition: function (x, y) {
        return set({ x: x, y: y });
      },
      setMinSize: function (width, height) {
        return width == null && height == null ? set({ minSize: null }) : set({ minWidth: width || 0, minHeight: height || 0 });
      },
      setMaxSize: function (width, height) {
        return width == null && height == null ? set({ maxSize: null }) : set({ maxWidth: width || 0, maxHeight: height || 0 });
      },
      setTitle: function (t) {
        return set({ title: String(t) });
      },
      setResizable: function (b) {
        return set({ resizable: b !== false });
      },
      setDecorations: function (b) {
        return set({ decorations: b !== false });
      },
      setAlwaysOnTop: function (b) {
        return set({ alwaysOnTop: b !== false });
      },
      setFullscreen: function (b) {
        return set({ fullscreen: b !== false });
      },
      setSkipTaskbar: function (b) {
        return set({ skipTaskbar: b !== false });
      },
      center: function () { return act("center"); },
      minimize: function () { return act("minimize"); },
      unminimize: function () { return act("unminimize"); },
      maximize: function () { return act("maximize"); },
      unmaximize: function () { return act("unmaximize"); },
      toggleMaximize: function () { return act("toggleMaximize"); },
      toggleFullscreen: function () { return act("toggleFullscreen"); },
      show: function () { return act("show"); },
      hide: function () { return act("hide"); },
      focus: function () { return act("focus"); },
      close: function () { return act("close"); },
      destroy: function () { return act("destroy"); },
      startDragging: function () { return act("startDragging"); },
      reload: function () { return act("reload"); },
      /** Window events: resized, moved, focus, blur, closeRequested, destroyed, scaleChanged, themeChanged, fileDrop ("*": all). Returns an unsubscribe function. */
      on: function (type, fn) {
        const key = (label || currentLabel()) + "\u0000" + type;
        if (!winListeners.has(key)) {
          winListeners.set(key, new Set());
        }
        winListeners.get(key).add(fn);
        winOp("listen", target, { events: [type] }).catch(function (e) {
          console.error("fogu.window.on:", e);
        });
        return function () {
          const set = winListeners.get(key);
          if (set) {
            set.delete(fn);
          }
        };
      },
      /** fn(event) decides whether the window closes: false (or a promise of false) keeps it open; null restores the default. */
      onCloseRequested: function (fn) {
        const l = label || currentLabel();
        if (fn) {
          closeDeciders.set(l, fn);
        } else {
          closeDeciders.delete(l);
        }
        return winOp("interceptClose", target, { on: !!fn });
      },
    };
  }

  const windowObj = windowApi(null);
  /** The API of another window (by label). */
  windowObj.of = function (label) {
    return windowApi(String(label));
  };
  /** Opens a window on a page of the app (route or URL) with window options; resolves to its state. */
  windowObj.create = function (label, opts) {
    const o = Object.assign({}, opts || {}, { label: String(label) });
    return winOp("create", null, o).then(function () {
      return windowApi(String(label));
    });
  };
  /** The labels of the open windows. */
  windowObj.list = function () {
    return winOp("list", null, {});
  };

  // ---------- tray ----------

  const trayObj = {
    /** Tray events (desktop.tray in Fogu): click, doubleClick, rightClick, middleClick, menu ("*": all). Returns an unsubscribe function. */
    on: function (type, fn) {
      return fogu.on("desktop:tray", function (e) {
        if (type === "*" || (e && e.type === type)) {
          fn(e);
        }
      });
    },
    /** Creates or changes a tray icon from the page (menu items without functions; listen with on("menu")). */
    set: function (opts) {
      return winOp("tray", null, Object.assign({ op: "set" }, opts || {}));
    },
    remove: function (id) {
      return winOp("tray", null, { op: "remove", id: id || "main" });
    },
  };

  // ---------- menus, shortcuts, notifications ----------

  const menuObj = {
    /** Menu bar clicks: fn({ menu, item, checked }). Returns an unsubscribe function. */
    on: function (fn) {
      return fogu.on("desktop:menu", fn);
    },
    /** Sets the app menu (or a window's: opts.window); items without functions, listen with on(). null removes it. */
    set: function (items, opts) {
      const o = opts || {};
      return winOp("menu", null, { window: o.window || null, menu: items || null });
    },
  };

  const shortcutFns = new Map();
  const shortcutObj = {
    /** A global shortcut ("CmdOrCtrl+Shift+K"): fn({ shortcut, state }) on every press. Resolves when registered. */
    register: function (accel, fn) {
      if (fn) {
        shortcutFns.set(String(accel), fn);
      }
      return winOp("shortcut", null, { shortcut: String(accel) });
    },
    /** Removes one (or, without an argument, every) global shortcut. */
    unregister: function (accel) {
      if (accel == null) {
        shortcutFns.clear();
      } else {
        shortcutFns.delete(String(accel));
      }
      return winOp("shortcut", null, { shortcut: accel == null ? null : String(accel), off: true });
    },
    /** Every shortcut event (also those the Fogu program registered). Returns an unsubscribe function. */
    on: function (fn) {
      return fogu.on("desktop:shortcut", fn);
    },
  };

  const fogu = {
    __host: true,
    __ready: false,
    /** Calls the Fogu function registered with desktop.command(name, fn). opts: { timeout (ms), signal (AbortSignal) }. */
    call: function (name, payload, opts) {
      return callCommand(name, payload, opts);
    },
    /** The default timeout of fogu.call in ms (0: none). */
    callTimeout: 0,
    /** Fires desktop.on(name, fn) handlers in the Fogu program. */
    emit: function (name, data) {
      return invoke("fogu_emit", { name: String(name), data: data === undefined ? null : toWire(data) });
    },
    /** Listens to desktop.emit(name, data); "*" receives (name, data) of every event. Returns an unsubscribe function. */
    on: function (name, fn) {
      if (!listeners.has(name)) {
        listeners.set(name, new Set());
      }
      listeners.get(name).add(fn);
      return function () {
        fogu.off(name, fn);
      };
    },
    off: function (name, fn) {
      const set = listeners.get(name);
      if (set) {
        fn ? set.delete(fn) : set.clear();
      }
    },
    /** A promise of the next desktop.emit(name, data). */
    once: function (name) {
      return new Promise(function (resolve) {
        const off = fogu.on(name, function (data) {
          off();
          resolve(data);
        });
      });
    },
    /** This window (or another: fogu.window.of(label)); see windowApi. */
    window: windowObj,
    /** Tray icon events. */
    tray: trayObj,
    /** The menu bar. */
    menu: menuObj,
    /** Global shortcuts. */
    shortcut: shortcutObj,
    /** A system notification: notify("Title", "Body") or notify({ title, body, icon }). */
    notify: function (title, body) {
      const o = typeof title === "object" && title !== null ? title : { title: String(title), body: body == null ? null : String(body) };
      return winOp("notify", null, o);
    },
    /** fn() after every hot reload of the Fogu code (fogu desktop dev). Returns an unsubscribe function. */
    onReload: function (fn) {
      return fogu.on("desktop:reloaded", fn);
    },
    // used by the host (and the tests)
    __dispatch: dispatch,
    __toWire: toWire,
    __fromWire: fromWire,
    __parseAccel: parseAccel,
    __fatal: showFatal,
  };
  Object.defineProperty(window, "fogu", { value: fogu, writable: false, configurable: false });

  fogu.on("desktop:window", onWindowEvent);
  fogu.on("desktop:accelerators", loadAccels);
  fogu.on("desktop:shortcut", function (e) {
    const fn = e && e.state === "pressed" ? shortcutFns.get(e.shortcut) : null;
    if (fn) {
      try {
        fn(e);
      } catch (err) {
        console.error("fogu.shortcut handler failed:", err);
      }
    }
  });

  // ---------- frameless windows: drag regions and window buttons ----------

  const DRAG = "[data-fogu-drag],[fogu-drag]";
  const NO_DRAG = "button,a,input,select,textarea,label,[contenteditable],[data-fogu-no-drag],[fogu-no-drag],[data-fogu-window]";
  document.addEventListener("mousedown", function (e) {
    if (e.button !== 0 || !(e.target instanceof Element)) {
      return;
    }
    const region = e.target.closest(DRAG);
    if (!region) {
      return;
    }
    const blocker = e.target.closest(NO_DRAG);
    if (blocker && region.contains(blocker) && blocker !== region) {
      return;
    }
    e.preventDefault();
    if (e.detail === 2) {
      windowObj.toggleMaximize();
    } else {
      windowObj.startDragging();
    }
  });
  document.addEventListener("click", function (e) {
    const btn = e.target instanceof Element ? e.target.closest("[data-fogu-window]") : null;
    if (!btn) {
      return;
    }
    const a = btn.getAttribute("data-fogu-window");
    const map = { minimize: "minimize", maximize: "toggleMaximize", close: "close", fullscreen: "toggleFullscreen", hide: "hide" };
    if (map[a]) {
      windowObj.set({ action: map[a] });
    }
  });

  // ---------- menu accelerators the webview keeps from the menu (Windows) ----------
  // On Windows the keys go to WebView2, not to the window's menu: the page
  // matches the menus' accelerators itself and fires the item through the host.

  const IS_WINDOWS = typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent || "");
  let accels = [];

  const KEY_NAMES = {
    plus: "+", space: " ", enter: "Enter", return: "Enter", esc: "Escape", escape: "Escape", tab: "Tab",
    backspace: "Backspace", delete: "Delete", del: "Delete", insert: "Insert", home: "Home", end: "End",
    pageup: "PageUp", pagedown: "PageDown", up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight",
    arrowup: "ArrowUp", arrowdown: "ArrowDown", arrowleft: "ArrowLeft", arrowright: "ArrowRight",
  };

  /** "CmdOrCtrl+Shift+K" → { ctrl, shift, alt, meta, key } (key upper-cased), or null. */
  function parseAccel(text) {
    const parts = String(text).split("+").map(function (p) { return p.trim(); });
    if (parts.length > 1 && parts[parts.length - 1] === "") {
      parts.splice(parts.length - 2, 2, "+");
    }
    const out = { ctrl: false, shift: false, alt: false, meta: false, key: "" };
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i].toLowerCase();
      const last = i === parts.length - 1;
      if (!last && (p === "cmdorctrl" || p === "commandorcontrol" || p === "ctrl" || p === "control")) out.ctrl = true;
      else if (!last && p === "shift") out.shift = true;
      else if (!last && (p === "alt" || p === "option")) out.alt = true;
      else if (!last && (p === "cmd" || p === "command" || p === "super" || p === "meta" || p === "win")) out.meta = true;
      else if (last) out.key = (KEY_NAMES[p] || parts[i]).toUpperCase();
      else return null;
    }
    return out.key ? out : null;
  }

  /** The key of a keydown, as parseAccel writes it (layout-independent for letters and digits). */
  function eventKey(e) {
    if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
    if (/^Digit[0-9]$/.test(e.code)) return e.code.slice(5);
    return String(e.key || "").toUpperCase();
  }

  function loadAccels(list) {
    accels = (list || [])
      .map(function (a) { return { menu: a.menu, item: a.item, keys: parseAccel(a.accel) }; })
      .filter(function (a) { return a.keys; });
  }

  if (IS_WINDOWS) {
    document.addEventListener(
      "keydown",
      function (e) {
        if (!accels.length || e.repeat) return;
        const key = eventKey(e);
        const label = currentLabel();
        // this window's own menu, else the app menu
        const own = accels.some(function (a) { return a.menu === label; });
        const scope = own ? label : "app";
        const hit = accels.find(function (a) {
          const k = a.keys;
          return a.menu === scope && k.key === key && k.ctrl === e.ctrlKey && k.shift === e.shiftKey && k.alt === e.altKey && k.meta === e.metaKey;
        });
        if (!hit) return;
        e.preventDefault();
        e.stopPropagation();
        winOp("menuItem", null, { menu: hit.menu, item: hit.item }).catch(function (err) {
          console.error("fogu: menu accelerator:", err);
        });
      },
      true
    );
  }

  // tell the host the page can receive events (it queues them until then)
  const ready = function () {
    invoke("fogu_ready", {})
      .then(function (r) {
        for (const ev of r.events || []) {
          dispatch(ev.name, ev.data);
        }
        if (r.fatal) {
          showFatal(r.fatal);
        }
        fogu.__ready = true;
        window.dispatchEvent(new CustomEvent("fogu:ready"));
        if (IS_WINDOWS) {
          winOp("accelerators", null, {}).then(loadAccels, function () {});
        }
      })
      .catch(function (e) {
        console.error("fogu: ready failed:", e);
      });
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", ready);
  } else {
    ready();
  }
})();
