import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { BENCHMARKS } from '../../data/benchmarks.generated';

const COLORS: Record<string, string> = {
  'Fogu': 'bg-orange-500',
  'Fogu (no JIT)': 'bg-amber-300 dark:bg-amber-400/70',
  'Python': 'bg-sky-500',
  'Bun': 'bg-pink-500',
};

const fmt = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${Math.round(ms)} ms`);

export const BenchmarksSection: React.FC = () => {
  const b = BENCHMARKS;
  return (
    <section id="benchmarks" data-group="Performance" data-title="Benchmarks" className="scroll-mt-20 mt-16">
      <h2 className="text-2xl font-bold">Benchmarks: Fogu vs Python vs Bun</h2>
      <p className="mt-3">
        The same eight programs written in Fogu, Python and JavaScript (run by Bun). Each number is the{' '}
        <b>median of {b.runs} runs</b> of the whole process, start-up included; lower is better. The languages are
        interleaved (each repetition runs every language once), so changes in the machine's load affect all of them
        alike, and the runner checks that the three languages print the same result.
      </p>
      <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
        {b.cpu} ({b.cores} logical CPUs) · {b.os} · {Object.values(b.versions).join(' · ')} · measured {b.date}
      </p>

      <div className="flex flex-wrap gap-4 mt-6 text-sm">
        {b.langs.map((l) => (
          <span key={l} className="inline-flex items-center gap-2">
            <span className={`inline-block w-3 h-3 rounded-sm ${COLORS[l]}`} /> {l}
          </span>
        ))}
      </div>

      <div className="mt-4 grid gap-5">
        {b.rows.map((row) => {
          const times = b.langs.map((l) => (row.times as Record<string, number>)[l]).filter((t) => t != null);
          const max = Math.max(...times);
          const min = Math.min(...times);
          return (
            <div key={row.bench} className="rounded-xl border border-slate-200 dark:border-slate-800 p-4">
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <h3 className="font-semibold"><code>{row.bench}</code></h3>
                <span className="text-sm text-slate-500 dark:text-slate-400">{row.about}</span>
              </div>
              <div className="mt-3 grid gap-1.5">
                {b.langs.map((l) => {
                  const t = (row.times as Record<string, number>)[l];
                  if (t == null) return null;
                  return (
                    <div key={l} className="grid grid-cols-[8.5rem_1fr_5rem] items-center gap-3 text-sm">
                      <span className="text-slate-600 dark:text-slate-300 truncate">{l}</span>
                      <div className="h-3 rounded bg-slate-100 dark:bg-slate-800/70 overflow-hidden">
                        <div className={`h-full rounded ${COLORS[l]}`} style={{ width: `${Math.max(1.5, (t / max) * 100)}%` }} />
                      </div>
                      <span className={`text-right tabular-nums ${t === min ? 'font-semibold' : ''}`}>{fmt(t)}</span>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>

      {b.rows.some((r) => r.before != null) && (
        <>
          <h3 className="text-lg font-semibold mt-10">Before and after the optimizations</h3>
          <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
            Fogu with the JIT, median in ms: <code>bench/baseline.json</code> (an earlier run) against this one — two
            separate runs on the same machine, so differences of a few percent are noise.
          </p>
          <div className="overflow-x-auto mt-3">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                  <th className="py-2 pr-4">benchmark</th>
                  <th className="py-2 pr-4 text-right">before</th>
                  <th className="py-2 pr-4 text-right">now</th>
                  <th className="py-2 text-right">change</th>
                </tr>
              </thead>
              <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
                {b.rows.map((row) => {
                  const now = (row.times as Record<string, number>)['Fogu'];
                  const before = row.before as number | null;
                  if (before == null || now == null) return null;
                  const pct = ((now - before) / before) * 100;
                  const color = pct < -3 ? 'text-emerald-600 dark:text-emerald-400' : pct > 3 ? 'text-red-600 dark:text-red-400' : 'text-slate-500';
                  return (
                    <tr key={row.bench}>
                      <td className="py-2 pr-4"><code>{row.bench}</code></td>
                      <td className="py-2 pr-4 text-right tabular-nums">{Math.round(before)}</td>
                      <td className="py-2 pr-4 text-right tabular-nums">{Math.round(now)}</td>
                      <td className={`py-2 text-right tabular-nums ${color}`}>{pct > 0 ? '+' : ''}{pct.toFixed(0)}%</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </>
      )}

      {b.net && <NetResults n={b.net as unknown as Net} />}

      <h3 className="text-lg font-semibold mt-10">Optimizations made</h3>
      <ul className="list-disc pl-6 mt-2 space-y-2">
        {b.changes.map((c) => (
          <li key={c.title}><b>{c.title}.</b> {c.detail}</li>
        ))}
      </ul>

      <h3 className="text-lg font-semibold mt-10">Reading the numbers</h3>
      <ul className="list-disc pl-6 mt-2 space-y-1">
        <li>
          <b>Fogu</b> runs with its Cranelift JIT (the default): functions and loops over numbers are compiled to
          machine code after a few calls or iterations, which is why <code>fib</code>, <code>loop</code> and{' '}
          <code>mandel</code> land near Bun.
        </li>
        <li>
          <b>Fogu (no JIT)</b> is the tree-walking interpreter (<code>fogu --no-jit</code>), the semantic reference
          of the language. It is slower than CPython on call-heavy code.
        </li>
        <li>
          <code>strings</code>, <code>maps</code>, <code>records</code> and <code>sort</code> spend their time in the
          runtime's Rust code and in the calls around it; the JIT does not compile code that works on strings, lists
          or maps yet.
        </li>
        <li>Start-up: native modules are built on their first import, so a script starts in a few milliseconds.</li>
      </ul>

      <h3 className="text-lg font-semibold mt-8">Run them yourself</h3>
      <DocCodeBlock
        lang="bash"
        code={`cargo build --release
