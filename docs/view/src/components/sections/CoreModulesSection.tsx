import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { RefSection } from '../RefSection';

interface Props {
  onRun: (code: string) => void;
}

export const CoreModulesSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= LIBRARY OVERVIEW ================= */}
      <section id="library" data-group="Standard library" data-title="Overview" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold">Standard library</h2>
        <p>
          Modules are imported with <code>import name from "fogu:name"</code> (or by name: <code>import {`{ parse }`} from "fogu:json"</code>).
          Each section below lists every function with its parameters and return value, and shows an example with its <strong>real output</strong> (the examples are run when this page is generated).
        </p>

        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm">
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Core</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-math"><code>fogu:math</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-json"><code>fogu:json</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-time"><code>fogu:time</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-system"><code>fogu:system</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-fs"><code>fogu:fs</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-path"><code>fogu:path</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-process"><code>fogu:process</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-sync"><code>fogu:sync</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-test"><code>fogu:test</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-log"><code>fogu:log</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-regex"><code>fogu:regex</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-crypto"><code>fogu:crypto</code></a>
                </td>
              </tr>
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Data formats</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-csv"><code>fogu:csv</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-excel"><code>fogu:excel</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-yaml"><code>fogu:yaml</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-toml"><code>fogu:toml</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-xml"><code>fogu:xml</code></a>
                </td>
              </tr>
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Storage</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-kv"><code>fogu:kv</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-surrealkv"><code>fogu:surrealkv</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-surrealdb"><code>fogu:surrealdb</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-sql"><code>fogu:sql</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-orm"><code>fogu:orm</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-redis"><code>fogu:redis</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-s3"><code>fogu:s3</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-scylla"><code>fogu:scylla</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-etcd"><code>fogu:etcd</code></a>
                </td>
              </tr>
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Web &amp; network</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-http"><code>fogu:http</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-web"><code>fogu:web</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-topcoat"><code>fogu:topcoat</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-jwt"><code>fogu:jwt</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-websocket"><code>fogu:websocket</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-socketio"><code>fogu:socketio</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-grpc"><code>fogu:grpc</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-net"><code>fogu:net</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-security"><code>fogu:security</code></a>
                </td>
              </tr>
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Messaging &amp; events</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-nats"><code>fogu:nats</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-kafka"><code>fogu:kafka</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-rabbitmq"><code>fogu:rabbitmq</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-mqtt"><code>fogu:mqtt</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-broker"><code>fogu:broker</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-fogumq"><code>fogu:fogumq</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-foguMQ-client"><code>fogu:foguMQ-client</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-bullmq"><code>fogu:bullmq</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-events"><code>fogu:events</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-cron"><code>fogu:cron</code></a>
                </td>
              </tr>
              <tr>
                <td className="py-2 pr-4 font-medium whitespace-nowrap">Integrations</td>
                <td className="py-2 flex flex-wrap gap-x-2">
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-mail"><code>fogu:mail</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-ai"><code>fogu:ai</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-ssh"><code>fogu:ssh</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-shell"><code>fogu:shell</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-ffi"><code>fogu:ffi</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-playbook"><code>fogu:playbook</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-vm"><code>fogu:vm</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#mod-cli"><code>fogu:cli</code></a>
                  <a className="text-amber-600 dark:text-amber-400 underline" href="#embed"><code>C API (.so/.dll)</code></a>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <h3 className="text-lg font-semibold mt-6">Conventions</h3>
        <p>
          <strong>Errors are values.</strong> Operational failures — a missing file, a refused connection, a server error, malformed input — come back as an <code>error</code> value: handle them with <code>?</code> (propagate) or <code>const [value, err] = f()</code>. Programming mistakes — a wrong argument type, an unknown option — stop the program with a message.
          <strong> Options</strong> are passed as a last map argument: <code>f(x, {`{ timeout: 5s }`})</code>.
          <strong> Durations</strong> accept a duration literal (<code>500ms</code>, <code>2s</code>, <code>1h</code>) or an int number of milliseconds. Network operations wait cooperatively: other <code>fg</code> tasks keep running while one waits.
        </p>

        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import json from "fogu:json"

