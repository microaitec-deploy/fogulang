// Running the fogu binary (no vscode imports: unit-tested).
import { spawn } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";

export interface RunResult {
  code: number | null;
  stdout: string;
  stderr: string;
  /** spawn error (ENOENT when the binary is missing) */
  error?: NodeJS.ErrnoException;
  timedOut?: boolean;
}

export interface RunOptions {
  cwd?: string;
  input?: string;
  timeoutMs?: number;
  env?: NodeJS.ProcessEnv;
}

/** Runs `bin args…`, optionally writing `input` to stdin. Never rejects. */
export function run(bin: string, args: string[], opts: RunOptions = {}): Promise<RunResult> {
  return new Promise((resolve) => {
    let stdout = "";
    let stderr = "";
    let done = false;
    let timer: NodeJS.Timeout | undefined;
    const finish = (r: RunResult) => {
      if (done) return;
      done = true;
      if (timer) clearTimeout(timer);
      resolve(r);
    };
    let child;
    try {
      child = spawn(bin, args, {
        cwd: opts.cwd && existsSync(opts.cwd) ? opts.cwd : undefined,
        env: { ...process.env, NO_COLOR: "1", FOGU_DOTENV: "0", ...opts.env },
        windowsHide: true,
        shell: false,
      });
    } catch (e) {
      finish({ code: null, stdout, stderr, error: e as NodeJS.ErrnoException });
      return;
    }
    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (d: string) => (stdout += d));
    child.stderr?.on("data", (d: string) => (stderr += d));
    child.on("error", (e) => finish({ code: null, stdout, stderr, error: e }));
    child.on("close", (code) => finish({ code, stdout, stderr }));
    if (opts.timeoutMs) {
      timer = setTimeout(() => {
        child.kill();
        finish({ code: null, stdout, stderr, timedOut: true });
      }, opts.timeoutMs);
    }
    if (opts.input !== undefined) {
      child.stdin?.on("error", () => undefined);
      child.stdin?.end(opts.input, "utf8");
    } else {
      child.stdin?.end();
    }
  });
}

function isFile(p: string): boolean {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
}

export interface ResolveEnv {
  platform?: NodeJS.Platform;
  path?: string;
  home?: string;
  workspaceFolder?: string;
  foguHome?: string;
}

/** Expands `~` and `${workspaceFolder}` / `${userHome}` in a configured path. */
export function expandPath(p: string, env: ResolveEnv = {}): string {
  const home = env.home ?? homedir();
  let out = p.trim();
  if (out === "~" || out.startsWith("~/") || out.startsWith("~\\")) out = home + out.slice(1);
  out = out.replace(/\$\{userHome\}/g, home);
  if (env.workspaceFolder) out = out.replace(/\$\{workspaceFolder\}/g, env.workspaceFolder);
  return out;
}

/**
 * The fogu executable: the configured path, else `fogu` on the PATH, else
 * `$FOGU_HOME/bin/fogu` / `~/.fogu/bin/fogu` (`.exe` on Windows), else plain `fogu`.
 */
export function resolveFogu(configured: string | undefined, env: ResolveEnv = {}): string {
  const platform = env.platform ?? process.platform;
  const exe = platform === "win32" ? "fogu.exe" : "fogu";
  if (configured && configured.trim()) return expandPath(configured, env);
  const pathVar = env.path ?? process.env.PATH ?? process.env.Path ?? "";
  const sep = platform === "win32" ? ";" : platform === process.platform ? delimiter : ":";
  for (const dir of pathVar.split(sep)) {
    if (!dir) continue;
    const p = join(dir.replace(/^"|"$/g, ""), exe);
    if (isFile(p)) return p;
  }
  const home = env.home ?? homedir();
  const foguHome = env.foguHome ?? process.env.FOGU_HOME;
  for (const base of [foguHome, join(home, ".fogu")]) {
    if (!base) continue;
    const p = join(base, "bin", exe);
    if (isFile(p)) return p;
  }
  return "fogu";
}

/** `fogu fmt -`: the formatted text, or an error message. */
export async function formatWithFogu(bin: string, text: string, cwd?: string): Promise<{ text?: string; error?: string }> {
  const r = await run(bin, ["fmt", "-"], { input: text, cwd, timeoutMs: 15000 });
  if (r.error) return { error: r.error.code === "ENOENT" ? `fogu not found (${bin})` : r.error.message };
  if (r.timedOut) return { error: "fogu fmt timed out" };
  if (r.code !== 0) return { error: (r.stderr || r.stdout).trim().replace(/^error:\s*/, "") || `fogu fmt exited with ${r.code}` };
  return { text: r.stdout };
}

/** `fogu check <file>`: the combined output and exit code. */
export async function checkWithFogu(bin: string, file: string, cwd?: string): Promise<RunResult> {
  return run(bin, ["check", file], { cwd, timeoutMs: 20000 });
}

/** Board names and descriptions from `fogu mcu boards`. */
export function parseBoards(output: string): { name: string; detail: string; description: string }[] {
  const boards: { name: string; detail: string; description: string }[] = [];
  let inBoards = false;
  for (const raw of output.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "").split(/\r?\n/)) {
    if (/^Boards:/.test(raw)) {
      inBoards = true;
      continue;
    }
    if (/^\S/.test(raw)) {
      inBoards = false;
      continue;
    }
    if (!inBoards) continue;
    const m = /^ {1,3}([A-Za-z0-9][\w.-]*)\s{2,}(.*)$/.exec(raw);
    if (m) {
      boards.push({ name: m[1], description: m[2].replace(/\s{2,}/g, "  ").trim(), detail: "" });
    } else if (boards.length && raw.trim()) {
      const b = boards[boards.length - 1];
      b.detail = (b.detail ? b.detail + " " : "") + raw.trim();
    }
  }
  return boards;
}