target/release/fogu bench/run.fg             # 5 runs per case (needs python3 and bun)
target/release/fogu bench/run.fg 3 fib loop  # fewer runs, some cases
python docs/tools/gen_bench.py               # refresh docs/benchmarks.md and this page`}
      />
      <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
        The programs are in <code>bench/</code>: <code>&lt;name&gt;.fg</code>, <code>&lt;name&gt;.py</code>,{' '}
        <code>&lt;name&gt;.js</code>.
      </p>
    </section>
  );
};

interface Net {
  date: string;
  rounds: number;
  versions: Record<string, string>;
  load: [number, number] | null;
  http: { name: string; rps: number; p50: number; p99: number; errors: number }[];
  ws: { name: string; mps: number; errors: number }[];
  tasks: { count: number; perTask: { name: string; bytes: number }[] } | null;
  sleeps: { name: string; ms: number }[];
  pingpong: { name: string; ms: number }[];
  memoryNotes: { name: string; text: string }[];
  method: string[];
}

const Table: React.FC<{ head: string[]; rows: (string | number)[][] }> = ({ head, rows }) => (
  <div className="overflow-x-auto mt-3">
    <table className="w-full text-sm">
      <thead>
        <tr className="text-left border-b border-slate-200 dark:border-slate-800">
          {head.map((h, i) => <th key={h} className={`py-2 pr-4 ${i ? 'text-right' : ''}`}>{h}</th>)}
        </tr>
      </thead>
      <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
        {rows.map((r) => (
          <tr key={String(r[0])}>
            {r.map((c, i) => <td key={i} className={`py-2 pr-4 ${i ? 'text-right tabular-nums' : ''}`}>{c}</td>)}
          </tr>
        ))}
      </tbody>
    </table>
  </div>
);

const n0 = (x: number) => Math.round(x).toLocaleString('en-US');

const NetResults: React.FC<{ n: Net }> = ({ n }) => (
  <>
    <h3 className="text-lg font-semibold mt-10">Servers and concurrency</h3>
    <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
      Median of {n.rounds} rounds, every implementation once per round · {Object.values(n.versions).join(' · ')}
      {n.load && ` · machine load ${n.load[0].toFixed(1)} → ${n.load[1].toFixed(1)}`} · measured {n.date}
    </p>
    {n.http.length > 0 && (
      <>
        <h4 className="font-semibold mt-6">HTTP throughput</h4>
        <Table head={['server', 'requests/s', 'p50', 'p99', 'non-200']} rows={n.http.map((x) => [x.name, n0(x.rps), `${x.p50.toFixed(2)} ms`, `${x.p99.toFixed(2)} ms`, x.errors])} />
      </>
    )}
    {n.ws.length > 0 && (
      <>
        <h4 className="font-semibold mt-6">WebSocket echo</h4>
        <Table head={['server', 'messages/s', 'errors']} rows={n.ws.map((x) => [x.name, n0(x.mps), x.errors])} />
      </>
    )}
    {n.tasks && (
      <>
        <h4 className="font-semibold mt-6">Memory per task ({n0(n.tasks.count)} tasks)</h4>
        <Table head={['runtime', 'per task']} rows={n.tasks.perTask.map((x) => [x.name, `${(x.bytes / 1024).toFixed(1)} KB`])} />
        <p className="mt-3 text-sm">What it counts (peak RSS with N tasks minus with none, divided by N):</p>
        <ul className="list-disc pl-6 mt-1 text-sm space-y-1">
          {n.memoryNotes.map((m) => <li key={m.name}><b>{m.name}:</b> {m.text}</li>)}
        </ul>
      </>
    )}
    {n.sleeps.length > 0 && (
      <>
        <h4 className="font-semibold mt-6">10,000 concurrent waits of 100 ms</h4>
        <Table head={['runtime', 'total']} rows={n.sleeps.map((x) => [x.name, `${n0(x.ms)} ms`])} />
      </>
    )}
    {n.pingpong.length > 0 && (
      <>
        <h4 className="font-semibold mt-6">100,000 channel round trips</h4>
        <Table head={['runtime', 'total']} rows={n.pingpong.map((x) => [x.name, `${n0(x.ms)} ms`])} />
      </>
    )}
    <h4 className="font-semibold mt-6">Method</h4>
    <ul className="list-disc pl-6 mt-1 text-sm space-y-1">{n.method.map((m) => <li key={m}>{m}</li>)}</ul>
  </>
);
