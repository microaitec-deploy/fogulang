// A Vite plugin that turns lang/examples/ into two virtual modules:
//
//   virtual:fogu-examples        metadata of every example file (path, the
//                                description from its leading comment, the
//                                fogu:* modules it imports, its size)
//   virtual:fogu-examples-code   the source of every file, loaded on demand
//
// Only files tracked by git are listed (so the GitHub links always resolve);
// outside a git checkout every file of examples/ is used.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { Plugin } from 'vite';

const META = 'virtual:fogu-examples';
const CODE = 'virtual:fogu-examples-code';
const SHOWN = /\.(fg|ya?ml|json|conf|md)$/;

export interface ExampleFile {
  path: string; // relative to the repository root, e.g. "examples/tls.fg"
  desc: string; // the leading comment's first paragraph ("" when there is none)
  imports: string[]; // fogu:* modules
  lines: number;
}

function listFiles(root: string): string[] {
  try {
    const out = execFileSync('git', ['ls-files', 'examples'], { cwd: root, encoding: 'utf8' });
    const files = out.split('\n').filter(Boolean);
    if (files.length) return files;
  } catch {
    /* not a git checkout */
  }
  const walk = (dir: string): string[] =>
    fs.readdirSync(path.join(root, dir), { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name).split(path.sep).join('/')],
    );
  return walk('examples');
}

/** The first paragraph of the comment at the top of a file. */
export function leadingComment(src: string): string {
  const lines = src.replace(/^#!.*\n/, '').split('\n');
  const marker = /^(\/\/|#)/;
  const words: string[] = [];
  for (const raw of lines) {
    if (!marker.test(raw.trim())) break;
    const text = raw.trim().replace(/^(\/\/|#)/, '');
    if (/^\s{2,}\S/.test(text) && words.length) break; // an indented command line
    const t = text.trim();
    if (/^[-=]{4,}/.test(t)) continue; // a ruler
    if (!t) {
      if (words.length) break;
      continue;
    }
    if (/^(fogu|curl|docker|npm|cargo|printf|AWS|R2|[A-Z_]+=)\b/.test(t) && words.length) break;
    words.push(t);
  }
  let s = words.join(' ').replace(/\s+/g, ' ').trim();
  s = s.split(/(?<=[.!?])\s+(?=[A-Z`])/)[0]; // the first sentence
  if (/^fogu\s/.test(s) || /^(run with|run:)/i.test(s)) return '';
  return s.length >= 24 ? s : '';
}

export function foguExamples(): Plugin {
  const root = path.resolve(__dirname, '..', '..'); // lang/
  const load = () =>
    listFiles(root)
      .filter((f) => SHOWN.test(f))
      .map((f) => ({ f, src: fs.readFileSync(path.join(root, f), 'utf8') }));
  return {
    name: 'fogu-examples',
    resolveId(id) {
      if (id === META || id === CODE) return '\0' + id;
    },
    load(id) {
      if (id === '\0' + META) {
        const files: ExampleFile[] = load().map(({ f, src }) => ({
          path: f,
          desc: leadingComment(src),
          imports: [...new Set([...src.matchAll(/from\s+"fogu:([\w-]+)"/g)].map((m) => m[1]))],
          lines: src.split('\n').length,
        }));
        return `export default ${JSON.stringify(files)};`;
      }
      if (id === '\0' + CODE) {
        const code: Record<string, string> = {};
        for (const { f, src } of load()) code[f] = src;
        return `export default ${JSON.stringify(code)};`;
      }
    },
    configureServer(server) {
      server.watcher.add(path.join(root, 'examples'));
    },
  };
}