const [cfg, err] = json.parse("{ broken")      // an error value, not a crash
if err != nil {
    print("invalid config:", err.message)
}

fn load(text: str): error {
    const data = json.parse(text)?              // ? returns the error to the caller
    print("name:", data.name)
    return nil
}
print(load("{\\"name\\": \\"fogu\\"}"), load("nope") != nil)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`invalid config: json.parse: unterminated JSON string
name: fogu
nil true`}
        />
      </section>

      {/* ================= GLOBAL FUNCTIONS (generated: docs/tools/reference) ================= */}
      <RefSection id="globals" onRun={onRun} />

      {/* ================= STRINGS, LISTS, MAPS AND BUFFERS (generated: docs/tools/reference) ================= */}
      <RefSection id="values" onRun={onRun} />

      {/* ================= THE FOGU OBJECT (generated: docs/tools/reference) ================= */}
      <RefSection id="fogu-object" onRun={onRun} />

      {/* ================= FOGU:MATH ================= */}
      <section id="mod-math" data-group="Core modules" data-title="fogu:math" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:math</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import math from "fogu:math"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>math.PI · math.E</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">float</td><td className="py-2">Constants.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>math.abs(x)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">number</td><td className="py-2">Absolute value (int stays int).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>math.sqrt(x)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">float</td><td className="py-2">Square root.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>math.pow(base, exp)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">number</td><td className="py-2">Power.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>math.floor(x) · math.ceil(x)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Rounding down / up.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import math from "fogu:math"
print(math.abs(-4), math.sqrt(16), math.pow(2, 10), math.floor(2.7), math.ceil(2.1))
print(math.PI)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`4 4.0 1024.0 2 3
3.141592653589793`}
        />
      </section>

      {/* ================= FOGU:JSON ================= */}
      <section id="mod-json" data-group="Core modules" data-title="fogu:json" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:json</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import json from "fogu:json"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>json.parse(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">value · error</td><td className="py-2">JSON text → maps, lists, str, int, float, bool, nil. Invalid JSON is an error value.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>json.stringify(value)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Compact JSON (map keys in order).</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import json from "fogu:json"
const data = json.parse("{\\"name\\": \\"Ana\\", \\"tags\\": [\\"a\\", \\"b\\"], \\"age\\": 30}")?
print(data.name, data.tags, typeof(data.age))
print(json.stringify({ ok: true, list: [1, 2.5, nil] }))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`Ana [a, b] int
{"list":[1,2.5,null],"ok":true}`}
        />
      </section>

      {/* ================= FOGU:TIME ================= */}
      <section id="mod-time" data-group="Core modules" data-title="fogu:time" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:time</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import time from "fogu:time"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>time.now()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Milliseconds since the UNIX epoch.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>time.Date()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Date</td><td className="py-2">The current instant (UTC).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>time.delay(d) · time.sleep(seconds)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Waits; other <code>fg</code> tasks keep running.</td></tr>
            </tbody>
          </table>
        </div>

        <h3 className="text-lg font-semibold mt-6">Date</h3>
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.year · d.month · d.day</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Month 1–12.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.hour · d.minute · d.second · d.millis · d.weekday</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Weekday 0 = Sunday.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.getTime()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Milliseconds since the epoch (also <code>d.ms</code>).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.toISOString()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>2026-10-10T12:00:00.000Z</code></td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.toString()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Human readable.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.addMs(ms)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Moves the date by <code>ms</code> (in place).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>d.diff(other)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Milliseconds between the dates.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>getFullYear() getMonth() getDate() getDay() getHours() getMinutes() getSeconds() getMilliseconds()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">JavaScript-style getters (month 0–11).</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import time from "fogu:time"
const start = time.now()
time.delay(20ms)
print(time.now() - start >= 20)
const d = time.Date()
print(d.year > 2000, d.month >= 1, d.toISOString().ends_with("Z"))
const later = time.Date()
later.addMs(24 * 3600 * 1000)
print(later.diff(d) >= 86400000)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`true
true true true
true`}
        />
      </section>

      {/* ================= FOGU:SYSTEM ================= */}
      <section id="mod-system" data-group="Core modules" data-title="fogu:system" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:system</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import system from "fogu:system"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.args</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Script arguments (<code>fogu run app.fg a b</code> → <code>[a, b]</code>).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.time()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">float</td><td className="py-2">Seconds since the epoch.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.exit(code)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">—</td><td className="py-2">Ends the program.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.info.os · .arch · .family</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>windows</code>/<code>linux</code>/<code>macos</code> · <code>x86_64</code>/<code>aarch64</code> · <code>windows</code>/<code>unix</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.info.cores · .pid</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Logical CPUs · process id.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.info.env(name)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · nil</td><td className="py-2">An environment variable.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>system.info.sleep_ms(ms)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Blocks the whole program (prefer <code>time.delay</code>).</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import system from "fogu:system"
