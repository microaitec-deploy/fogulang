"""Standard library reference: containers and services."""

O = "Containers & services"

SECTIONS = [
    {
        "id": "mod-docker",
        "group": O,
        "title": "fogu:docker",
        "import": 'import docker from "fogu:docker"',
        "intro": "Control Docker or Podman (containers, images, networks, volumes, compose projects, Swarm services) and "
        "write or edit Dockerfiles and compose files. Commands use the <code>docker</code>/<code>podman</code> CLI "
        "(<code>FOGU_CONTAINER_ENGINE</code> or <code>docker.use(\"podman\")</code>); <code>docker.remote(ssh)</code> "
        "runs everything on a server through <a href=\"#mod-ssh\">fogu:ssh</a>. A failing command is an error value "
        "with its stderr.",
        "blocks": [
            {
                "h3": "Containers, images, networks",
                "api": [
                    ("docker.engine · docker.available() · docker.version() · docker.raw(args, { cwd })", "", "<code>raw</code> never fails on exit codes."),
                    ("docker.ps({ all, filter }) · docker.status(container)", "list · map", "<code>status</code>: running, state, health, exitCode, startedAt, restarts, image."),
                    ("docker.run(image, { name, detach = true, rm, ports, env, volumes, network, restart, labels, workdir, user, entrypoint, command, args })", "str · error", "The container id (or the output when <code>detach: false</code>)."),
                    ("docker.start(name) · stop · restart · kill · pause · unpause · rm(name, { force, volumes })", "nil · error", ""),
                    ("docker.logs(name, { tail, since, timestamps }) · exec(name, command, { user, workdir, env }) · inspect(name) · stats() · cp(from, to)", "", ""),
                    ("docker.images() · pull · push · tag · rmi · build(context, { tag, file, args, target, noCache, platform })", "", ""),
                    ("docker.networks() · volumes() · createNetwork · removeNetwork · createVolume · removeVolume · prune(what)", "", ""),
                ],
            },
            {
                "h3": "Compose and Swarm services",
                "api": [
                    ("docker.compose(file?, { project, cwd, envFile })", "ComposeProject", "On a remote engine a local compose file is copied first."),
                    ("p.up(services?, { detach = true, build, pull, wait, removeOrphans }) · down({ volumes, images }) · ps() · logs(svc?, { tail })", "", ""),
                    ("p.services() · p.status(service)", "list · map", "<code>{ name, state, running, status, health, containers, ports }</code>, including services not created yet."),
                    ("p.restart(svc?) · stop · start · pull · build · rm · exec(service, command) · config()", "", ""),
                    ("docker.services({ filter }) · docker.service(name)", "list · SwarmService", "Swarm: <code>status()</code>, <code>tasks()</code>, <code>restart()</code> (rolling), <code>stop()</code>, <code>start(n)</code>, <code>scale(n)</code>, <code>update({ image, replicas, env })</code>, <code>logs()</code>, <code>remove()</code>."),
                    ("docker.readCompose(path) · writeCompose(path, value) · composeString(value)", "", "Compose files as maps, written with the usual key order."),
                ],
            },
            {
                "h3": "Dockerfiles",
                "api": [
                    ("docker.dockerfile(from?, { as, platform }) · parseDockerfile(text) · readDockerfile(path)", "Dockerfile", ""),
                    ("df.from · run(cmd | [cmds]) · copy(src, dest, { from, chown, chmod }) · add · workdir · env(k, v | map) · arg · label · expose(…) · volume(…) · user · cmd · entrypoint · healthcheck(cmd, { interval, timeout, retries }) · comment · raw", "nil", "A list of commands becomes one <code>RUN</code> joined with <code>&amp;&amp;</code>."),
                    ("df.instructions · find(instr) · replace(instr, value) · insert(i, instr, value) · remove(i)", "", "Edit an existing Dockerfile."),
                    ("df.toString() · df.save(path = \"Dockerfile\")", "", ""),
                ],
                "example": r'''
import docker from "fogu:docker"
const df = docker.dockerfile("node:20-alpine", { as: "app" })
df.workdir("/app")
df.copy("package*.json", ".")
df.run(["npm ci --omit=dev", "npm cache clean --force"])
df.env({ NODE_ENV: "production" })
df.expose(3000)
df.cmd(["node", "server.js"])
print(df.toString())
docker.writeCompose("compose.yaml", { services: { web: { build: ".", ports: ["3000:3000"], restart: "always" } } })?
print(docker.composeString(docker.readCompose("compose.yaml")?))
''',
                "run": True,
            },
            {
                "example": r'''
import docker from "fogu:docker"
import ssh from "fogu:ssh"
const id = docker.run("nginx:alpine", { name: "web", ports: { "8080": "80" } })?
print(docker.status("web")?.running, docker.exec("web", "nginx -v")?.stderr)
docker.rm("web", { force: true })?
const app = docker.compose("compose.yaml", { project: "shop" })
app.up()?
for s in app.services()? { print(s.name, s.state) }
app.restart("web")?
const prod = docker.remote(ssh.connect("deploy@prod.example.com")?)
print(prod.ps()?)
''',
                "output": "true nginx version: nginx/1.27.2\nweb running\nworker running\n[{Names: shop-web-1, State: running, …}]",
                "output_title": "output (with Docker)",
            },
        ],
    },
    {
        "id": "mod-service",
        "group": O,
        "title": "fogu:service",
        "import": 'import service from "fogu:service"',
        "intro": "Linux services with systemd: generate unit files and <code>service.sh</code> management scripts, "
        "install, deploy, control and inspect them — locally or on a server with <code>{ ssh }</code>. Commands run "
        "<code>systemctl</code>/<code>journalctl</code> (with <code>sudo -n</code> when <code>sudo: true</code>).",
        "blocks": [
            {
                "api": [
                    ("service.define({ name, exec, args, description, user = \"root\", group, workdir, env, envFile, after = \"network.target\", type = \"simple\", restart = \"always\", restartSec = 5, limitNofile = 65535, wantedBy, extra, sudo, dir, ssh })", "Service", "<code>extra</code>: <code>{ Service: { MemoryMax: \"1G\" } }</code>; <code>ssh</code>: a connection or <code>\"user@host\"</code>."),
                    ("svc.unit() · svc.script()", "str", "The <code>.service</code> file · a bash script: <code>create start stop restart status remove logs</code>."),
                    ("svc.writeUnit(path?) · svc.writeScript(path = \"service.sh\") · svc.installScript(path)", "nil · error", "<code>installScript</code> writes it on the (remote) host."),
                    ("svc.install() · svc.uninstall()", "nil · error", "Unit file, daemon-reload, enable · stop, disable, remove."),
                    ("svc.deploy(localBinary)", "nil · error", "Copies the program to its <code>exec</code> path (scp on a server), installs and restarts."),
                    ("svc.start() · stop · restart · reload · enable · disable · isActive() · isEnabled() · logs({ lines, since })", "", ""),
                    ("svc.status() · service.status(name, { ssh })", "map · error", "<code>{ active, sub, enabled, running, pid, memory, since, exitStatus, description }</code>."),
                    ("service.list({ all, ssh }) · service.start(name, { sudo, ssh }) · stop · restart · reload · enable · disable · logs(name, opts) · daemonReload()", "", ""),
                    ("service.parseUnit(text) · service.readUnit(name | path, { ssh }) · service.unitString(map)", "", "Unit files as <code>{ Unit: {…}, Service: {…}, Install: {…} }</code>."),
                ],
                "example": r'''
import service from "fogu:service"
const kv = service.define({
    name: "microaitec-kv",
    exec: "/apps/microaitec/kv/one_kv",
    args: ["--port", "9000"],
    workdir: "/apps/microaitec/kv",
    env: { RUST_LOG: "info" },
})
print(kv.unit())
kv.writeScript("service.sh")?           // ./service.sh create | start | stop | restart | status | remove | logs
print(service.parseUnit(kv.unit()).Service.ExecStart)
''',
                "run": True,
            },
            {
                "example": r'''
import service from "fogu:service"
import ssh from "fogu:ssh"
const server = ssh.connect("deploy@prod.example.com")?
const api = service.define({ name: "api", exec: "/opt/api/api", workdir: "/opt/api", sudo: true, ssh: server })
api.deploy("target/release/api")?       // upload, install the unit, restart
print(api.status()?.running)
print(api.logs({ lines: 20 })?)
''',
                "output": "true\n2026-10-15T20:12:01+00:00 prod api[4121]: listening on :8080",
                "output_title": "output (with a server)",
            },
        ],
    },
]
