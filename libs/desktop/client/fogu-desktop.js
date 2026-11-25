// @ts-nocheck — the types are in fogu-desktop.d.ts
// fogu-desktop.js — an ES module over `window.fogu` for bundled UIs (Vite,
// React, Vue, Svelte…). The host injects window.fogu (client/fogu.js) into
// every page before its scripts, so this module only forwards to it; outside
// the app (a plain browser tab) calls reject with a clear error and listeners
// are no-ops. Types: fogu-desktop.d.ts next to this file.
//
//   import { call, on } from "./fogu-desktop.js"
//   const n = await call("increment", 1)
//   const off = on("count", (n) => render(n))

const NOT_IN_APP = "fogu: not running inside a fogu-desktop window (open the app, not the browser tab)";

function host() {
  return typeof window !== "undefined" && window.fogu && window.fogu.__host ? window.fogu : null;
}

/** Whether the page runs inside a fogu-desktop window. */
export function isDesktop() {
  return host() !== null;
}

/** Runs the Fogu command registered with desktop.command(name, fn). opts: { timeout (ms), signal (AbortSignal) }. */
export function call(name, payload, opts) {
  const f = host();
  return f ? f.call(name, payload, opts) : Promise.reject(new Error(NOT_IN_APP));
}

/** Sets the default timeout of call() in ms (0: none). */
export function setCallTimeout(ms) {
  const f = host();
  if (f) {
    f.callTimeout = Number(ms) || 0;
  }
}

/** Fires desktop.on(name, fn) in the Fogu program. */
export function emit(name, data) {
  const f = host();
  return f ? f.emit(name, data) : Promise.reject(new Error(NOT_IN_APP));
}

/** Listens to desktop.emit(name, data) ("*" receives (name, data)); returns an unsubscribe function. */
export function on(name, fn) {
  const f = host();
  return f ? f.on(name, fn) : function () {};
}

/** Removes a listener, or all the listeners of `name`. */
export function off(name, fn) {
  const f = host();
  if (f) {
    f.off(name, fn);
  }
}

/** The next desktop.emit(name, data). */
export function once(name) {
  const f = host();
  return f ? f.once(name) : new Promise(function () {});
}

/** Resolves when the host delivered the queued events (immediately outside the app). */
export function ready() {
  return new Promise(function (resolve) {
    if (!host() || host().__ready) {
      resolve();
      return;
    }
    window.addEventListener("fogu:ready", function () { resolve(); }, { once: true });
  });
}

function notInApp() {
  return Promise.reject(new Error(NOT_IN_APP));
}

/** A window API that rejects every call (outside the app). */
function absentWindow(label) {
  return new Proxy(
    { label: label || "main" },
    {
      get(target, k) {
        if (k in target) return target[k];
        if (k === "on") return function () { return function () {}; };
        if (k === "of") return absentWindow;
        if (k === "then") return undefined;
        return notInApp;
      },
    }
  );
}

/**
 * This window: size, position, maximize, fullscreen, always on top, title,
 * show/hide, close, events… (`appWindow.of(label)` another one,
 * `appWindow.create(label, opts)` a new one). Named appWindow so it does not
 * shadow the global `window`.
 */
export const appWindow = new Proxy(
  {},
  {
    get(_t, k) {
      const f = host();
      const w = f ? f.window : absentWindow();
      const v = w[k];
      return typeof v === "function" ? v.bind(w) : v;
    },
  }
);

/** Tray icon events (the tray is made by desktop.tray in Fogu). */
export const tray = {
  on(type, fn) {
    const f = host();
    return f ? f.tray.on(type, fn) : function () {};
  },
  set(opts) {
    const f = host();
    return f ? f.tray.set(opts) : notInApp();
  },
  remove(id) {
    const f = host();
    return f ? f.tray.remove(id) : notInApp();
  },
};

/** The menu bar: on(fn) for clicks, set(items, { window }) (desktop.menu in Fogu). */
export const menu = {
  on(fn) {
    const f = host();
    return f ? f.menu.on(fn) : function () {};
  },
  set(items, opts) {
    const f = host();
    return f ? f.menu.set(items, opts) : notInApp();
  },
};

/** Global shortcuts: register(accel, fn), unregister(accel?), on(fn). */
export const shortcut = {
  register(accel, fn) {
    const f = host();
    return f ? f.shortcut.register(accel, fn) : notInApp();
  },
  unregister(accel) {
    const f = host();
    return f ? f.shortcut.unregister(accel) : notInApp();
  },
  on(fn) {
    const f = host();
    return f ? f.shortcut.on(fn) : function () {};
  },
};

/** A system notification. */
export function notify(title, body) {
  const f = host();
  return f ? f.notify(title, body) : notInApp();
}

/** fn() after every hot reload of the Fogu code (fogu desktop dev). */
export function onReload(fn) {
  const f = host();
  return f ? f.onReload(fn) : function () {};
}

export const fogu = { call, setCallTimeout, emit, menu, shortcut, notify, onReload, on, off, once, ready, isDesktop, window: appWindow, tray };
export default fogu;
