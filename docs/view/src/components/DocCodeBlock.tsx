import React, { useState } from 'react';

interface DocCodeBlockProps {
  code: string;
  lang?: string;
  title?: string;
  onRun?: (code: string) => void;
}

export const DocCodeBlock: React.FC<DocCodeBlockProps> = ({ code, lang = 'fogu', title, onRun }) => {
  const [copied, setCopied] = useState(false);

  const cleanCode = code.replace(/^\n/, '').replace(/\s+$/, '');

  const handleCopy = () => {
    if (navigator.clipboard) {
      navigator.clipboard.writeText(cleanCode).then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      });
    }
  };

  const highlight = (src: string) => {
    const KEYWORDS = new Set([
      'let', 'const', 'fn', 'return', 'if', 'else', 'for', 'in', 'while',
      'break', 'continue', 'match', 'case', 'default', 'struct', 'interface',
      'embed', 'new', 'this', 'import', 'export', 'from', 'as', 'true',
      'false', 'nil', 'fg', 'chan', 'select'
    ]);
    const BUILTINS = new Set([
      'print', 'len', 'typeof', 'str', 'int', 'float', 'clock', 'close',
      'timeout', 'error', 'panic'
    ]);
    const TYPES = new Set([
      'int', 'float', 'str', 'bool', 'duration', 'list', 'map', 'tuple',
      'fn', 'chan', 'error', 'any'
    ]);

    const escapeHtml = (s: string) =>
      s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

    let out = '';
    let i = 0;
    let afterColon = false;
    const n = src.length;

    while (i < n) {
      const c = src[i];
      const rest = src.slice(i);
      let m: RegExpExecArray | null;

      if (rest.startsWith('//')) {
        const end = src.indexOf('\n', i);
        const j = end < 0 ? n : end;
        out += '<span class="tk-com">' + escapeHtml(src.slice(i, j)) + '</span>';
        i = j;
      } else if (rest.startsWith('/*')) {
        const end = src.indexOf('*/', i + 2);
        const j = end < 0 ? n : end + 2;
        out += '<span class="tk-com">' + escapeHtml(src.slice(i, j)) + '</span>';
        i = j;
      } else if (c === '"' || c === "'" || c === '`') {
        let j = i + 1;
        while (j < n && src[j] !== c) {
          if (src[j] === '\\') j++;
          j++;
        }
        j = Math.min(j + 1, n);
        out += '<span class="tk-str">' + escapeHtml(src.slice(i, j)) + '</span>';
        i = j;
      } else if ((m = /^(0x[0-9a-fA-F_]+|0b[01_]+|\d[\d_]*(\.\d+)?(e[+-]?\d+)?(ns|us|ms|s|m|h|d)?)(?![A-Za-z_])/.exec(rest))) {
        out += '<span class="tk-num">' + m[0] + '</span>';
        i += m[0].length;
      } else if ((m = /^[A-Za-z_À-ɏ][\wÀ-ɏ]*/.exec(rest))) {
        const w = m[0];
        const call = src.slice(i + w.length).match(/^\s*\(/);
        if (KEYWORDS.has(w)) out += '<span class="tk-kw">' + w + '</span>';
        else if (afterColon && TYPES.has(w)) out += '<span class="tk-ty">' + w + '</span>';
        else if (BUILTINS.has(w) && call) out += '<span class="tk-bi">' + w + '</span>';
        else if (call) out += '<span class="tk-fn">' + w + '</span>';
        else out += w;
        i += w.length;
      } else if ((m = /^(<-|=>|\?\?|\?\.|\.\.\.|\.\.|\*\*|[+\-*\/%=!<>]=?|&&|\|\||\?)/.exec(rest))) {
        out += '<span class="tk-op">' + escapeHtml(m[0]) + '</span>';
        i += m[0].length;
      } else {
        out += escapeHtml(c);
        i++;
      }

      if (c === ':') afterColon = true;
      else if (c === '{' || c === '=' || c === ')' || c === '\n') afterColon = false;
    }
    return out;
  };

  const renderedHtml = lang === 'fogu' ? highlight(cleanCode) : cleanCode.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

  return (
    <div className="group relative mt-4 rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-[#11141b] overflow-hidden text-left">
      <div className="flex items-center justify-between px-4 py-1.5 border-b border-slate-200 dark:border-slate-800 text-xs text-slate-500">
        <span className="font-mono">{title || lang}</span>
        <div className="flex items-center gap-2">
          {onRun && lang === 'fogu' && (
            <button
              type="button"
              onClick={() => onRun(cleanCode)}
              className="rounded px-2 py-0.5 hover:bg-slate-200 dark:hover:bg-slate-700 text-ember-600 dark:text-ember-400 font-medium"
            >
              run
            </button>
          )}
          <button
            type="button"
            onClick={handleCopy}
            className="rounded px-2 py-0.5 hover:bg-slate-200 dark:hover:bg-slate-700 transition-colors"
          >
            {copied ? 'copied ✓' : 'copy'}
          </button>
        </div>
      </div>
      <pre className="overflow-x-auto p-4 text-[13px] leading-6 font-mono">
        <code dangerouslySetInnerHTML={{ __html: renderedHtml }} />
      </pre>
    </div>
  );
};
