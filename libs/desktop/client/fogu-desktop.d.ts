// Types of the fogu-desktop page API: `window.fogu` (injected by the host,
// client/fogu.js) and the ES module fogu-desktop.js. Copy both files next to
// your sources (create.fg does it) and import from "./fogu-desktop.js", or
// reference this file for the global (`/// <reference path="./fogu-desktop.d.ts" />`).

/** An event listener; `"*"` listeners receive (name, data). */
export type FoguListener<T = unknown> = (data: T) => void;

/** Options of a call: give up after `timeout` ms, or when `signal` aborts (the Fogu command itself keeps running). */
export interface FoguCallOptions {
  timeout?: number;
  signal?: AbortSignal;
}

export interface FoguDesktop {
  /**
   * Runs desktop.command(name, fn) in Fogu; resolves to its return value (JSON), rejects with its error message
   * (a TimeoutError / AbortError with opts). Bytes cross both ways: Uint8Array ↔ Fogu buffer.
   */
  call<T = unknown>(name: string, payload?: unknown, opts?: FoguCallOptions): Promise<T>;
  /** The default timeout of call() in ms (0: none). */
  callTimeout: number;
  /** Fires desktop.on(name, fn) in Fogu. */
  emit(name: string, data?: unknown): Promise<void>;
  /** Every event: (name, data). Returns an unsubscribe function. */
  on(name: "*", fn: (name: string, data: unknown) => void): () => void;
  /** desktop.emit(name, data) from Fogu. Returns an unsubscribe function. */
  on<T = unknown>(name: string, fn: FoguListener<T>): () => void;
  /** Removes a listener, or all the listeners of `name`. */
  off(name: string, fn?: (...args: any[]) => void): void;
  /** The next desktop.emit(name, data). */
  once<T = unknown>(name: string): Promise<T>;
  /** This window and the others. */
  readonly window: FoguWindows;
  /** Tray icon events. */
  readonly tray: FoguTray;
  /** The menu bar (desktop.menu in Fogu). */
  readonly menu: FoguMenu;
  /** Global shortcuts. */
  readonly shortcut: FoguShortcut;
  /** A system notification. */
  notify(title: string | FoguNotification, body?: string): Promise<boolean>;
  /** fn() after every hot reload of the Fogu code (fogu desktop dev). Returns an unsubscribe function. */
  onReload(fn: (e: { entry: string }) => void): () => void;
}

/** A menu item (also the tray's). `role` makes a system item; `items` a submenu. */
export interface FoguMenuItem {
  text?: string;
  id?: string;
  enabled?: boolean;
  checked?: boolean;
  accelerator?: string;
  action?: "quit" | "show" | "hide" | "toggle";
  role?:
    | "copy" | "cut" | "paste" | "selectAll" | "undo" | "redo" | "minimize" | "maximize" | "fullscreen"
    | "hide" | "hideOthers" | "showAll" | "closeWindow" | "quit" | "services" | "about" | "separator";
  items?: Array<FoguMenuItem | "-">;
}

/** A menu bar click. `menu`: the window's label, or "app". */
export interface FoguMenuEvent {
  type: "menu";
  menu: string;
  item: string;
  checked: boolean | null;
}

export interface FoguMenu {
  on(fn: (e: FoguMenuEvent) => void): () => void;
  set(items: Array<FoguMenuItem | "-"> | null, opts?: { window?: string }): Promise<null>;
}

export interface FoguShortcutEvent {
  shortcut: string;
  state: "pressed" | "released";
}

export interface FoguShortcut {
  register(accelerator: string, fn?: (e: FoguShortcutEvent) => void): Promise<null>;
  unregister(accelerator?: string): Promise<null>;
  on(fn: (e: FoguShortcutEvent) => void): () => void;
}

export interface FoguNotification {
  title: string;
  body?: string;
  icon?: string;
}

/** A window's state (logical pixels). */
export interface FoguWindowState {
  label: string;
  title: string;
  width: number;
  height: number;
  x: number | null;
  y: number | null;
  maximized: boolean;
  minimized: boolean;
  fullscreen: boolean;
  visible: boolean;
  focused: boolean;
  decorated: boolean;
  resizable: boolean;
  alwaysOnTop?: boolean;
  scaleFactor: number;
}

/** Options of a window (fogu.json "desktop", desktop.createWindow, fogu.window.create / set). */
export interface FoguWindowOptions {
  title?: string;
  width?: number;
  height?: number;
  x?: number;
  y?: number;
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
  /** null removes the limit */
  minSize?: null;
  maxSize?: null;
  center?: boolean;
  resizable?: boolean;
  maximized?: boolean;
  minimized?: boolean;
  fullscreen?: boolean;
  alwaysOnTop?: boolean;
  decorations?: boolean;
  /** only when the window is created (macOS: through the private API, see the README) */
  transparent?: boolean;
  /** the whole window's opacity, 0–1 */
  opacity?: number;
  visible?: boolean;
  focus?: boolean;
  shadow?: boolean;
  skipTaskbar?: boolean;
  closable?: boolean;
  minimizable?: boolean;
  maximizable?: boolean;
  contentProtected?: boolean;
  /** create: a route of the UI ("settings.html", "/settings", "#/about") or an http(s) URL */
  url?: string;
  action?: FoguWindowAction;
}

export type FoguWindowAction =
  | "minimize" | "unminimize" | "maximize" | "unmaximize" | "toggleMaximize" | "toggleFullscreen"
  | "center" | "show" | "hide" | "focus" | "close" | "destroy" | "startDragging" | "reload";

export type FoguWindowEventType =
  | "resized" | "moved" | "focus" | "blur" | "closeRequested" | "destroyed"
  | "scaleChanged" | "themeChanged" | "fileDrop" | "*";

