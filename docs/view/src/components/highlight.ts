// Syntax highlighting for the code blocks: Fogu (a small tokenizer that knows
// the language's keywords, types and built-ins), plus shell, JSON and YAML.

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const span = (cls: string, s: string) => `<span class="${cls}">${esc(s)}</span>`;

const KEYWORDS = new Set([
  'let', 'const', 'fn', 'return', 'if', 'else', 'for', 'in', 'while', 'loop',
  'break', 'continue', 'match', 'case', 'default', 'struct', 'interface',
  'embed', 'new', 'this', 'import', 'export', 'from', 'as', 'true',
  'false', 'nil', 'fg', 'chan', 'select', 'defer', 'type',
]);
const BUILTINS = new Set([
  'print', 'len', 'typeof', 'str', 'int', 'float', 'clock', 'close',
  'timeout', 'error', 'panic', 'bytes',
]);
const TYPES = new Set([
  'int', 'float', 'str', 'bool', 'duration', 'list', 'map', 'tuple',
  'fn', 'chan', 'error', 'any', 'buffer',
]);

function fogu(src: string): string {
  let out = '';
  let i = 0;
  let afterColon = false;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const rest = src.slice(i, i + 200);
    let m: RegExpExecArray | null;
    if (c === '/' && src[i + 1] === '/') {
      const end = src.indexOf('\n', i);
      const j = end < 0 ? n : end;
      out += span('tk-com', src.slice(i, j));
      i = j;
    } else if (c === '/' && src[i + 1] === '*') {
      const end = src.indexOf('*/', i + 2);
      const j = end < 0 ? n : end + 2;
      out += span('tk-com', src.slice(i, j));
      i = j;
    } else if (c === '"' || c === "'" || c === '`') {
      let j = i + 1;
      while (j < n && src[j] !== c) {
        if (src[j] === '\\') j++;
        j++;
      }
      j = Math.min(j + 1, n);
      out += span('tk-str', src.slice(i, j));
      i = j;
    } else if ((m = /^(0x[0-9a-fA-F_]+|0b[01_]+|\d[\d_]*(\.\d+)?(e[+-]?\d+)?(ns|us|ms|s|m|h|d)?)(?![A-Za-z_])/.exec(rest)) && !/[\w]/.test(src[i - 1] ?? '')) {
      out += span('tk-num', m[0]);
      i += m[0].length;
    } else if ((m = /^[A-Za-z_À-ɏ][\wÀ-ɏ]*/.exec(rest))) {
      const w = m[0];
      const call = /^\s*\(/.test(src.slice(i + w.length, i + w.length + 8));
      if (KEYWORDS.has(w)) out += span('tk-kw', w);
      else if (afterColon && TYPES.has(w)) out += span('tk-ty', w);
      else if (BUILTINS.has(w) && call) out += span('tk-bi', w);
      else if (call) out += span('tk-fn', w);
      else out += esc(w);
      i += w.length;
    } else if ((m = /^(<-|=>|\?\?|\?\.|\.\.\.|\.\.|\*\*|[+\-*/%=!<>]=?|&&|\|\||\?)/.exec(rest))) {
      out += span('tk-op', m[0]);
      i += m[0].length;
    } else {
      out += esc(c);
      i++;
    }
    if (c === ':') afterColon = true;
    else if (c === '{' || c === '=' || c === ')' || c === '\n' || c === ',') afterColon = false;
  }
  return out;
}

/** Shell / PowerShell: comments, strings, the command of each line, flags. */
function shell(src: string): string {
  return src
    .split('\n')
    .map((line) => {
      let out = '';
      let i = 0;
      let first = true;
      while (i < line.length) {
        const c = line[i];
        const rest = line.slice(i);
        let m: RegExpExecArray | null;
        if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) {
          out += span('tk-com', rest);
          break;
        } else if (c === '"' || c === "'") {
          const j = line.indexOf(c, i + 1);
          const end = j < 0 ? line.length : j + 1;
          out += span('tk-str', line.slice(i, end));
          i = end;
        } else if ((m = /^[^\s|;&<>()"'#]+/.exec(rest))) {
          const w = m[0];
          if (first && !/^[$>]$/.test(w)) {
            out += span('tk-fn', w);
            first = false;
          } else if (/^-{1,2}[\w-]+/.test(w)) out += span('tk-ty', w);
          else out += esc(w);
          i += w.length;
        } else {
          if (c === '|' || c === ';' || c === '&') first = true;
          out += esc(c);
          i++;
        }
      }
      return out;
    })
    .join('\n');
}

function json(src: string): string {
  return src.replace(/("(?:\\.|[^"\\])*")(\s*:)?|(-?\b\d+(?:\.\d+)?\b)|\b(true|false|null)\b|([^"\d\w]+|\w+)/g, (all, str, colon, num, lit, other) => {
    if (str) return colon ? span('tk-key', str) + esc(colon) : span('tk-str', str);
    if (num) return span('tk-num', num);
    if (lit) return span('tk-kw', lit);
    return esc(other ?? all);
  });
}

function yaml(src: string): string {
  return src
    .split('\n')
    .map((line) => {
      const hash = line.search(/(^|\s)#/);
      const body = hash >= 0 ? line.slice(0, hash) : line;
      const comment = hash >= 0 ? line.slice(hash) : '';
      const m = /^(\s*(?:- )?)([\w.-]+)(:)(.*)$/.exec(body);
      let out: string;
      if (m) {
        const value = m[4].replace(/("[^"]*"|'[^']*')/g, '\u0000$1\u0001');
        out = esc(m[1]) + span('tk-key', m[2]) + esc(m[3]) + esc(value).replace(/\u0000(.*?)\u0001/g, '<span class="tk-str">$1</span>');
      } else out = esc(body).replace(/(\{\{.*?\}\})/g, '<span class="tk-bi">$1</span>');
      return out + (comment ? span('tk-com', comment) : '');
    })
    .join('\n');
}

export function highlight(src: string, lang: string): string {
  switch (lang) {
    case 'fogu':
    case 'js':
      return fogu(src);
    case 'bash':
    case 'sh':
    case 'shell':
    case 'powershell':
      return shell(src);
    case 'json':
      return json(src);
    case 'yaml':
      return yaml(src);
    default:
      return esc(src);
  }
}
