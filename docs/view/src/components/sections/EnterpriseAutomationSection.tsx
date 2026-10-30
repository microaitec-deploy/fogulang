import React, { useState } from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { ArchitectureDiagram } from '../ArchitectureDiagram';
import { InteractivePlayground } from '../InteractivePlayground';
import {
  Cog,
  Server,
  Terminal,
  Shield,
  Zap,
  Network,
  Bot,
  Layers,
  Database,
  Workflow,
  Cpu,
  Clock,
  CheckCircle2,
  AlertTriangle,
  Play
} from 'lucide-react';

interface Props {
  onRun: (code: string) => void;
}

export const EnterpriseAutomationSection: React.FC<Props> = ({ onRun }) => {
  const [activeAutoTab, setActiveAutoTab] = useState<'playbook' | 'ssh' | 'shell' | 'cron' | 'events'>('playbook');
  const [activeScalabilityTab, setActiveScalabilityTab] = useState<'nats' | 'rabbitmq' | 'redis' | 'scylla' | 'circuit'>('nats');
  const [activeAiTab, setActiveAiTab] = useState<'agent' | 'streaming' | 'tools' | 'rag'>('agent');

  return (
    <>
      {/* ================= 1. SYSTEM ARCHITECTURE ================= */}
      <section id="enterprise-arch" data-group="Enterprise & Automation" data-title="System Architecture" className="scroll-mt-20 mt-16">
        <div className="pb-4 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-orange-600 dark:text-orange-400">
            <Layers className="w-4 h-4" />
            <span>High-Availability Infrastructure</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            Enterprise System Architecture
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-3xl">
            Fôgu is engineered from the ground up for mission-critical automation, distributed microservices,
            and autonomous AI agents. Explore the visual system topology below to inspect each architectural layer.
          </p>
        </div>

        {/* Visual Interactive Architecture Diagram */}
        <div className="mt-6">
          <ArchitectureDiagram />
        </div>
      </section>

      {/* ================= 2. AUTOMATION & ORCHESTRATION ================= */}
      <section id="automation-devops" data-group="Enterprise & Automation" data-title="Automation & Orchestration" className="scroll-mt-20 mt-20">
        <div className="pb-4 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-orange-600 dark:text-orange-400">
            <Cog className="w-4 h-4" />
            <span>DevOps, SRE & Systems Orchestration</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            Automation & Infrastructure Functions
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-3xl">
            Fôgu eliminates fragile Bash scripts and heavy Python operational dependencies. With built-in modules
            for declarative idempotent playbooks, secure multi-host SSH fleet execution, cross-platform POSIX pipelines,
            and sub-millisecond cron scheduling, automation is treated as a first-class language citizen.
          </p>
        </div>

        {/* 3 Core Pillars */}
        <div className="mt-6 grid gap-4 sm:grid-cols-3">
          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="w-8 h-8 rounded-lg bg-orange-100 dark:bg-orange-950/40 text-orange-600 dark:text-orange-400 flex items-center justify-center font-bold mb-3">
              <Server className="w-4 h-4" />
            </div>
            <h3 className="font-semibold text-slate-900 dark:text-white text-sm">Strict Idempotency</h3>
            <p className="mt-1 text-xs text-slate-600 dark:text-slate-400 leading-relaxed">
              Modules like <code>fogu:playbook</code> and <code>fogu:fs</code> only apply mutations when current state diverges from target state, preventing accidental restarts across production fleets.
            </p>
          </div>

          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="w-8 h-8 rounded-lg bg-blue-100 dark:bg-blue-950/40 text-blue-600 dark:text-blue-400 flex items-center justify-center font-bold mb-3">
              <Terminal className="w-4 h-4" />
            </div>
            <h3 className="font-semibold text-slate-900 dark:text-white text-sm">Cross-Platform POSIX</h3>
            <p className="mt-1 text-xs text-slate-600 dark:text-slate-400 leading-relaxed">
              The built-in <code>fogu:shell</code> implements pipelines, redirects, and core utilities (<code>grep</code>, <code>sed</code>, <code>cat</code>, <code>wc</code>) with identical semantics on Linux, macOS, and Windows.
            </p>
          </div>

          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="w-8 h-8 rounded-lg bg-emerald-100 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 flex items-center justify-center font-bold mb-3">
              <Shield className="w-4 h-4" />
            </div>
            <h3 className="font-semibold text-slate-900 dark:text-white text-sm">Zero-Trust Fleet SSH</h3>
            <p className="mt-1 text-xs text-slate-600 dark:text-slate-400 leading-relaxed">
              Native Ed25519 cryptographic key authentication with <code>fogu:ssh</code>, parallel multi-server streaming execution, and encrypted SFTP file transport with SHA256 integrity verification.
            </p>
          </div>
        </div>

        {/* Interactive Tabbed Automation API */}
        <div className="mt-8">
          <div className="flex flex-wrap gap-2 border-b border-slate-200 dark:border-slate-800 pb-2">
            {[
              { id: 'playbook', label: 'fogu:playbook (Declarative IaC)' },
              { id: 'ssh', label: 'fogu:ssh (Remote Fleet Execution)' },
              { id: 'shell', label: 'fogu:shell (POSIX Streaming Pipelines)' },
              { id: 'cron', label: 'fogu:cron (Sub-Millisecond Scheduler)' },
              { id: 'events', label: 'fogu:events (Reactive Automation)' }
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveAutoTab(tab.id as any)}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
                  activeAutoTab === tab.id
                    ? 'bg-orange-500 text-white shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <div className="mt-4">
            {activeAutoTab === 'playbook' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
                  <span>Declarative Infrastructure-as-Code Playbooks</span>
                  <span className="text-xs px-2 py-0.5 rounded bg-orange-100 dark:bg-orange-950 text-orange-600 dark:text-orange-400">Idempotent</span>
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Define desired state for servers, packages, templates, and daemon services. Fôgu checks the live environment before mutating anything and returns structured diff reports.
                </p>
                <DocCodeBlock
                  title="deploy_production_fleet.fg"
                  onRun={onRun}
                  code={`import playbook from "fogu:playbook"
import log from "fogu:log"

// Execute declarative configuration with rollback protection
const report = playbook.run({
    name: "Provision Production Microservices Cluster",
    hosts: ["app-01.infra.internal", "app-02.infra.internal"],
    serial: 1, // Rolling update: one host at a time
    vars: {
        appVersion: "v2.8.4",
        httpPort: 8080,
        replicas: 4
    },
    tasks: [
        {
            name: "Ensure application user and group exist",
            user: { name: "fogu-svc", system: true, shell: "/bin/false" }
        },
        {
            name: "Synchronize configuration template",
            template: {
                src: "./templates/service.conf.tmpl",
                dest: "/etc/fogu/service.conf",
                owner: "fogu-svc",
                mode: "0640"
            }
        },
        {
            name: "Deploy binary artifact with SHA256 checksum",
            get_url: {
                url: "https://artifacts.corp.internal/bin/gateway-{{appVersion}}-linux-amd64",
                dest: "/usr/local/bin/gateway",
                checksum: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                mode: "0755"
            }
        },
        {
            name: "Restart and verify systemd service status",
            systemd: {
                name: "gateway.service",
                state: "restarted",
                enabled: true,
                healthcheck: "http://127.0.0.1:8080/healthz",
                timeout: 15s
            }
        }
    ]
})?

log.info("Playbook finished successfully", {
    hostsChanged: report.changedHosts,
    tasksExecuted: report.tasksCount,
    duration: report.duration
})`}
                />
              </div>
            )}

            {activeAutoTab === 'ssh' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Multi-Host Parallel SSH Fleet Orchestration
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Orchestrate 10,000+ edge servers or cloud nodes simultaneously using green-thread concurrency with encrypted Ed25519 keys, channel multiplexing, and live stream ingestion.
                </p>
                <DocCodeBlock
                  title="rolling_canary_upgrade.fg"
                  onRun={onRun}
                  code={`import ssh from "fogu:ssh"
import sync from "fogu:sync"
import log from "fogu:log"

const clusterNodes = [
    "edge-us-east-1.internal",
    "edge-eu-west-1.internal",
    "edge-ap-southeast-1.internal"
]

// Parallel fleet execution using green threads
const wg = sync.waitGroup()

for node in clusterNodes {
    wg.add(1)
    fg fn() {
        defer wg.done()
        const session = ssh.connect({
            host: node,
            port: 22,
            user: "deployer",
            keyFile: "~/.ssh/id_ed25519",
            timeout: 10s
        })?

        log.info("Deploying canary patch to host", { host: node })
        
        // Stream stdout and capture exit code
        const res = session.exec("sudo /opt/fogu/scripts/patch-worker.sh --graceful", {
            env: { RELEASE_TAG: "hotfix-904" }
        })?

        if res.exitCode != 0 {
            log.error("Canary deployment failed on node!", { host: node, stderr: res.stderr })
            session.exec("sudo /opt/fogu/scripts/rollback.sh")
            return
        }

        log.info("Host successfully upgraded", { host: node, stdout: res.stdout.trim() })
        session.close()
    }()
}

wg.wait()
log.info("Fleet upgrade finished without errors")`}
                />
              </div>
            )}

            {activeAutoTab === 'shell' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Cross-Platform Streaming POSIX Pipelines
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Chain processes with streaming stdin/stdout backpressure. Works identically across Linux, Windows, and macOS without spawning system shell binaries.
                </p>
                <DocCodeBlock
                  title="security_log_audit.fg"
                  onRun={onRun}
                  code={`import shell from "fogu:shell"
import fs from "fogu:fs"
import log from "fogu:log"

// Stream logs through pipeline: cat -> grep -> awk -> sort -> uniq -> count
const pipeline = shell.pipeline([
    "cat /var/log/audit/access.log",
    "grep 'HTTP/1.1 401'",
    "awk '{print $1}'", // Extract attacker IP addresses
    "sort",
    "uniq -c",
    "sort -rn"
])?

const suspiciousIps = []
while true {
    const line = pipeline.readLine()?
    if line == nil { break }
    
    const parts = line.trim().split(" ")
    const attempts = int(parts[0])
    const ip = parts[1]

    if attempts > 50 {
        suspiciousIps.push({ ip: ip, attempts: attempts })
        log.warn("Brute force detected!", { ip: ip, failedAttempts: attempts })
        // Trigger automated firewall ban rule
        shell.exec(\`iptables -A INPUT -s \${ip} -j DROP\`)
    }
}

log.info("Audit complete", { totalBanned: suspiciousIps.length })`}
                />
              </div>
            )}

            {activeAutoTab === 'cron' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Distributed Cron Scheduler with Leader Election
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  High-precision scheduler with sub-millisecond drift control, timezone anchoring, and distributed Redis Redlock to guarantee that cluster jobs run on exactly one replica.
                </p>
                <DocCodeBlock
                  title="scheduled_tiering.fg"
                  onRun={onRun}
                  code={`import cron from "fogu:cron"
import redis from "fogu:redis"
import s3 from "fogu:s3"
import log from "fogu:log"

const rdb = redis.connect(Fogu.env.REDIS_URL)?
const s3Client = s3.connect({ bucket: "prod-analytics-cold", region: "us-east-1" })?

// Run daily at 02:00 UTC with distributed lock protection
cron.schedule("0 2 * * *", { timezone: "UTC" }, fn() {
    // Acquire distributed lock for 10 minutes to prevent duplicate runs
    const lock = rdb.lock("cron:cold-tiering", { ttl: 10m })?
    if lock == nil {
        log.info("Another cluster node is already executing the tiering job. Skipping.")
        return
    }

    log.info("Starting automated cold storage tiering pipeline...")
    // Perform data tiering...
    lock.release()
    log.info("Tiering completed successfully.")
})

log.info("Cron scheduler active. Listening for triggers...")
cron.start()`}
                />
              </div>
            )}

            {activeAutoTab === 'events' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Reactive Event Loops & File System Watchers
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Listen for kernel inotify/FSEvents triggers, OS termination signals (SIGTERM/SIGHUP), and internal event emitter broadcasts with automatic worker pool dispatch.
                </p>
                <DocCodeBlock
                  title="reactive_hot_reload.fg"
                  onRun={onRun}
                  code={`import events from "fogu:events"
import fs from "fogu:fs"
import system from "fogu:system"
import log from "fogu:log"

const bus = events.emitter()

// Watch directory for hot configuration updates
fs.watch("./config", { recursive: true, debounce: 250ms }, fn(event) {
    if event.kind == "modify" && event.path.endsWith(".yaml") {
        bus.emit("config:changed", { file: event.path, at: event.timestamp })
    }
})

bus.on("config:changed", fn(data) {
    log.info("Configuration change detected! Reloading live memory buffers...", data)
    // Reload caches atomically without dropping active user connections
})

// Graceful shutdown on SIGINT or SIGTERM
system.onSignal(["SIGINT", "SIGTERM"], fn(sig) {
    log.warn(\`Received \${sig}. Draining worker queue before exit...\`)
    system.exit(0)
})`}
                />
              </div>
            )}
          </div>
        </div>
      </section>

      {/* ================= 3. AI & AUTONOMOUS AGENTS ================= */}
      <section id="ai-agents" data-group="Enterprise & Automation" data-title="AI & Autonomous Agents" className="scroll-mt-20 mt-20">
        <div className="pb-4 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-orange-600 dark:text-orange-400">
            <Bot className="w-4 h-4" />
            <span>LLMs, Function Calling & Multi-Agent Loops</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            AI & Autonomous Agents (<code>fogu:ai</code>)
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-3xl">
            Fôgu includes a universal multi-model LLM engine with native support for Google Gemini, Anthropic Claude,
            OpenAI, DeepSeek, Groq, and local Ollama/vLLM endpoints. Features include zero-allocation streaming,
            schema-validated function calling, and autonomous self-correcting agent loops.
          </p>
        </div>

        {/* AI Tabs */}
        <div className="mt-6">
          <div className="flex flex-wrap gap-2 border-b border-slate-200 dark:border-slate-800 pb-2">
            {[
              { id: 'agent', label: 'Autonomous SRE Remediation Agent' },
              { id: 'streaming', label: 'Zero-Allocation SSE Streaming' },
              { id: 'tools', label: 'Function Calling & Schema Validation' },
              { id: 'rag', label: 'Embeddings & In-Memory Vector Search' }
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveAiTab(tab.id as any)}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
                  activeAiTab === tab.id
                    ? 'bg-orange-500 text-white shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <div className="mt-4">
            {activeAiTab === 'agent' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Autonomous SRE Incident Remediation Agent
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  An autonomous agent inspects server telemetry, discovers deadlocks, verifies database connection health via tools, and executes recovery actions with human-readable logging.
                </p>
                <DocCodeBlock
                  title="autonomous_sre_bot.fg"
                  onRun={onRun}
                  code={`import ai from "fogu:ai"
import ssh from "fogu:ssh"
import sql from "fogu:sql"
import mail from "fogu:mail"
import log from "fogu:log"

const llm = ai.client({ provider: "gemini", model: "gemini-2.5-flash" })?

// 1. Declare tool definitions with JSON schema parameters
const getMetricsTool = ai.tool(
    "get_server_metrics",
    "Returns CPU, memory and swap usage for a specific server host",
    {
        type: "object",
        properties: { host: { type: "string" } },
        required: ["host"]
    },
    fn(args) {
        log.info("Agent invoked get_server_metrics tool", args)
        return { host: args.host, cpuUsage: 94.2, memoryUsedMb: 15820, memoryTotalMb: 16384, status: "CRITICAL" }
    }
)

const restartServiceTool = ai.tool(
    "restart_service",
    "Restarts a malfunctioning systemd service via SSH",
    {
        type: "object",
        properties: {
            host: { type: "string" },
            service: { type: "string" }
        },
        required: ["host", "service"]
    },
    fn(args) {
        log.warn("Agent executing remediation command: restarting service", args)
        return { host: args.host, service: args.service, restarted: true, newStatus: "active (running)" }
    }
)

// 2. Instantiate autonomous multi-step agent
const agent = ai.agent({
    client: llm,
    instructions: "You are an autonomous SRE triage engineer. Inspect server telemetry, identify the failing component, execute remediation tools, and summarize root cause.",
    tools: [getMetricsTool, restartServiceTool],
    maxSteps: 8,
    memory: true
})

// 3. Trigger remediation on incoming PagerDuty webhook
const incident = agent.run("Server prod-worker-04 is triggering high memory saturation alerts. Triage and remediate immediately.")?

log.info("Agent Incident Resolution Report:", {
    summary: incident.text,
    stepsTaken: incident.steps.length,
    tokensUsed: incident.usage.totalTokens
})`}
                />
              </div>
            )}

            {activeAiTab === 'streaming' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Streaming LLM Tokens Directly to HTTP / WebSockets
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Zero-allocation token streaming over HTTP Server-Sent Events (SSE). Client responses start streaming in under 40 milliseconds.
                </p>
                <DocCodeBlock
                  title="streaming_ai_gateway.fg"
                  onRun={onRun}
                  code={`import web from "fogu:web"
import ai from "fogu:ai"

const app = web.app()
const llm = ai.client({ provider: "anthropic", model: "claude-3-5-sonnet" })?

app.post("/api/chat/stream", fn(ctx) {
    const req = ctx.req.json()?
    
    // Set SSE headers
    ctx.res.setHeader("Content-Type", "text/event-stream")
    ctx.res.setHeader("Cache-Control", "no-cache")
    ctx.res.setHeader("Connection", "keep-alive")

    const stream = llm.stream({
        prompt: req.prompt,
        temperature: 0.7,
        maxTokens: 2048
    })?

    while true {
        const token = stream.next()?
        if token == nil { break } // End of stream
        ctx.res.write(\`data: \${token}\\n\\n\`)
    }

    ctx.res.write("data: [DONE]\\n\\n")
    ctx.res.end()
})

app.listen(8080)`}
                />
              </div>
            )}

            {activeAiTab === 'tools' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Structured JSON Output & Type-Safe Schema Validation
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Enforce strict JSON schema conformance from LLM responses for deterministic data extraction and ETL pipelines.
                </p>
                <DocCodeBlock
                  title="structured_extraction.fg"
                  onRun={onRun}
                  code={`import ai from "fogu:ai"
import log from "fogu:log"

const llm = ai.client({ provider: "openai", model: "gpt-4o-mini" })?

// Extract typed invoice data from unstructured vendor emails
const emailRaw = "Hi finance, please remit $4,850.00 to Apex Cloud Corp by Nov 15, 2026 for Invoice #INV-9821."

const result = llm.chat({
    prompt: \`Extract structured invoice details from this text:\\n\${emailRaw}\`,
    json: true, // Forces structured JSON mode
    schema: {
        type: "object",
        properties: {
            vendor: { type: "string" },
            invoiceNumber: { type: "string" },
            totalAmount: { type: "number" },
            currency: { type: "string" },
            dueDate: { type: "string" }
        },
        required: ["vendor", "invoiceNumber", "totalAmount", "dueDate"]
    }
})?

log.info("Parsed structured invoice data:", result.data)`}
                />
              </div>
            )}

            {activeAiTab === 'rag' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  In-Memory Vector Search & Retrieval-Augmented Generation (RAG)
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Compute dense embeddings, calculate cosine similarities, and inject relevant context into generation prompts.
                </p>
                <DocCodeBlock
                  title="rag_pipeline.fg"
                  onRun={onRun}
                  code={`import ai from "fogu:ai"
import math from "fogu:math"

const client = ai.client({ provider: "openai" })?

// 1. Generate text embeddings
const docs = [
    "Database failover policy: initiate switch to replica in eu-west-1 if primary fails 3 consecutive healthchecks.",
    "User authentication policy: passwords must be at least 16 characters and rotated every 90 days.",
    "Incident escalation: contact the on-call engineer at +1-555-0199 for P1 system outages."
]

const vectors = client.embed(docs, { model: "text-embedding-3-small" })?

// 2. Query against documents
const userQuestion = "What happens if the main database goes down?"
const queryVector = client.embed(userQuestion)?

// Find most similar document via cosine similarity
let bestIdx = 0
let highestScore = -1.0

for i, docVec in vectors {
    const score = math.cosineSimilarity(queryVector, docVec)
    if score > highestScore {
        highestScore = score
        bestIdx = i
    }
}

// 3. Augment prompt with retrieved context
const answer = client.chat({
    system: "Answer using only the provided enterprise policy context.",
    prompt: \`Context: \${docs[bestIdx]}\\n\\nQuestion: \${userQuestion}\`
})?

print(answer.text)`}
                />
              </div>
            )}
          </div>
        </div>
      </section>

      {/* ================= 4. DISTRIBUTED SCALABILITY & MESSAGING ================= */}
      <section id="scalability-distributed" data-group="Enterprise & Automation" data-title="Distributed Scalability" className="scroll-mt-20 mt-20">
        <div className="pb-4 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-orange-600 dark:text-orange-400">
            <Network className="w-4 h-4" />
            <span>Distributed Messaging, Caching & Resilience</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            High-Throughput Enterprise Scalability
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-3xl">
            Fôgu provides native Rust-backed primitives to construct architectures capable of processing millions
            of events per second with sub-millisecond latencies, distributed fault-tolerance, and minimal compute footprint.
          </p>
        </div>

        {/* Real-time Benchmark Metrics Strip */}
        <div className="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="text-xs font-mono text-slate-500 uppercase tracking-wider">NATS JetStream Throughput</div>
            <div className="mt-1 text-2xl font-bold font-mono text-slate-900 dark:text-white tabular-nums">
              1.2M <span className="text-xs font-normal text-slate-500">msgs/sec</span>
            </div>
            <p className="mt-1 text-xs text-slate-500">Durable disk writes & active deduplication</p>
          </div>

          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="text-xs font-mono text-slate-500 uppercase tracking-wider">ScyllaDB P99 Write Latency</div>
            <div className="mt-1 text-2xl font-bold font-mono text-slate-900 dark:text-white tabular-nums">
              0.85 <span className="text-xs font-normal text-slate-500">ms</span>
            </div>
            <p className="mt-1 text-xs text-slate-500">Columnar sharded storage with prepared stmts</p>
          </div>

          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="text-xs font-mono text-slate-500 uppercase tracking-wider">Green Task Memory Cost</div>
            <div className="mt-1 text-2xl font-bold font-mono text-slate-900 dark:text-white tabular-nums">
              ~4 <span className="text-xs font-normal text-slate-500">KB</span>
            </div>
            <p className="mt-1 text-xs text-slate-500">Millions of fg tasks without OS thread overhead</p>
          </div>

          <div className="rounded-xl border border-slate-200 dark:border-slate-800 bg-slate-50/50 dark:bg-[#0e1118] p-4 text-left">
            <div className="text-xs font-mono text-slate-500 uppercase tracking-wider">Cranelift JIT Speedup</div>
            <div className="mt-1 text-2xl font-bold font-mono text-emerald-600 dark:text-emerald-400 tabular-nums">
              120x <span className="text-xs font-normal text-slate-500">faster</span>
            </div>
            <p className="mt-1 text-xs text-slate-500">Runtime compilation to native x86_64/ARM64</p>
          </div>
        </div>

        {/* Scalability Tabs */}
        <div className="mt-8">
          <div className="flex flex-wrap gap-2 border-b border-slate-200 dark:border-slate-800 pb-2">
            {[
              { id: 'nats', label: 'NATS JetStream (Persistent Streams)' },
              { id: 'rabbitmq', label: 'RabbitMQ (Topic Exchanges & DLQ)' },
              { id: 'redis', label: 'Redis Cluster & Redlock' },
              { id: 'scylla', label: 'ScyllaDB / Cassandra (Sub-ms P99)' },
              { id: 'circuit', label: 'Circuit Breaker & Exponential Backoff' }
            ].map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveScalabilityTab(tab.id as any)}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
                  activeScalabilityTab === tab.id
                    ? 'bg-orange-500 text-white shadow-sm'
                    : 'text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <div className="mt-4">
            {activeScalabilityTab === 'nats' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  NATS JetStream Stream Processing & Consumer Groups
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  High-performance at-least-once distributed pub/sub with persistent stream logs, consumer group load balancing, and message deduplication.
                </p>
                <DocCodeBlock
                  title="nats_telemetry_pipeline.fg"
                  onRun={onRun}
                  code={`import nats from "fogu:nats"
import log from "fogu:log"

// Connect to distributed NATS cluster
const nc = nats.connect({ servers: ["nats://10.0.0.1:4222", "nats://10.0.0.2:4222"] })?
const js = nc.jetstream()?

// Ensure stream exists with disk retention
js.addStream({
    name: "ORDERS",
    subjects: ["orders.*"],
    retention: "limits",
    maxAge: 72h,
    storage: "file"
})?

// Bind durable consumer group for load-balanced processing
const sub = js.subscribe("orders.created", {
    durable: "payment-worker-group",
    ackPolicy: "explicit",
    maxDeliver: 5
})?

log.info("Worker listening for orders...")
while true {
    const msg = sub.next(timeout: 30s)?
    if msg == nil { continue }

    const order = msg.json()?
    log.info("Processing order payload", { orderId: order.id, amount: order.amount })
    
    // Explicit acknowledgement
    msg.ack()
}`}
                />
              </div>
            )}

            {activeScalabilityTab === 'rabbitmq' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  RabbitMQ Topic Routing, Dead-Letter Queues (DLQ) & Publisher Confirms
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Enterprise AMQP 0-9-1 messaging with topic exchanges, automated dead-letter routing for poison pills, and durable queue mirroring.
                </p>
                <DocCodeBlock
                  title="rabbitmq_resilient_worker.fg"
                  onRun={onRun}
                  code={`import rabbitmq from "fogu:rabbitmq"
import log from "fogu:log"

const conn = rabbitmq.connect("amqp://admin:secret@broker-cluster:5672")?
const ch = conn.channel()?

// Declare main topic exchange and dead-letter exchange
ch.exchangeDeclare("events.topic", "topic", { durable: true })
ch.exchangeDeclare("events.dlx", "direct", { durable: true })

// Bind processing queue with automatic DLQ redirect on failure
ch.queueDeclare("invoice-processing", {
    durable: true,
    arguments: {
        "x-dead-letter-exchange": "events.dlx",
        "x-dead-letter-routing-key": "invoice.poison"
    }
})
ch.queueBind("invoice-processing", "events.topic", "billing.invoice.*")

ch.consume("invoice-processing", fn(delivery) {
    const [success, err] = processInvoice(delivery.body)
    if !success {
        log.error("Rejecting failed message to DLQ", { err: err })
        delivery.nack({ requeue: false }) // Moves message to Dead Letter Queue
    } else {
        delivery.ack()
    }
})`}
                />
              </div>
            )}

            {activeScalabilityTab === 'redis' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Redis Clustered Caching, Pipelining & Redlock Distributed Locks
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Multi-node Redis Cluster with command pipelining for 500k+ ops/sec and Redlock mutual exclusion across distributed services.
                </p>
                <DocCodeBlock
                  title="redis_cluster_pipeline.fg"
                  onRun={onRun}
                  code={`import redis from "fogu:redis"
import log from "fogu:log"

const cluster = redis.cluster({
    nodes: ["10.0.1.1:6379", "10.0.1.2:6379", "10.0.1.3:6379"]
})?

// 1. High-throughput command pipelining
const pipe = cluster.pipeline()
for i in 0..1000 {
    pipe.set(\`user:session:\${i}\`, { active: true, lastSeen: time.now() }, 15m)
}
const results = pipe.exec()?
log.info("Batch saved 1,000 sessions in a single network round-trip", { count: results.length })

// 2. Distributed mutual exclusion lock (Redlock)
const lock = cluster.redlock("resource:billing-export", { ttl: 30s, retryCount: 3 })?
if lock != nil {
    defer lock.unlock()
    log.info("Acquired exclusive lock. Running critical section...")
    // Only 1 node in the entire cluster runs this block simultaneously
}`}
                />
              </div>
            )}

            {activeScalabilityTab === 'scylla' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  ScyllaDB / Apache Cassandra High-Write Big Data
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Asynchronous token-aware CQL driver with prepared statements and connection pooling for sub-millisecond P99 writes.
                </p>
                <DocCodeBlock
                  title="scylla_telemetry_insert.fg"
                  onRun={onRun}
                  code={`import scylla from "fogu:scylla"
import crypto from "fogu:crypto"
import time from "fogu:time"

const session = scylla.connect({
    hosts: ["10.2.0.10:9042", "10.2.0.11:9042"],
    keyspace: "iot_telemetry",
    datacenter: "us-east-1"
})?

// Prepare statement once for optimal binary protocol serialization
const stmt = session.prepare(\`
    INSERT INTO device_readings (device_id, bucket_hour, recorded_at, temperature, voltage, status)
    VALUES (?, ?, ?, ?, ?, ?)
    USING TTL 2592000
\`)?

// Asynchronous execution without blocking the event loop
session.execute(stmt, [
    crypto.uuid(),
    time.format(time.now(), "2026-10-17-14"),
    time.now(),
    24.85,
    3.31,
    "OK"
])?`}
                />
              </div>
            )}

            {activeScalabilityTab === 'circuit' && (
              <div>
                <h3 className="text-lg font-bold text-slate-900 dark:text-white">
                  Resilient Circuit Breaker & Exponential Backoff with Jitter
                </h3>
                <p className="text-sm text-slate-600 dark:text-slate-400 mt-1">
                  Prevent cascading service outages by tripping circuits when downstream dependencies fail, automatically testing recovery via half-open probe states.
                </p>
                <DocCodeBlock
                  title="circuit_breaker.fg"
                  onRun={onRun}
                  code={`import net from "fogu:net"
import http from "fogu:http"
import log from "fogu:log"

// Configure circuit breaker for flaky external banking API
const breaker = net.circuitBreaker({
    name: "payment-gateway",
    failureThreshold: 5,       // Trip open after 5 consecutive errors
    resetTimeout: 10s,         // Wait 10s before testing with half-open probe
    halfOpenMaxSuccess: 2      // Require 2 successful requests to close circuit
})

fn callPaymentProvider(payment) {
    return breaker.execute(fn() {
        const res = http.post("https://api.partnerbank.com/v1/charge", {
            json: payment,
            timeout: 2500ms
        })?

        if res.status >= 500 {
            error(\`Bank returned server error HTTP \${res.status}\`)
        }
        return res.json()
    })
}

// Calls automatically reject with "circuit_breaker_open" error when tripped,
// avoiding blocking threads on timing out upstream services.`}
                />
              </div>
            )}
          </div>
        </div>
      </section>

      {/* ================= 5. INTERACTIVE SCENARIOS LAB ================= */}
      <section id="enterprise-playground" data-group="Enterprise & Automation" data-title="Enterprise Scenarios Lab" className="scroll-mt-20 mt-20">
        <div className="pb-4 border-b border-slate-200 dark:border-slate-800">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-orange-600 dark:text-orange-400">
            <Zap className="w-4 h-4" />
            <span>Interactive Production Sandbox</span>
          </div>
          <h2 className="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">
            Enterprise Scenarios Interactive Lab
          </h2>
          <p className="mt-2 text-sm text-slate-600 dark:text-slate-400 max-w-3xl">
            Test and run production-grade enterprise Fôgu scripts directly in your browser. Inspect real-time execution
            logs, telemetry metrics (throughput, memory usage, green tasks), and download ready-to-deploy <code>.fg</code> files.
          </p>
        </div>

        <div className="mt-6">
          <InteractivePlayground />
        </div>
      </section>
    </>
  );
};