const info = system.info
print(typeof(info.os), info.cores > 0, typeof(info.pid), system.args)
print(info.env("FOGU_DOES_NOT_EXIST"), system.time() > 0)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`str true int []
nil true`}
        />
      </section>

      {/* ================= FOGU:FS ================= */}
      <section id="mod-fs" data-group="Core modules" data-title="fogu:fs" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:fs</code></h2>
        <p>Files and folders. Failures (missing file, permission) are error values.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import fs from "fogu:fs"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.readText(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">Whole file as UTF-8 text.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.readLines(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Lines without line endings.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.writeText(path, text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Creates or replaces.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.append(path, text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Adds to the end (creates the file).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.exists(path) · fs.isFile(path) · fs.isDir(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">State checks.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.mkdir(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Creates the folder and its parents.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.list(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Names inside a folder, sorted.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.stat(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>{`{ size, isFile, isDir, modified }`}</code> (modified: ms since the epoch).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.copy(from, to) · fs.move(from, to)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">File and folder transfer.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.remove(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">A file or an empty folder.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.removeAll(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">A folder and everything inside.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.glob(pattern)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Paths matching <code>*</code>, <code>?</code>, <code>**</code>, sorted.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.cwd() · fs.tempDir()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Working folder · system temp folder.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fs.remote(ssh | "user@host", {`{ sudo }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">RemoteFs</td><td className="py-2">The same functions on a server through <a className="underline" href="#mod-ssh">fogu:ssh</a> (plus <code>upload(local, remote)</code> and <code>download(remote, local)</code>); every call is an SSH command.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import fs from "fogu:fs"
fs.mkdir("data/logs")?
fs.writeText("data/a.txt", "hello")?
fs.append("data/a.txt", "\\nworld")?
print(fs.readLines("data/a.txt")?, fs.stat("data/a.txt")?.size)
fs.copy("data/a.txt", "data/logs/b.txt")?
print(fs.list("data")?, fs.glob("data/**/*.txt"))
const [_, err] = fs.readText("missing.txt")
print(err != nil)
fs.removeAll("data")?
print(fs.exists("data"))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`[hello, world] 11
[a.txt, logs] [data/a.txt, data/logs/b.txt]
true
false`}
        />
      </section>

      {/* ================= FOGU:PATH ================= */}
      <section id="mod-path" data-group="Core modules" data-title="fogu:path" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:path</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import path from "fogu:path"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.join(...parts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Joined with <code>/</code> and normalized.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.dirname(p) · path.basename(p)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Folder · last component.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.stem(p) · path.extname(p)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Name without extension · extension with the dot.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.normalize(p)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Resolves <code>.</code> and <code>..</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.isAbsolute(p)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">Check absolute path.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.absolute(p)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Absolute path from the working folder.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>path.sep</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">The system separator.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import path from "fogu:path"
print(path.join("src", "lib", "../main.fg"))
print(path.dirname("a/b/c.txt"), path.basename("a/b/c.txt"), path.stem("a/b/c.tar.gz"), path.extname("a/b/c.txt"))
print(path.normalize("a/./b/../c"), path.isAbsolute("/etc"), path.isAbsolute("etc"))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`src/main.fg
a/b c.txt c.tar .txt
a/c true false`}
        />
      </section>

      {/* ================= FOGU:PROCESS ================= */}
      <section id="mod-process" data-group="Core modules" data-title="fogu:process" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:process</code></h2>
        <p>A program that exits with a non-zero code is <em>not</em> an error: check <code>ok</code> and <code>code</code>. Not being able to start it is an error value. For pipes and Linux commands see <a className="underline" href="#mod-shell">fogu:shell</a>.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import process from "fogu:process"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.run(cmd, args?, {`{ cwd, env, input }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>{`{ ok, code, stdout, stderr }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.shell(command)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map</td><td className="py-2">Through <code>sh -c</code> / <code>cmd /C</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.env(name) · process.setEnv(name, value)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · nil</td><td className="py-2">Environment variable access.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.cwd() · process.chdir(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · nil</td><td className="py-2">Current directory management.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.pid · process.os · process.arch · process.args</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Process metadata.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>process.exit(code)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">—</td><td className="py-2">Exit runtime.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import process from "fogu:process"
const r = process.run("git", ["--version"])?
print(r.ok, r.code, r.stdout.starts_with("git version"))
const [_, err] = process.run("no-such-program-xyz")
print(err != nil)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`true 0 true
true`}
        />
      </section>

      {/* ================= FOGU:SYNC (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-sync" onRun={onRun} />

      {/* ================= FOGU:TEST ================= */}
      <section id="mod-test" data-group="Core modules" data-title="fogu:test" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:test</code></h2>
        <p>Used in <code>*.test.fg</code> files, run with <code>fogu test</code>. A test fails when an expectation fails, when it raises an error or returns an error value.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import { test, skip, expect } from "fogu:test"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>test(name, fn)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Declares a test.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>skip(name, fn)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">A test that is reported but not run.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>expect(value)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Expectation</td><td className="py-2">Then a matcher; <code>.not</code> negates it.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toBe(x) · toEqual(x)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Equality (<code>toEqual</code> compares lists and maps deeply).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toBeTruthy() · toBeFalsy() · toBeNil() · toBeError()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Truthy and type checks.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toBeType("int") · toContain(x) · toHaveLength(n)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Collection matchers.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toBeGreaterThan(n) · toBeLessThan(n) · toBeCloseTo(x, eps?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Numeric matchers.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="math.test.fg"
          code={`import { test, expect } from "fogu:test"

test("math", () => {
    expect(1 + 2).toBe(3)
    expect([1, 2]).toContain(2)
    expect("abc").not.toHaveLength(2)
    expect(0.1 + 0.2).toBeCloseTo(0.3)
})`}
        />
        <DocCodeBlock
          lang="text"
          title="fogu test"
          code={`$ fogu test
math.test.fg
  ✓ math (0ms)

1 passed — 1 file(s) [1ms]`}
        />
      </section>

      {/* ================= FOGU:LOG ================= */}
      <section id="mod-log" data-group="Core modules" data-title="fogu:log" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:log</code></h2>
        <p>Structured logs to stderr (default), stdout or a file. The level comes from <code>FOGU_LOG</code> (default <code>info</code>).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import log from "fogu:log"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.trace/debug/info/warn/error(message, fields?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2"><code>fields</code>: a map added as <code>key=value</code> (or JSON keys).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.setLevel(level)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2"><code>trace debug info warn error off</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.level()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Current level.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.setFormat("text" | "json")</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">One line per entry.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.setOutput("stderr" | "stdout" | path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">A file is appended to.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.setTime(bool)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Show the time stamp (default true).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>log.child(fields)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Logger</td><td className="py-2">A logger that adds <code>fields</code> to every line; same methods + <code>child</code>.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import log from "fogu:log"
log.setOutput("stdout")
log.setTime(false)
log.info("server started", { port: 3000 })
log.debug("not shown at the info level")
const db = log.child({ module: "db" })
db.warn("slow query", { ms: 812, sql: "select *" })
log.setFormat("json")
log.error("failed", { code: 23 })`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`INFO  server started port=3000
WARN  slow query module=db ms=812 sql="select *"
{"level":"error","msg":"failed","code":23}`}
        />
      </section>

      {/* ================= FOGU:REGEX ================= */}
      <section id="mod-regex" data-group="Core modules" data-title="fogu:regex" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:regex</code></h2>
        <p>Rust <code>regex</code> syntax (linear time, no backtracking). Positions are character indexes. In Fogu strings write <code>\\d</code> for <code>\d</code>. An invalid pattern is an error value.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import regex from "fogu:regex"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>regex.compile(pattern, {`{ ignoreCase, multiline, dotAll, verbose }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Regex · error</td><td className="py-2">Compile pattern.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.test(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">Matches somewhere.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.matches(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">The whole text matches.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.find(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil</td><td className="py-2"><code>{`{ text, start, end, groups, named }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.findAll(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Every match.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.replace(text, repl) · re.replaceAll(text, repl)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>$1</code>, <code>{`\${name}`}</code>, <code>$$</code> in <code>repl</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.split(text, limit?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Split by pattern.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>re.source · re.groups · re.names</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Pattern · group count · group names.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>regex.test/find/findAll/replace/replaceAll/split/matches(text, pattern, …)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">One-off shortcuts.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>regex.escape(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">A literal pattern.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import regex from "fogu:regex"
const email = regex.compile("(?P<user>[\\\\w.]+)@(?P<host>[\\\\w.]+)")?
print(email.test("write to ana@fogu.dev"))
const m = email.find("contact: ana@fogu.dev")
print(m.text, m.start, m.named.host, m.groups)
print(email.replaceAll("a@x.io, b@y.io", "\${user} (at \${host})"))
print(regex.split("one, two;three", "\\\\s*[,;]\\\\s*"))
print(regex.findAll("v1.2 and v3.10", "\\\\d+\\\\.\\\\d+").map((x) => x.text))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`true
ana@fogu.dev 9 fogu.dev [ana, fogu.dev]
a (at x.io), b (at y.io)
[one, two, three]
[1.2, 3.10]`}
        />
      </section>

      {/* ================= FOGU:CRYPTO ================= */}
      <section id="mod-crypto" data-group="Core modules" data-title="fogu:crypto" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:crypto</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import crypto from "fogu:crypto"`} />
        <div className="overflow-x-auto mt-4">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="text-left border-b border-slate-200 dark:border-slate-800">
                <th className="py-2 pr-4">Function</th>
                <th className="py-2 pr-4">Returns</th>
                <th className="py-2">Description</th>
              </tr>
            </thead>
            <tbody className="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.hash(algo, data, {`{ base64 }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>md5 sha1 sha256 sha384 sha512</code>; hex by default.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.hmac(algo, key, data, {`{ base64 }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">HMAC signature.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.encrypt(data, key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">AES-256-GCM; <code>key</code> is a password (argon2id) or 64 hex chars. Base64 output with salt and nonce.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.decrypt(box, key, {`{ bytes }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">Wrong key or altered data → error value.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.hashPassword(password)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">argon2id PHC string, for storing passwords.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.verifyPassword(password, hash)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">Verify argon2id hash.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.randomBytes(n) · crypto.randomHex(n)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · str</td><td className="py-2">Secure random.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.randomInt(min, max)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Inclusive, unbiased.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.uuid()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Random UUID v4.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.base64(data, {`{ url }`}) · crypto.fromBase64(text, {`{ bytes }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>data</code>: str or list of bytes.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.hex(data) · crypto.fromHex(text, {`{ bytes }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Hex conversion.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>crypto.equal(a, b)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">Constant-time comparison.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import crypto from "fogu:crypto"
print(crypto.hash("sha256", "abc"))
print(crypto.hmac("sha256", "key", "The quick brown fox jumps over the lazy dog"))
const box = crypto.encrypt("card 4242", "my secret")
print(crypto.decrypt(box, "my secret")?)
const [_, err] = crypto.decrypt(box, "wrong")
print(err)
const h = crypto.hashPassword("hunter2")
print(crypto.verifyPassword("hunter2", h), crypto.verifyPassword("hunter3", h))
print(crypto.base64("olá"), crypto.fromBase64("b2zDoQ=="), crypto.hex("hi"), len(crypto.uuid()))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
card 4242
error: crypto.decrypt: wrong key or corrupted data
true false
b2zDoQ== olá 6869 36`}
        />
      </section>
    </>
  );
};
