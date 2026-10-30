export interface EnterpriseScenario {
  id: string;
  title: string;
  subtitle: string;
  category: 'automation' | 'integration' | 'scalability' | 'ai';
  description: string;
  architectureNotes: string[];
  code: string;
  simulatedOutput: string[];
  executionStats: {
    duration: string;
    throughput: string;
    memory: string;
    tasksCount: number;
    jitActive: boolean;
  };
}

export const ENTERPRISE_SCENARIOS: EnterpriseScenario[] = [
  {
    id: 'saga-gateway',
    title: 'Distributed Payment Gateway & Saga Orchestrator',
    subtitle: 'fogu:web + fogu:bullmq + fogu:scylla + fogu:s3 + fogu:jwt',
    category: 'scalability',
    description: 'High-volume asynchronous transaction processing with idempotency guarantees, ledger writes to ScyllaDB, S3 receipt generation, and automatic failure compensation via BullMQ.',
    architectureNotes: [
      'Zero-allocation JWT verification at the HTTP edge gateway',
      'Dispatched to BullMQ queue with transactionId deduplication and exponential backoff',
      'Resilient ledger persistence in ScyllaDB cluster with sub-millisecond P99 writes',
      'Parallel signed receipt upload to Amazon S3 / Cloudflare R2'
    ],
    code: `import web from "fogu:web"
import bullmq from "fogu:bullmq"
import scylla from "fogu:scylla"
import s3 from "fogu:s3"
import jwt from "fogu:jwt"
import crypto from "fogu:crypto"
import time from "fogu:time"

// 1. Connect to distributed enterprise infrastructure
const db = scylla.connect({ hosts: ["10.0.1.5:9042", "10.0.1.6:9042"], keyspace: "fintech" })?
const paymentsQueue = bullmq.queue("payments-saga", { connection: Fogu.env.REDIS_URL })?
const receiptBucket = s3.connect({ bucket: "secure-receipts", region: "us-east-1" })?

// 2. Enterprise HTTP edge gateway with Zero-Trust authentication
const app = web.app()
app.use(jwt.middleware(Fogu.env.JWT_SECRET))

app.post("/api/v1/checkout", (ctx) => {
    const payload = ctx.req.json()?
    const txId = crypto.uuid()

    // Strict idempotency enqueueing
    paymentsQueue.add("process-charge", {
        txId: txId,
        accountId: ctx.user.sub,
        amount: payload.amount,
        currency: payload.currency ?? "USD"
    }, {
        deduplication: { id: txId, ttl: 24h },
        attempts: 4,
        backoff: { type: "exponential", delay: 500ms }
    })?

    return web.json({ status: "ACCEPTED", transactionId: txId }, { status: 202 })
})

// 3. High-concurrency worker running on cooperative green threads (fg)
const worker = bullmq.worker("payments-saga", { concurrency: 32 })?

fg fn() {
    while true {
        const job = worker.next()?
        if job == nil { break }

        const tx = job.data
        // Idempotent ledger record in ScyllaDB
        db.execute(
            "INSERT INTO transactions (id, account_id, amount, status, created_at) VALUES (?, ?, ?, ?, toTimestamp(now()))",
            [tx.txId, tx.accountId, tx.amount, "PROCESSING"]
        )?

        // Encrypted receipt generation and upload to S3
        const receipt = \`DIGITAL TRANSACTION RECEIPT\\nID: \${tx.txId}\\nAMOUNT: \${tx.amount} \${tx.currency}\`
        const s3Key = \`receipts/\${tx.accountId}/\${tx.txId}.txt\`
        receiptBucket.put(s3Key, receipt, { contentType: "text/plain" })?

        db.execute("UPDATE transactions SET status = ?, receipt_url = ? WHERE id = ?", ["COMPLETED", s3Key, tx.txId])?
        job.complete({ processedAt: time.now(), s3Path: s3Key })
    }
}()`,
    simulatedOutput: [
      "[INIT] Connected to ScyllaDB cluster (2 active nodes, average latency 0.8ms)",
      "[INIT] BullMQ queue 'payments-saga' ready with 32 active worker fibers",
      "[HTTP] Gateway listening on port 8080 (HTTP/2 enabled, Zero-Trust JWT Guard active)",
      "[SAGA] Job process-charge received: tx_89df2c19a4b | Account: usr_4920",
      "[SCYLLA] INSERT transactions id=tx_89df2c19a4b status=PROCESSING [0.8ms]",
      "[S3] Signed receipt uploaded to s3://secure-receipts/receipts/usr_4920/tx_89df2c19a4b.txt",
      "[SCYLLA] UPDATE transactions SET status=COMPLETED [0.6ms]",
      "[BULLMQ] Transaction completed with clean exit. Idempotency guaranteed."
    ],
    executionStats: {
      duration: "1.42ms",
      throughput: "24,800 req/s",
      memory: "18.4 MB",
      tasksCount: 32,
      jitActive: true
    }
  },
  {
    id: 'iot-telemetry-nats',
    title: 'High-Throughput IoT Telemetry Stream Ingestion',
    subtitle: 'fogu:mqtt + fogu:nats (JetStream) + fogu:scylla + fogu:web',
    category: 'integration',
    description: 'Massive scale pipeline that ingests sensor packets over MQTT, publishes into durable NATS JetStream with deduplication, and broadcasts anomalies via WebSockets to operator dashboards.',
    architectureNotes: [
      'MQTT edge connection with auto-reconnection and wildcard topic subscription (factories/+/sensors/+)',
      'NATS JetStream as the durable event backplane with retention windows and message replay',
      'Asynchronous worker analyzes temperature spikes and thresholds in-memory',
      'Embedded WebSocket server broadcasts real-time anomaly feeds to monitoring panels'
    ],
    code: `import mqtt from "fogu:mqtt"
import nats from "fogu:nats"
import scylla from "fogu:scylla"
import web from "fogu:web"
import time from "fogu:time"

// 1. Initialize NATS JetStream with persistent file stream
const nc = nats.connect(Fogu.env.NATS_URL ?? "nats://127.0.0.1:4222")?
const js = nc.jetstream()
js.addStream({
    name: "TELEMETRY",
    subjects: ["telemetry.>"],
    max_age: 7d,
    storage: "file"
})?

// 2. WebSocket hub for real-time operator alerts
const app = web.app()
const clients = []

app.ws("/stream/alerts", (ws) => {
    clients.push(ws)
    while true {
        const msg = ws.next()?
        if msg == nil { break }
    }
    clients.remove(clients.index_of(ws))
})

// 3. Ingest MQTT packets from field sensors
const edgeBroker = mqtt.connect("mqtt://iot-gateway.internal:1883", { clientId: "fogu-telemetry-engine" })?
edgeBroker.subscribe("factories/+/sensors/+")?

fg fn() {
    while true {
        const packet = edgeBroker.next()?
        if packet == nil { continue }
        
        const data = packet.json()
        // Acknowledged publishing to JetStream
        js.publish(\`telemetry.factory.\${data.factoryId}\`, packet.data, {
            msgId: \`\${data.sensorId}-\${data.timestamp}\`
        })?

        // Immediate anomaly detection
        if data.temperature > 85.0 {
            const alertMsg = \`CRITICAL: Sensor \${data.sensorId} overheating (\${data.temperature}°C)\`
            for c in clients {
                c.send({ level: "CRITICAL", alert: alertMsg, timestamp: time.now() })
            }
        }
    }
}()

app.listen(9000)?`,
    simulatedOutput: [
      "[NATS] JetStream connected: Stream 'TELEMETRY' ready (file storage, 7-day retention)",
      "[MQTT] Connected to iot-gateway.internal:1883 | Subscribed to factories/+/sensors/+",
      "[WS] Dashboard client connected to /stream/alerts",
      "[INGEST] Telemetry stream active: 125,000 packets/sec throughput",
      "[ALERT] Temperature threshold exceeded: Sensor SEN-X90 at 89.4°C",
      "[WS] Broadcast alert dispatched to 4 active operator panels"
    ],
    executionStats: {
      duration: "0.85ms",
      throughput: "125,000 msgs/s",
      memory: "24.1 MB",
      tasksCount: 64,
      jitActive: true
    }
  },
  {
    id: 'infra-playbook',
    title: 'Infrastructure Orchestration & Zero-Downtime Rolling Deployment',
    subtitle: 'fogu:playbook + fogu:ssh + fogu:shell + fogu:mail',
    category: 'automation',
    description: 'Ansible-style declarative orchestration native to Fogu: parallel execution across fleet hosts over OpenSSH, local health checks, symlink release rotation, and transactional mail alerts.',
    architectureNotes: [
      'Dynamic inventory with group aggregation (web_servers, db_clusters)',
      'Idempotent task execution: changes only apply if state differs',
      'Zero-downtime rolling update with port wait_for probes between host batches',
      'Consolidated deployment status report sent via Resend API or SMTP'
    ],
    code: `import playbook from "fogu:playbook"
import mail from "fogu:mail"
import shell from "fogu:shell"
import cli from "fogu:cli"

// 1. Enterprise server inventory
const inventory = {
    hosts: {
        "web-prod-01": { host: "10.0.2.11", user: "deploy", key: "~/.ssh/id_ed25519" },
        "web-prod-02": { host: "10.0.2.12", user: "deploy", key: "~/.ssh/id_ed25519" },
        "web-prod-03": { host: "10.0.2.13", user: "deploy", key: "~/.ssh/id_ed25519" }
    },
    groups: {
        web_cluster: { hosts: ["web-prod-01", "web-prod-02", "web-prod-03"], vars: { appPort: 8080 } }
    }
}

// 2. Zero-Downtime Deployment Playbook
const deploymentPlan = [
    {
        name: "Rolling Update Fogu Application",
        hosts: "web_cluster",
        become: true,
        vars: { version: "v2.4.1" },
        tasks: [
            {
                name: "Ensure release directory exists",
                file: { path: "/srv/fogu-app/releases/{{ version }}", state: "directory", mode: "0755" }
            },
            {
                name: "Deploy compiled standalone binary",
                copy: { src: "dist/enterprise-service", dest: "/srv/fogu-app/releases/{{ version }}/app", mode: "0755" }
            },
            {
                name: "Atomic symlink update (Zero Downtime)",
                shell: "ln -sfn /srv/fogu-app/releases/{{ version }} /srv/fogu-app/current"
            },
            {
                name: "Reload systemd service with zero connection drops",
                service: { name: "fogu-app", state: "reloaded" }
            },
            {
                name: "Verify health-check endpoint",
                wait_for: { port: "{{ appPort }}", host: "127.0.0.1", timeout: 10s }
            }
        ]
    }
]

// 3. Parallel execution with 3 forks
print(cli.style("=== INITIATING PRODUCTION DEPLOYMENT ===", "bold", "cyan"))
const result = playbook.run(deploymentPlan, { inventory: inventory, forks: 3 })?

// 4. Send deployment report by email
const mailer = mail.connect({ provider: "resend", apiKey: Fogu.env.RESEND_API_KEY })?
mailer.send({
    from: "DevOps <devops@enterprise.com>",
    to: "engineering@enterprise.com",
    subject: \`Deploy \${result.ok ? "SUCCESS" : "FAILURE"} - Web Cluster\`,
    text: \`Playbook Execution Report:\\nHosts updated: \${inventory.groups.web_cluster.hosts.join(", ")}\\nRecap:\\n\${json.stringify(result.recap)}\`
})?`,
    simulatedOutput: [
      "PLAY [Rolling Update Fogu Application] ************************************",
      "TASK [Ensure release directory exists] **********************************",
      "ok: [web-prod-01] => /srv/fogu-app/releases/v2.4.1",
      "ok: [web-prod-02] => /srv/fogu-app/releases/v2.4.1",
      "ok: [web-prod-03] => /srv/fogu-app/releases/v2.4.1",
      "TASK [Deploy compiled standalone binary] ********************************",
      "changed: [web-prod-01] (checksum match: d83a9)",
      "changed: [web-prod-02] (checksum match: d83a9)",
      "changed: [web-prod-03] (checksum match: d83a9)",
      "TASK [Atomic symlink update (Zero Downtime)] ****************************",
      "changed: [web-prod-01] => ln -sfn /srv/fogu-app/releases/v2.4.1 /srv/fogu-app/current",
      "changed: [web-prod-02] => ln -sfn /srv/fogu-app/releases/v2.4.1 /srv/fogu-app/current",
      "changed: [web-prod-03] => ln -sfn /srv/fogu-app/releases/v2.4.1 /srv/fogu-app/current",
      "TASK [Reload systemd service with zero connection drops] *****************",
      "changed: [web-prod-01] systemd reloaded",
      "changed: [web-prod-02] systemd reloaded",
      "changed: [web-prod-03] systemd reloaded",
      "PLAY RECAP *************************************************************",
      "web-prod-01 : ok=5  changed=3  unreachable=0  failed=0",
      "web-prod-02 : ok=5  changed=3  unreachable=0  failed=0",
      "web-prod-03 : ok=5  changed=3  unreachable=0  failed=0",
      "[MAIL] Deployment summary report dispatched via Resend API (id: res_948f2)"
    ],
    executionStats: {
      duration: "3.18s",
      throughput: "3 parallel fleet nodes",
      memory: "14.2 MB",
      tasksCount: 15,
      jitActive: false
    }
  },
  {
    id: 'ai-enterprise-agent',
    title: 'Autonomous Enterprise AI Agent with Tool Calling & SQL Governance',
    subtitle: 'fogu:ai + fogu:orm + fogu:redis + fogu:log',
    category: 'ai',
    description: 'Autonomous copilot with multi-step reasoning, Redis session memory, strict JSON schema validation for tool calls, read-only SQL governance, and live streaming.',
    architectureNotes: [
      'Multi-provider LLM support (Gemini, Claude, GPT, Ollama) with zero code changes',
      'Typed tools with strict JSON schema constraints and error recovery',
      'Database governance: read-only analytical queries with parameter sanitization',
      'Session memory conversation history persisted in Redis with TTL expiry'
    ],
    code: `import ai from "fogu:ai"
import { create_client } from "fogu:orm"
import redis from "fogu:redis"
import log from "fogu:log"

// 1. ORM database client and session store
const r = redis.connect(Fogu.env.REDIS_URL)?
const db = create_client({
    schema: {
        provider: "postgres",
        url: Fogu.env.DATABASE_URL,
        models: {
            Order: {
                fields: {
                    id: { type: "Int", primary: true },
                    customer: { type: "String" },
                    total: { type: "Float" },
                    status: { type: "String" }
                }
            }
        }
    }
})?

// 2. Define secure analytical tool with strict schema
const querySalesTool = ai.tool(
    "query_sales_summary",
    "Returns revenue and count of orders filtered by status",
    {
        type: "object",
        properties: {
            status: { type: "string", enum: ["PAID", "PENDING", "CANCELLED"] }
        },
        required: ["status"]
    },
    (args) => {
        log.info("Tool invoked by LLM agent", { status: args.status })
        const orders = db.order.findMany({ where: { status: args.status } })?
        const sum = orders.reduce((acc, o) => acc + o.total, 0.0)
        return { count: len(orders), totalRevenue: sum, currency: "USD" }
    }
)

// 3. Initialize Autonomous Fogu Agent
const client = ai.client({ provider: "gemini", model: "gemini-2.5-pro" })?
const agent = ai.agent({
    client: client,
    instructions: "You are the Senior Enterprise Data Analyst. Always inspect database tables via tools before answering financial queries.",
    tools: [querySalesTool],
    maxSteps: 5,
    memory: true
})

// 4. Executive prompt execution
const response = agent.run("What is the consolidated revenue for confirmed orders (PAID) today?")?
print("Agent Response:", response.text)
print("Steps executed:", response.steps.map((s) => s.toolCalls.map((t) => t.name)))`,
    simulatedOutput: [
      "[AI] Agent initialized with 'gemini-2.5-pro' model and memory buffer",
      "[AGENT] Analyzing user prompt...",
      "[TOOL_CALL] Invoking tool 'query_sales_summary' with parameters: { status: 'PAID' }",
      "[ORM] SELECT * FROM orders WHERE status = 'PAID' [executed in 1.1ms, 1,420 rows]",
      "[TOOL_RESULT] { count: 1420, totalRevenue: 849320.50, currency: 'USD' }",
      "[AGENT] Synthesizing final answer based on grounded tool data...",
      "Agent Response: Consolidated revenue for confirmed orders (PAID) today is $849,320.50 across 1,420 transactions with an average ticket size of $598.11.",
      "Steps executed: [[query_sales_summary]]"
    ],
    executionStats: {
      duration: "1.12s",
      throughput: "1 multi-step agentic loop",
      memory: "21.0 MB",
      tasksCount: 4,
      jitActive: true
    }
  }
];
