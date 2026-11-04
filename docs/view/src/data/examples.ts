// The runnable examples of the repository (lang/examples/), grouped by topic.
// The file list, each file's description (its leading comment) and the fogu:*
// modules it imports come from the repository at build time (examples-plugin.ts);
// this file only adds the topics, a description for the files without a
// leading comment, and which docs pages each example belongs to.
import files from 'virtual:fogu-examples';
import type { ExampleFile } from '../../examples-plugin';

export const REPO = 'https://github.com/microaitec-deploy/fogulang';
export const blobUrl = (path: string) => `${REPO}/blob/main/${path}`;
export const treeUrl = (path: string) => `${REPO}/tree/main/${path}`;

/** One example: a single file, or a folder (a project with several files). */
export interface Example {
  id: string; // the anchor on the Examples page: "ex-tls", "ex-orm"
  title: string; // "tls.fg", "orm/"
  path: string; // "examples/tls.fg", "examples/orm"
  folder: boolean;
  files: ExampleFile[]; // the files shown (main file first)
  desc: string;
  imports: string[];
  topic: string;
  pages: string[]; // docs pages it illustrates (section ids)
}

/** Topics, in display order: [title, one-line description]. */
export const TOPICS: [string, string][] = [
  ['Language basics', 'Variables, functions, errors as values, structs, modules.'],
  ['Concurrency & time', 'fg tasks, channels, sync primitives, timers, cron and events.'],
  ['Files, processes & the system', 'The file system, processes, the terminal, system information and services.'],
  ['Web & HTTP', 'HTTP servers and clients, fogu:web, auth, TLS, WebRTC and full-stack apps.'],
  ['Network sockets', 'Raw TCP and UDP servers and clients with fogu:net.'],
  ['Data & storage', 'Embedded key-value stores, SQL, the ORM, Redis, S3 and distributed databases.'],
  ['Messaging', 'Brokers and queues: NATS, RabbitMQ, MQTT, BullMQ and the Fogu broker.'],
  ['Integrations & automation', 'LLMs, e-mail, playbooks and running code at run time.'],
  ['Projects & packages', 'Complete projects: tests, dependencies from GitHub.'],
  ['Guide programs', 'The complete programs the Guides build step by step.'],
];

interface Meta {
  topic: string;
  title?: string;
  desc?: string; // when the file has no leading comment
  pages?: string[]; // docs pages besides the ones of the modules it imports
}

