// Parser for the human-readable output of `fogu check` (no vscode imports: unit-tested).
//
// Errors (stderr, one per file, rendered by lang/src/toolkit/diag.rs):
//
//   ✗ error[E0100]: variable 'y' is not defined
//     --> a.fg:4            (or a.fg:4:12 when the column is known)
//      |
//    4 |   return y
//      |          ^
//      = help: did you mean 'f'?
//
// Warnings (stdout; Fogu 0.7.3 prints them like the errors, 0.7.2 and older on one line):
//
//     warning[W0001]: unused variable 'a' (prefix it with '_' to silence this)
//       --> e.fg:2
//     warning: e.fg: line 2: unused variable 'a' (prefix it with '_' to silence this)

export type Severity = "error" | "warning";

export interface ParsedDiagnostic {
  severity: Severity;
  /** 0-based line */
  line: number;
  /** 0-based start column (UTF-16 code units, like VS Code) */
  start: number;
  /** 0-based end column (exclusive) */
  end: number;
  message: string;
  code?: string;
  /** the path printed by fogu (as given on the command line) */
  file?: string;
  /** an "unused ..." warning (rendered faded) */
  unnecessary?: boolean;
}

// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;]*[A-Za-z]/g;

export function stripAnsi(s: string): string {
  return s.replace(ANSI, "");
}

const ERROR_RE = /^\s*(?:✗\s+)?(error|warning)(?:\[([EW]\d+)\])?:\s+(.*)$/;
const LOC_RE = /^\s*-->\s+(.*?):(\d+)(?::(\d+))?\s*$/;
const CARET_RE = /^\s*\| ( *)(\^+)\s*$/;
const HELP_RE = /^\s*= help:\s*(.*)$/;
const WARN_RE = /^\s*warning:\s+(.*?):\s+line\s+(\d+)(?::(\d+))?:\s+(.*)$/;

/** Index (UTF-16) in `line` of the code point number `cp` (0-based). */
function cpToUtf16(line: string, cp: number): number {
  let i = 0;
  let n = 0;
  for (const ch of line) {
    if (n >= cp) return i;
    i += ch.length;
    n++;
  }
  return line.length + Math.max(0, cp - n);
}

/**
 * fogu prints the source line with each tab replaced by 4 spaces and puts the
 * caret under that text. Maps a column of the expanded text back to `line`.
 */
function expandedToUtf16(line: string, col: number): number {
  let exp = 0;
  let i = 0;
  for (const ch of line) {
    if (exp >= col) return i;
    exp += ch === "\t" ? 4 : 1;
    i += ch.length;
  }
  return line.length;
}

const isWord = (c: string | undefined) => !!c && /[\p{L}\p{N}_]/u.test(c);

/** Range of the quoted name (`variable 'x'`) as a whole word in the line, else the trimmed line. */
export function guessRange(line: string, msg: string): [number, number] {
  const name = msg.split("'")[1];
  if (name) {
    let from = 0;
    for (;;) {
      const at = line.indexOf(name, from);
      if (at < 0) break;
      if (!isWord(line[at - 1]) && !isWord(line[at + name.length])) return [at, at + name.length];
      from = at + 1;
    }
  }
  const start = line.length - line.trimStart().length;
  const end = line.trimEnd().length;
  return end > start ? [start, end] : [0, Math.max(line.length, 1)];
}

function wordEnd(line: string, start: number): number {
  let e = start;
  while (e < line.length && isWord(line[e])) e++;
  return Math.max(e, start + 1);
}

/**
 * Parses `fogu check` output (stdout and stderr concatenated). `getLine(n)`
 * returns the text of the 0-based line `n` of the checked document; it is used
 * to turn the caret marker into exact columns.
 */
export function parseCheckOutput(output: string, getLine: (n: number) => string | undefined = () => undefined): ParsedDiagnostic[] {
  const lines = stripAnsi(output).split(/\r?\n/);
  const out: ParsedDiagnostic[] = [];
  for (let i = 0; i < lines.length; i++) {
    const w = WARN_RE.exec(lines[i]);
    if (w) {
      const line = Math.max(0, parseInt(w[2], 10) - 1);
      const msg = w[4].trim();
      const text = getLine(line) ?? "";
      let start: number;
      let end: number;
      if (w[3]) {
        start = cpToUtf16(text, parseInt(w[3], 10) - 1);
        end = wordEnd(text, start);
      } else {
        [start, end] = guessRange(text, msg);
      }
      out.push({ severity: "warning", line, start, end, message: msg, file: w[1], unnecessary: /^unused\b/.test(msg) });
      continue;
    }
    const e = ERROR_RE.exec(lines[i]);
    if (!e) continue;
    const severity: Severity = e[1] === "warning" ? "warning" : "error";
    const code = e[2];
    const message = e[3].trim();
    let file: string | undefined;
    let lineNo: number | undefined;
    let col: number | undefined;
    let caret: [number, number] | undefined;
    const help: string[] = [];
    let j = i + 1;
    for (; j < lines.length; j++) {
      const l = lines[j];
      const loc = LOC_RE.exec(l);
      if (loc && lineNo === undefined) {
        file = loc[1];
        lineNo = parseInt(loc[2], 10);
        col = loc[3] ? parseInt(loc[3], 10) : undefined;
        continue;
      }
      const c = CARET_RE.exec(l);
      if (c) {
        caret = [c[1].length, c[1].length + c[2].length];
        continue;
      }
      const h = HELP_RE.exec(l);
      if (h) {
        help.push(h[1].trim());
        continue;
      }
      if (/^\s*\d*\s*\|/.test(l)) continue; // gutter / source excerpt
      break;
    }
    i = j - 1;
    if (lineNo === undefined) continue; // no position (e.g. a CLI error)
    const line = Math.max(0, lineNo - 1);
    const text = getLine(line);
    let start = 0;
    let end = 1;
    if (text !== undefined) {
      if (col !== undefined) {
        start = cpToUtf16(text, col - 1);
        end = caret ? Math.max(start + 1, expandedToUtf16(text, caret[1]) - expandedToUtf16(text, caret[0]) + start) : wordEnd(text, start);
        end = Math.min(Math.max(end, start + 1), Math.max(text.length, start + 1));
      } else if (caret) {
        start = expandedToUtf16(text, caret[0]);
        end = Math.max(start + 1, expandedToUtf16(text, caret[1]));
      } else {
        [start, end] = guessRange(text, message);
      }
    } else if (col !== undefined) {
      start = col - 1;
      end = start + (caret ? caret[1] - caret[0] : 1);
    } else if (caret) {
      [start, end] = caret;
    }
    const full = help.length ? `${message}\nhelp: ${help.join("\nhelp: ")}` : message;
    const d: ParsedDiagnostic = { severity, line, start, end, message: full, code, file };
    if (severity === "warning" && /^unused\b/.test(message)) d.unnecessary = true;
    out.push(d);
  }
  return out;
}
