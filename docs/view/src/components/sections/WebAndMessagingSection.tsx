import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { RefSection } from '../RefSection';

interface Props {
  onRun: (code: string) => void;
}

export const WebAndMessagingSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= FOGU:HTTP (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-http" onRun={onRun} />

      {/* ================= FOGU:WEB (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-web" onRun={onRun} />

      {/* ================= FOGU:TOPCOAT (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-topcoat" onRun={onRun} />

      {/* ================= FOGU:JWT ================= */}
      <section id="mod-jwt" data-group="Web &amp; network" data-title="fogu:jwt" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:jwt</code></h2>
        <p>JSON Web Tokens with HMAC (HS256/HS384/HS512). A token that fails verification is an error value.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import jwt from "fogu:jwt"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>jwt.sign(payload, secret, {`{ algorithm, expiresIn, notBefore, issuer, audience, subject, jwtid, header, noTimestamp }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>iat</code> is added; durations or seconds for times.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>jwt.verify(token, secret, {`{ algorithms, issuer, audience, subject, leeway, maxAge, ignoreExpiration }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2">The claims.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>jwt.decode(token)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>{`{ header, payload }`}</code> without verifying.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>jwt.middleware(secret, {`{ property = "user", optional, …verify options }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">middleware</td><td className="py-2">For <code>fogu:web</code>: <code>Authorization: Bearer</code> → <code>ctx.user</code>, otherwise 401.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import jwt from "fogu:jwt"
const token = jwt.sign({ sub: "42", role: "admin" }, "s3cret", { expiresIn: 1h, issuer: "fogu" })
const claims = jwt.verify(token, "s3cret", { issuer: "fogu" })?
print(claims.sub, claims.role, claims.exp - claims.iat)
print(jwt.decode(token)?.header)
const [_, e1] = jwt.verify(token, "wrong")
const [_, e2] = jwt.verify(jwt.sign({ a: 1 }, "s3cret", { expiresIn: -10 }), "s3cret")
print(e1, e2)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`42 admin 3600
{alg: HS256, typ: JWT}
error: jwt: invalid signature error: jwt: token expired`}
        />
      </section>

      {/* ================= FOGU:WEBSOCKET (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-websocket" onRun={onRun} />

      {/* ================= FOGU:SOCKETIO (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-socketio" onRun={onRun} />

      {/* ================= FOGU:GRPC (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-grpc" onRun={onRun} />

      {/* ================= FOGU:SECURITY (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-security" onRun={onRun} />

      {/* ================= FOGU:NET (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-net" onRun={onRun} />

      {/* ================= FOGU:EVENTS ================= */}
      <section id="mod-events" data-group="Messaging &amp; events" data-title="fogu:events" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:events</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import events from "fogu:events"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>events.EventEmitter()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Emitter</td><td className="py-2">Construct event emitter.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>e.on(event, fn) · e.once(event, fn)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Listeners receive every emitted argument (declare them all).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>e.off(event, fn?) · e.removeAll(event?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Deregister listener.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>e.emit(event, ...args)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2"><code>true</code> when someone listened.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>e.listeners(event) · e.eventNames() · e.hasListeners(event)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Inspect emitter state.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import events from "fogu:events"
const bus = events.EventEmitter()
bus.on("order", (id, total) => print("order", id, "total", total))
bus.once("order", (id, total) => print("first order only:", id))
print(bus.emit("order", 1, 9.9))
print(bus.emit("order", 2, 15))
print(bus.emit("nobody-listens"), bus.eventNames())`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`order 1 total 9.9
first order only: 1
true
false
false []`}
        />
      </section>

      {/* ================= FOGU:CRON ================= */}
      <section id="mod-cron" data-group="Messaging &amp; events" data-title="fogu:cron" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:cron</code></h2>
        <p>Jobs run when the program calls <code>cron.tick()</code> (typically in its main loop).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import cron from "fogu:cron"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cron.every(interval, fn)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">A job every <code>interval</code> (duration or ms).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cron.schedule(expression, fn)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">5-field cron expression (<code>"*/5 * * * *"</code>); an invalid one stops the program.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cron.tick()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Runs the jobs that are due; returns how many ran.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cron.list()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Job names (<code>every(10ms)</code>, the cron expression…).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cron.clear()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Removes every job.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import cron from "fogu:cron"
import time from "fogu:time"
let beats = 0
cron.every(10ms, () => { beats = beats + 1 })
cron.schedule("0 3 * * *", () => print("nightly backup"))
for i in 0..6 {
    cron.tick()
    time.delay(10ms)
}
print(beats >= 3, cron.list())`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`true [every(10ms), schedule(0 3 * * *)]`}
        />
      </section>

      {/* ================= FOGU:NATS (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-nats" onRun={onRun} />

      {/* ================= FOGU:KAFKA (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-kafka" onRun={onRun} />

      {/* ================= FOGU:RABBITMQ ================= */}
      <RefSection id="mod-rabbitmq" onRun={onRun} />

      {/* ================= FOGU:MQTT ================= */}
      <RefSection id="mod-mqtt" onRun={onRun} />

      {/* ================= FOGU:BROKER (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-broker" onRun={onRun} />

      {/* ================= FOGU:FOGUMQ (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-fogumq" onRun={onRun} />

      {/* ================= FOGU:FOGUMQ-CLIENT (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-foguMQ-client" onRun={onRun} />

      {/* ================= FOGU:BULLMQ ================= */}
      <section id="mod-bullmq" data-group="Messaging &amp; events" data-title="fogu:bullmq" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:bullmq</code></h2>
        <p>Job queues on Redis, wire-compatible with <a className="underline" href="https://docs.bullmq.io">BullMQ</a> for Node.js v5 (on the <code>bullmq-rs</code> crate): Fogu and Node producers and workers can share the same queues, and dashboards such as Bull Board see them. The worker owns locks, stalled-job recovery, retries with backoff, concurrency and rate limits; each job is handed to the program, which finishes it with <code>complete</code> or <code>fail</code>. Redis failures are <code>error</code> values.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import bullmq from "fogu:bullmq"`} />
        
        <h3 className="text-lg font-semibold mt-6">Queue</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>bullmq.queue(name, {`{ connection, prefix = "bull" }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">BullQueue · error</td><td className="py-2"><code>connection</code> is a <code>redis://</code> / <code>rediss://</code> URL (default <code>$REDIS_URL</code> or <code>redis://127.0.0.1:6379</code>).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.add(name, data, opts?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">job · error</td><td className="py-2">Options: <code>delay attempts backoff: {`{ type: "fixed" | "exponential", delay, max }`} priority jobId ttl deduplication: {`{ id, ttl }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.addBulk([{`{ name, data, opts }`}])</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Batch enqueue jobs.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.getJob(id) · q.getJobs(states, start = 0, end = -1, asc = false)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">job | nil · list</td><td className="py-2">A job: <code>{`{ id name data state timestamp attemptsMade progress processedOn finishedOn failedReason returnvalue opts }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.counts() · q.count()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · int</td><td className="py-2">Per state: <code>wait active delayed prioritized completed failed paused waiting-children</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.upsertJobScheduler(id, {`{ every | pattern, tz, limit, immediately }`}, {`{ name, data, opts }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">job · error</td><td className="py-2">Repeatable jobs (cron <code>pattern</code> or <code>every</code> duration).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.removeJobScheduler(id)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · error</td><td className="py-2">Cancel scheduled job.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>q.pause() · resume() · isPaused() · drain() · remove(id) · clean(grace, limit, state) · obliterate({`{ force }`}) · getJobLogs(id)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Queue controls.</td></tr>
            </tbody>
          </table>
        </div>

        <h3 className="text-lg font-semibold mt-6">Worker and events</h3>
        <p>Limitation: the worker of <code>bullmq-rs</code> publishes <code>completed</code> events with an empty return value; <code>q.getJob(id).returnvalue</code> has the value given to <code>complete</code>.</p>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>bullmq.worker(name, {`{ connection, prefix, concurrency = 1, lockDuration, maxStalledCount, limiter: { max, duration } }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">BullWorker · error</td><td className="py-2">Worker instance with concurrency control.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>w.next(timeout?) · w.tryNext()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">BullJob | nil · error</td><td className="py-2">Waits cooperatively (other <code>fg</code> tasks keep running); nil on timeout or after <code>close()</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>job.complete(value?) · job.fail(reason)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2"><code>fail</code> retries the job while <code>attempts</code> remain. A job dropped without an answer fails.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>job.progress(value) · job.log(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Fields are those of the job map, plus <code>done</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>w.pause() · w.resume() · w.close() · w.running · w.pending</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Worker state controls.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>bullmq.events(name, {`{ connection, prefix, lastEventId }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">BullEvents · error</td><td className="py-2"><code>next(timeout?)</code> → <code>{`{ event, jobId, id, … }`}</code> (<code>added waiting active progress completed failed delayed stalled removed</code>).</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import bullmq from "fogu:bullmq"
const conn = { connection: "redis://127.0.0.1:6379" }
const q = bullmq.queue("emails", conn)?
q.add("welcome", { to: "ana@x.com" }, { attempts: 3, backoff: { type: "exponential", delay: 1s } })?
const w = bullmq.worker("emails", { connection: conn.connection, concurrency: 4 })?
while true {
    const job = w.next()?
    print(job.name, job.data.to, job.attemptsMade)
    job.progress(50)
    job.complete({ sent: true })          // or job.fail("smtp down") → retried
}`}
        />
        <DocCodeBlock
          lang="text"
          title="output (with Redis)"
          code={`welcome ana@x.com 0`}
        />
      </section>
    </>
  );
};