/** Keyed by the path below examples/ ("tls.fg", "orm/" for a folder). */
const META: Record<string, Meta> = {
  'basics.fg': { topic: 'Language basics', desc: 'Variables, types, operators, control flow, lists, maps, functions and templates.', pages: ['variables', 'operators', 'control', 'functions', 'collections', 'strings'] },
  'errors.fg': { topic: 'Language basics', pages: ['errors'] },
  'structs_concurrency.fg': { topic: 'Language basics', desc: '`struct` and `embed`, `match`, and `fg` tasks with `chan`, `select` and `timeout`.', pages: ['structs', 'concurrency'] },
  'modules.fg': { topic: 'Language basics', desc: '`import` / `export` between files (with util.fg) and the fogu:* modules.', pages: ['modules'] },
  'util.fg': { topic: 'Language basics', desc: 'The module imported by modules.fg: exported functions and constants.', pages: ['modules'] },

  'sync.fg': { topic: 'Concurrency & time', pages: ['concurrency'] },
  'time.fg': { topic: 'Concurrency & time', desc: 'Dates and times with fogu:time: `Date`, `delay` and `sleep`.' },
  'cron.fg': { topic: 'Concurrency & time', desc: 'Scheduled jobs with fogu:cron: `every`, `schedule` and `tick`.' },
  'events.fg': { topic: 'Concurrency & time', desc: 'An event bus with the `EventEmitter` of fogu:events.' },

  'files_processes.fg': { topic: 'Files, processes & the system', pages: ['fogu-object'] },
  'system.fg': { topic: 'Files, processes & the system', desc: 'System information with fogu:system, plus fogu:math and fogu:json.' },
  'cli.fg': { topic: 'Files, processes & the system' },
  'windows_service.fg': { topic: 'Files, processes & the system', pages: ['mod-service', 'build'] },

  'http_server.fg': { topic: 'Web & HTTP', desc: 'A small HTTP server with fogu:http on port 3000 (pair it with http_client.fg).' },
  'http_client.fg': { topic: 'Web & HTTP', desc: 'A request to the server of http_server.fg: status, headers and body.' },
  'web_server.fg': { topic: 'Web & HTTP', desc: 'A fogu:web app on port 3000: routes, route parameters and JSON responses.' },
  'web_auth.fg': { topic: 'Web & HTTP' },
  'web_fullstack.fg': { topic: 'Web & HTTP' },
  'tls.fg': { topic: 'Web & HTTP' },
  'password.fg': { topic: 'Web & HTTP' },
  'webrtc_chat.fg': { topic: 'Web & HTTP' },

  'tcp_server.fg': { topic: 'Network sockets', desc: 'A TCP server with fogu:net on port 9000 (pair it with tcp_client.fg).' },
  'tcp_client.fg': { topic: 'Network sockets', desc: 'A TCP client that talks to tcp_server.fg.' },
  'udp_server.fg': { topic: 'Network sockets', desc: 'A UDP server with fogu:net on port 9001 (pair it with udp_client.fg).' },
  'udp_client.fg': { topic: 'Network sockets', desc: 'A UDP client that talks to udp_server.fg.' },

  'kv.fg': { topic: 'Data & storage', desc: 'The embedded key-value store of fogu:kv (creates demo.redb).' },
  'sql.fg': { topic: 'Data & storage', desc: 'SQL with fogu:sql on SQLite (creates demo.db): tables, parameters and queries.' },
  'sql_multi_db.fg': { topic: 'Data & storage', desc: 'One program for SQLite, PostgreSQL and MySQL: the database is chosen by the URL.' },
  'orm/': { topic: 'Data & storage', title: 'orm/', desc: 'A Prisma-style ORM: the native fogu:orm (native.fg) next to an ORM written in Fogu (orm.fg, used by main.fg).', pages: ['mod-orm', 'mod-sql'] },
  'redis.fg': { topic: 'Data & storage' },
  's3.fg': { topic: 'Data & storage' },
  'surrealdb.fg': { topic: 'Data & storage', desc: 'SurrealDB over WebSocket or HTTP with fogu:surrealdb, including live queries.' },
  'etcd.fg': { topic: 'Data & storage', desc: 'etcd v3 with fogu:etcd: keys, prefixes and leases with a TTL.' },

  'nats.fg': { topic: 'Messaging' },
  'rabbitmq.fg': { topic: 'Messaging' },
  'mqtt.fg': { topic: 'Messaging' },
  'bullmq.fg': { topic: 'Messaging' },
  'fogumq.fg': { topic: 'Messaging', pages: ['mod-foguMQ-client'] },

  'ai.fg': { topic: 'Integrations & automation', pages: ['ai-agents', 'guide-ai'] },
  'mail.fg': { topic: 'Integrations & automation' },
  'vm.fg': { topic: 'Integrations & automation' },
  'playbook/': { topic: 'Integrations & automation', title: 'playbook/', desc: 'An Ansible-style playbook run by `fogu play`: an inventory, a site with tasks and a template.', pages: ['mod-playbook', 'automation-devops', 'guide-automation'] },

  'test/': { topic: 'Projects & packages', title: 'test/', desc: 'A project with fogu.json (`fogu run`, `fogu dev`, `fogu build --compile`) and its tests (`fogu test`).', pages: ['testing', 'mod-test', 'packages', 'build'] },
  'github_lib/': { topic: 'Projects & packages', title: 'github_lib/', pages: ['packages'] },

  'guides/cli-tool.fg': { topic: 'Guide programs', pages: ['guide-cli'] },
  'guides/rest-api.fg': { topic: 'Guide programs', pages: ['guide-rest-api'] },
  'guides/topcoat-app.fg': { topic: 'Guide programs', pages: ['guide-topcoat'] },
  'guides/concurrency.fg': { topic: 'Guide programs', pages: ['guide-concurrency', 'concurrency'] },
  'guides/errors.fg': { topic: 'Guide programs', pages: ['guide-errors', 'errors'] },
  'guides/storage.fg': { topic: 'Guide programs', pages: ['guide-storage'] },
  'guides/realtime.fg': { topic: 'Guide programs', pages: ['guide-realtime'] },
  'guides/broker.fg': { topic: 'Guide programs', pages: ['guide-broker'] },
  'guides/automation.fg': { topic: 'Guide programs', pages: ['guide-automation', 'automation-devops'] },
};

