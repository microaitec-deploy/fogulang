import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { RefSection } from '../RefSection';

interface Props {
  onRun: (code: string) => void;
}

export const AdvancedModulesSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= FOGU:IMAGE (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-image" onRun={onRun} />

      {/* ================= FOGU:PLOT ================= */}
      <section id="mod-plot" data-group="Images &amp; charts" data-title="fogu:plot" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:plot</code></h2>
        <p>Charts in the style of matplotlib — line, scatter, area, bar (grouped), histogram and pie/donut — drawn as SVG and saved as SVG, PNG, JPEG or WebP. Works with lists and ranges (arrays and data frames from the <code>datascience</code> lib: pass <code>a.toList()</code> / <code>df.col(name)</code>).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import plot from "fogu:plot"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>plot.figure({`{ title, xlabel, ylabel, width = 800, height = 450, theme = "light" | "dark", grid = true, legend = true, xmin, xmax, ymin, ymax, background }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Figure</td><td className="py-2">Create figure.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fig.line(x?, y, opts) · scatter(…) · area(…)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2"><code>line(y)</code> uses 0, 1, 2… as x.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fig.bar(categories, values, {`{ label, color }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Several <code>bar</code> calls are grouped.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fig.hist(values, {`{ bins = 10 }`}) · fig.pie(labels, values, {`{ donut }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Histograms and pie charts.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>fig.svg() · fig.image({`{ scale }`}) · fig.save(path, {`{ scale = 2, quality }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · Image · nil</td><td className="py-2">Save or render.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>plot.line(x, y, options) · plot.bar · plot.scatter · plot.area · plot.hist · plot.pie</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Figure</td><td className="py-2">One-series shortcuts.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import plot from "fogu:plot"
const fig = plot.figure({ title: "Sales", xlabel: "month", ylabel: "€" })
fig.area([1, 2, 3, 4, 5, 6], [10, 14, 9, 17, 21, 19], { label: "2025" })
fig.line([1, 2, 3, 4, 5, 6], [8, 11, 13, 15, 14, 22], { label: "2026", dash: true, marker: true })
fig.save("sales.png")?
const langs = plot.figure({ title: "Languages", theme: "dark" })
langs.bar(["Fogu", "Rust", "Go"], [42, 30, 25], { label: "2025" })
langs.bar(["Fogu", "Rust", "Go"], [55, 33, 22], { label: "2026" })
langs.save("langs.svg")?
print(plot.pie(["A", "B"], [70, 30], { donut: true }).svg().starts_with("<svg"))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`true`}
        />
      </section>

      {/* ================= FOGU:DOCKER ================= */}
      <section id="mod-docker" data-group="Containers &amp; services" data-title="fogu:docker" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:docker</code></h2>
        <p>Control Docker or Podman (containers, images, networks, volumes, compose projects, Swarm services) and write or edit Dockerfiles and compose files. Commands use the <code>docker</code>/<code>podman</code> CLI (<code>FOGU_CONTAINER_ENGINE</code> or <code>docker.use("podman")</code>); <code>docker.remote(ssh)</code> runs everything on a server through <a className="underline" href="#mod-ssh">fogu:ssh</a>.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import docker from "fogu:docker"`} />
        
        <h3 className="text-lg font-semibold mt-6">Containers, images, networks</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>docker.run(image, opts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">The container id (or the output when <code>detach: false</code>).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>docker.start · stop · restart · kill · rm(name, opts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Container lifecycle.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>docker.compose(file?, opts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">ComposeProject</td><td className="py-2">Manage compose projects.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          code={`import docker from "fogu:docker"
const df = docker.dockerfile("node:20-alpine", { as: "app" })
df.workdir("/app")
df.copy("package*.json", ".")
df.run(["npm ci --omit=dev", "npm cache clean --force"])
df.env({ NODE_ENV: "production" })
df.expose(3000)
df.cmd(["node", "server.js"])
print(df.toString())
docker.writeCompose("compose.yaml", { services: { web: { build: ".", ports: ["3000:3000"], restart: "always" } } })?
print(docker.composeString(docker.readCompose("compose.yaml")?))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`FROM node:20-alpine AS app
WORKDIR /app
COPY package*.json .
RUN npm ci --omit=dev \\
    && npm cache clean --force
ENV NODE_ENV=production
EXPOSE 3000
CMD ["node","server.js"]

services:
  web:
    build: .
    restart: always
    ports:
    - 3000:3000`}
        />
      </section>

      {/* ================= FOGU:SERVICE ================= */}
      <section id="mod-service" data-group="Containers &amp; services" data-title="fogu:service" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:service</code></h2>
        <p>Linux services with systemd: generate unit files and <code>service.sh</code> management scripts, install, deploy, control and inspect them — locally or on a server with <code>{`{ ssh }`}</code>. Commands run <code>systemctl</code>/<code>journalctl</code> (with <code>sudo -n</code> when <code>sudo: true</code>).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import service from "fogu:service"`} />
        <DocCodeBlock
          title="example.fg"
          code={`import service from "fogu:service"
const kv = service.define({
    name: "microaitec-kv",
    exec: "/apps/microaitec/kv/one_kv",
    args: ["--port", "9000"],
    workdir: "/apps/microaitec/kv",
    env: { RUST_LOG: "info" },
})
print(kv.unit())
kv.writeScript("service.sh")?           // ./service.sh create | start | stop | restart | status | remove | logs
print(service.parseUnit(kv.unit()).Service.ExecStart)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`[Unit]
Description=microaitec-kv
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/apps/microaitec/kv
Environment="RUST_LOG=info"
ExecStart=/apps/microaitec/kv/one_kv --port 9000
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target

/apps/microaitec/kv/one_kv --port 9000`}
        />
      </section>

      {/* ================= FOGU:ZIP (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-zip" onRun={onRun} />

      {/* ================= FOGU:PROTOBUF (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-protobuf" onRun={onRun} />
    </>
  );
};
