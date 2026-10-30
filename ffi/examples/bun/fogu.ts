// fogu.ts — a small wrapper over libfogu_rt (the Fogu runtime as a shared
// library) for Bun, through bun:ffi. Values cross as JSON.
//
//   import { Fogu } from "./fogu.ts"
//   const fogu = new Fogu()                 // finds the library next to this file
//   fogu.eval('fn add(a, b) { return a + b }')
//   fogu.call("add", 1, 2)                  // 3
//   fogu.close()
import { dlopen, FFIType, CString, JSCallback, type Pointer } from "bun:ffi";
import { existsSync } from "node:fs";
import { join } from "node:path";

/** The library file of this platform, as published on the Fogu server. */
export function libraryName(platform = process.platform, arch = process.arch): string {
  const a = arch === "x64" ? "x64" : arch === "arm64" ? "arm64" : arch === "ia32" ? "x86" : arch === "arm" ? "armv7" : arch;
  if (platform === "win32") return `fogu_rt-windows-${a}.dll`;
  if (platform === "darwin") return `libfogu_rt-macos-${a}.dylib`;
  return `libfogu_rt-linux-${a}.so`;
}

/** Where the library is looked for: $FOGU_RT_LIB, then next to this file. */
export function findLibrary(): string {
  const candidates = [
    process.env.FOGU_RT_LIB,
    join(import.meta.dir, libraryName()),
    // names produced by `cargo ffi` in a checkout
    join(import.meta.dir, process.platform === "win32" ? "fogu_rt.dll" : process.platform === "darwin" ? "libfogu_rt.dylib" : "libfogu_rt.so"),
  ].filter(Boolean) as string[];
  const found = candidates.find((p) => existsSync(p));
  if (!found) {
    throw new Error(`libfogu_rt not found (download ${libraryName()} from the Fogu server, or set FOGU_RT_LIB). Looked in: ${candidates.join(", ")}`);
  }
  return found;
}

const SYMBOLS = {
  fogu_version: { args: [], returns: FFIType.cstring },
  fogu_new: { args: [], returns: FFIType.ptr },
  fogu_free: { args: [FFIType.ptr], returns: FFIType.void },
  fogu_free_string: { args: [FFIType.ptr], returns: FFIType.void },
  fogu_eval: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  fogu_spawn: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  fogu_run_file: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  fogu_call: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },
  fogu_get: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.ptr },
  fogu_set: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },
  fogu_emit: { args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], returns: FFIType.ptr },
  fogu_on_host: { args: [FFIType.ptr, FFIType.function, FFIType.ptr], returns: FFIType.ptr },
} as const;

const cstr = (s: string) => Buffer.from(s + "\0");

/** A Fogu runtime: its globals, fg tasks and servers live until close(). */
export class Fogu {
  private lib;
  private rt: Pointer | null;
  private hostCb: JSCallback | null = null;
  private handlers = new Map<string, ((data: unknown) => void)[]>();

  constructor(path = findLibrary()) {
    this.lib = dlopen(path, SYMBOLS);
    this.rt = this.lib.symbols.fogu_new();
    if (!this.rt) throw new Error("fogu_new failed");
  }

  get version(): string {
    return String(this.lib.symbols.fogu_version());
  }

  /** Reads a {"ok","value"|"error"} answer and frees it. */
  private take(p: Pointer | null): any {
    if (!p) throw new Error("fogu: null answer");
    const r = JSON.parse(new CString(p).toString());
    this.lib.symbols.fogu_free_string(p);
    if (!r.ok) throw new Error(r.error);
    return r.value;
  }

  private get handle(): Pointer {
    if (!this.rt) throw new Error("fogu: runtime closed");
    return this.rt;
  }

  /** Runs code in the global scope; returns the value of its last expression. */
  eval(code: string): any {
    return this.take(this.lib.symbols.fogu_eval(this.handle, cstr(code)));
  }
  /** Runs code as a background task (servers, loops) and returns at once. */
  spawn(code: string): void {
    this.take(this.lib.symbols.fogu_spawn(this.handle, cstr(code)));
  }
  runFile(path: string): any {
    return this.take(this.lib.symbols.fogu_run_file(this.handle, cstr(path)));
  }
  /** Calls a global Fogu function with JSON-able arguments. */
  call(name: string, ...args: unknown[]): any {
    return this.take(this.lib.symbols.fogu_call(this.handle, cstr(name), cstr(JSON.stringify(args))));
  }
  get(name: string): any {
    return this.take(this.lib.symbols.fogu_get(this.handle, cstr(name)));
  }
  set(name: string, value: unknown): void {
    this.take(this.lib.symbols.fogu_set(this.handle, cstr(name), cstr(JSON.stringify(value))));
  }
  /** Fires the program's Fogu.signal.on(name, fn) handlers. */
  emit(name: string, data: unknown = null): void {
    this.take(this.lib.symbols.fogu_emit(this.handle, cstr(name), cstr(JSON.stringify(data))));
  }
  /** Receives Fogu.host.emit(name, data) from the program. */
  on(name: string, fn: (data: unknown) => void): void {
    if (!this.hostCb) {
      this.hostCb = new JSCallback(
        (n: Pointer, json: Pointer) => {
          const event = new CString(n).toString();
          const data = JSON.parse(new CString(json).toString());
          this.lib.symbols.fogu_free_string(n);
          this.lib.symbols.fogu_free_string(json);
          for (const h of [...(this.handlers.get(event) ?? []), ...(this.handlers.get("*") ?? [])]) h(data);
        },
        { args: [FFIType.ptr, FFIType.ptr, FFIType.ptr], returns: FFIType.void, threadsafe: true },
      );
      this.take(this.lib.symbols.fogu_on_host(this.handle, this.hostCb.ptr, null));
    }
    this.handlers.set(name, [...(this.handlers.get(name) ?? []), fn]);
  }
  /** Stops the program's tasks and servers and frees the runtime. */
  close(): void {
    if (this.rt) this.lib.symbols.fogu_free(this.rt);
    this.rt = null;
    this.hostCb?.close();
    this.hostCb = null;
  }
}