/** Folders shown as one project; their main file comes first. */
const FOLDERS: Record<string, string> = {
  orm: 'main.fg',
  playbook: 'site.yml',
  test: 'main.fg',
  github_lib: 'main.fg',
};

const slugify = (s: string) => s.toLowerCase().replace(/\.fg$/, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

function build(): Example[] {
  const out: Example[] = [];
  const byFolder = new Map<string, ExampleFile[]>();
  for (const f of files as ExampleFile[]) {
    const rel = f.path.replace(/^examples\//, '');
    const top = rel.split('/')[0];
    if (rel.includes('/') && FOLDERS[top]) {
      if (!byFolder.has(top)) byFolder.set(top, []);
      byFolder.get(top)!.push(f);
      continue;
    }
    if (!rel.endsWith('.fg')) continue; // README.md, fogu.json… of the examples root
    const meta = META[rel] ?? { topic: 'More examples' };
    out.push({
      id: 'ex-' + slugify(rel),
      title: meta.title ?? rel.split('/').pop()!,
      path: f.path,
      folder: false,
      files: [f],
      desc: f.desc || meta.desc || '',
      imports: f.imports,
      topic: meta.topic,
      pages: meta.pages ?? [],
    });
  }
  for (const [dir, fs] of byFolder) {
    const meta = META[dir + '/'] ?? { topic: 'Projects & packages' };
    const main = FOLDERS[dir];
    const shown = fs
      .filter((f) => !/(\.gitignore|fogu\.lock)$/.test(f.path))
      .sort((a, b) => (a.path.endsWith('/' + main) ? -1 : b.path.endsWith('/' + main) ? 1 : a.path.localeCompare(b.path)));
    const first = shown[0];
    out.push({
      id: 'ex-' + slugify(dir),
      title: meta.title ?? dir + '/',
      path: 'examples/' + dir,
      folder: true,
      files: shown,
      desc: meta.desc || first?.desc || '',
      imports: [...new Set(shown.flatMap((f) => f.imports))],
      topic: meta.topic,
      pages: meta.pages ?? [],
    });
  }
  const order = TOPICS.map((t) => t[0]);
  const rank = (t: string) => (order.indexOf(t) < 0 ? order.length : order.indexOf(t));
  return out.sort((a, b) => rank(a.topic) - rank(b.topic));
}

export const EXAMPLES: Example[] = build();

export const sourceUrl = (e: Example) => (e.folder ? treeUrl(e.path) : blobUrl(e.path));

/** The examples of a docs page: listed for it, or importing its module (mod-<name>). */
export function examplesFor(pageId: string): Example[] {
  const mod = pageId.startsWith('mod-') ? pageId.slice(4) : null;
  const direct = EXAMPLES.filter((e) => e.pages.includes(pageId));
  // files named after the module first (tls.fg for mod-tls), then the others importing it
  const named = mod ? EXAMPLES.filter((e) => !direct.includes(e) && e.imports.includes(mod) && e.title.replace(/\.fg$|\/$/, '').toLowerCase().split(/[_-]/).includes(mod.toLowerCase())) : [];
  const importing = mod ? EXAMPLES.filter((e) => !direct.includes(e) && !named.includes(e) && e.imports.includes(mod)) : [];
  return [...named, ...direct, ...importing];
}

/** The sources, loaded the first time one is shown. */
let code: Promise<Record<string, string>> | null = null;
export function loadCode(): Promise<Record<string, string>> {
  if (!code) code = import('virtual:fogu-examples-code').then((m) => m.default);
  return code;
}

export function langOf(path: string): string {
  if (path.endsWith('.fg')) return 'fogu';
  if (/\.ya?ml$/.test(path)) return 'yaml';
  if (path.endsWith('.json')) return 'json';
  if (path.endsWith('.md')) return 'markdown';
  return 'text';
}