/** A window event: { window, type } plus resized { width, height }, moved { x, y }, themeChanged { theme }, fileDrop { paths }. */
export interface FoguWindowEvent {
  window: string;
  type: string;
  width?: number;
  height?: number;
  x?: number;
  y?: number;
  theme?: string;
  paths?: string[];
  scaleFactor?: number;
  [key: string]: unknown;
}

/** One window (fogu.window = the calling one). Every call goes to the host and resolves when done. */
export interface FoguWindow {
  readonly label: string;
  set(opts: FoguWindowOptions): Promise<void>;
  state(): Promise<FoguWindowState>;
  size(): Promise<{ width: number; height: number }>;
  position(): Promise<{ x: number; y: number }>;
  title(): Promise<string>;
  isMaximized(): Promise<boolean>;
  isMinimized(): Promise<boolean>;
  isFullscreen(): Promise<boolean>;
  isVisible(): Promise<boolean>;
  isFocused(): Promise<boolean>;
  setSize(width: number, height: number): Promise<void>;
  setPosition(x: number, y: number): Promise<void>;
  /** no arguments: remove the limit */
  setMinSize(width?: number, height?: number): Promise<void>;
  setMaxSize(width?: number, height?: number): Promise<void>;
  setTitle(title: string): Promise<void>;
  setResizable(on?: boolean): Promise<void>;
  setDecorations(on?: boolean): Promise<void>;
  setAlwaysOnTop(on?: boolean): Promise<void>;
  setFullscreen(on?: boolean): Promise<void>;
  setSkipTaskbar(on?: boolean): Promise<void>;
  center(): Promise<void>;
  minimize(): Promise<void>;
  unminimize(): Promise<void>;
  maximize(): Promise<void>;
  unmaximize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  toggleFullscreen(): Promise<void>;
  show(): Promise<void>;
  hide(): Promise<void>;
  focus(): Promise<void>;
  /** asks first (closeRequested handlers, closeToTray) */
  close(): Promise<void>;
  /** closes without asking */
  destroy(): Promise<void>;
  /** call it on mousedown (data-fogu-drag does it for you) */
  startDragging(): Promise<void>;
  reload(): Promise<void>;
  /** Returns an unsubscribe function. */
  on(type: FoguWindowEventType, fn: (e: FoguWindowEvent) => void): () => void;
  /** fn decides whether the window closes: false (or a promise of false) keeps it open; null restores the default. */
  onCloseRequested(fn: ((e: FoguWindowEvent) => boolean | void | Promise<boolean | void>) | null): Promise<void>;
}

/** fogu.window: this window, plus the other windows. */
export interface FoguWindows extends FoguWindow {
  of(label: string): FoguWindow;
  create(label: string, opts?: FoguWindowOptions): Promise<FoguWindow>;
  list(): Promise<string[]>;
}

/** A tray event: click, rightClick, middleClick, doubleClick { tray }, menu { tray, item, checked }. */
export interface FoguTrayEvent {
  tray: string;
  type: "click" | "rightClick" | "middleClick" | "doubleClick" | "menu";
  item?: string;
  checked?: boolean | null;
}

export type FoguTrayMenuItem =
  | "-"
  | { id?: string; text: string; enabled?: boolean; checked?: boolean; accelerator?: string; action?: "quit" | "show" | "hide" | "toggle"; items?: FoguTrayMenuItem[] };

export interface FoguTray {
  on(type: FoguTrayEvent["type"] | "*", fn: (e: FoguTrayEvent) => void): () => void;
  /** Creates or changes a tray icon from the page (desktop.tray in Fogu is the usual way). */
  set(opts: { id?: string; icon?: string; tooltip?: string | null; title?: string | null; visible?: boolean; showMenuOnLeftClick?: boolean; clickAction?: "show" | "toggle" | "hide"; doubleClickAction?: "show" | "toggle" | "hide"; menu?: FoguTrayMenuItem[] | null }): Promise<void>;
  remove(id?: string): Promise<boolean>;
}

/** The module's object: window.fogu plus `ready` and `isDesktop`. */
export interface FoguModule extends Omit<FoguDesktop, "callTimeout"> {
  /** Sets the default timeout of call() in ms (0: none). */
  setCallTimeout(ms: number): void;
  /** Resolves when the host delivered the events queued before the page loaded. */
  ready(): Promise<void>;
  /** Whether the page runs inside a fogu-desktop window. */
  isDesktop(): boolean;
}

export declare function isDesktop(): boolean;
export declare function call<T = unknown>(name: string, payload?: unknown, opts?: FoguCallOptions): Promise<T>;
export declare function setCallTimeout(ms: number): void;
export declare function emit(name: string, data?: unknown): Promise<void>;
export declare function on(name: "*", fn: (name: string, data: unknown) => void): () => void;
export declare function on<T = unknown>(name: string, fn: FoguListener<T>): () => void;
export declare function off(name: string, fn?: (...args: any[]) => void): void;
export declare function once<T = unknown>(name: string): Promise<T>;
export declare function ready(): Promise<void>;
/** fogu.window (named appWindow so it does not shadow the global `window`). */
export declare const appWindow: FoguWindows;
export declare const tray: FoguTray;
export declare const menu: FoguMenu;
export declare const shortcut: FoguShortcut;
export declare function notify(title: string | FoguNotification, body?: string): Promise<boolean>;
export declare function onReload(fn: (e: { entry: string }) => void): () => void;
export declare const fogu: FoguModule;
export default fogu;

declare global {
  interface Window {
    /** The fogu-desktop page API (injected by the host; missing in a plain browser). */
    readonly fogu: FoguDesktop;
  }
  interface WindowEventMap {
    "fogu:ready": CustomEvent;
  }
}
