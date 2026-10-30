"""Standard library reference: messaging, events and scheduling."""

M = "Messaging & events"

SECTIONS = [
    {
        "id": "mod-events",
        "group": M,
        "title": "fogu:events",
        "import": 'import events from "fogu:events"',
        "blocks": [
            {
                "api": [
                    ("events.EventEmitter()", "Emitter", ""),
                    ("e.on(event, fn) · e.once(event, fn)", "nil", "Listeners receive every emitted argument (declare them all)."),
                    ("e.off(event, fn?) · e.removeAll(event?)", "nil", ""),
                    ("e.emit(event, ...args)", "bool", "<code>true</code> when someone listened."),
                    ("e.listeners(event) · e.eventNames() · e.hasListeners(event)", "", ""),
                ],
                "example": r'''
import events from "fogu:events"
const bus = events.EventEmitter()
bus.on("order", (id, total) => print("order", id, "total", total))
bus.once("order", (id, total) => print("first order only:", id))
print(bus.emit("order", 1, 9.9))
print(bus.emit("order", 2, 15))
print(bus.emit("nobody-listens"), bus.eventNames())
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-cron",
        "group": M,
        "title": "fogu:cron",
        "import": 'import cron from "fogu:cron"',
        "intro": "Jobs run when the program calls <code>cron.tick()</code> (typically in its main loop).",
        "blocks": [
            {
                "api": [
                    ("cron.every(interval, fn)", "nil", "A job every <code>interval</code> (duration or ms)."),
                    ("cron.schedule(expression, fn)", "nil", "5-field cron expression (<code>\"*/5 * * * *\"</code>); an invalid one stops the program."),
                    ("cron.tick()", "int", "Runs the jobs that are due; returns how many ran."),
                    ("cron.list()", "list", "Job names (<code>every(10ms)</code>, the cron expression…)."),
                    ("cron.clear()", "nil", "Removes every job."),
                ],
                "example": r'''
import cron from "fogu:cron"
import time from "fogu:time"
let beats = 0
cron.every(10ms, () => { beats = beats + 1 })
cron.schedule("0 3 * * *", () => print("nightly backup"))
for i in 0..6 {
    cron.tick()
    time.delay(10ms)
}
print(beats >= 3, cron.list())
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-nats",
        "group": M,
        "title": "fogu:nats",
        "import": 'import nats from "fogu:nats"',
        "intro": "NATS core protocol plus JetStream (streams, acknowledged publishing, pull consumers).",
        "blocks": [
            {
                "api": [
                    ("nats.connect(url | { url, user, pass, token, name, timeout })", "Nats · error", "<code>nats://[user:pass@|token@]host:4222</code>, default <code>$NATS_URL</code>."),
                    ("nc.publish(subject, data?, { headers, reply })", "nil · error", "<code>data</code>: str, or any value as JSON."),
                    ("nc.subscribe(subject, { queue, max })", "Subscription", "Wildcards <code>*</code> and <code>&gt;</code>; queue groups split the messages."),
                    ("sub.next(timeout?) · sub.tryNext() · sub.unsubscribe()", "Message · nil", ""),
                    ("nc.request(subject, data?, { timeout, headers })", "Message · error", "No responders → error value."),
                    ("nc.flush() · nc.close() · nc.connected · nc.server", "", ""),
                    ("msg.subject · .data · .headers · .reply · .json() · .respond(data)", "", ""),
                ],
            },
            {
                "h3": "JetStream",
                "api": [
                    ("nc.jetstream({ prefix, timeout })", "JetStream", ""),
                    ("js.addStream({ name, subjects, storage, max_msgs, max_age, … })", "map · error", "NATS stream config (snake_case keys)."),
                    ("js.updateStream(cfg) · js.streamInfo(name) · js.deleteStream(name) · js.purgeStream(name)", "", ""),
                    ("js.publish(subject, data, { msgId, headers, timeout })", "{ stream, seq, duplicate }", "<code>msgId</code> deduplicates."),
                    ("js.consumer(stream, { durable, filter, ackWait, deliver, maxDeliver })", "Consumer · error", "A durable pull consumer."),
                    ("consumer.fetch(batch = 1, expires = 5s)", "list", "Up to <code>batch</code> messages."),
                    ("msg.ack() · msg.nak(delay?) · msg.term() · msg.inProgress()", "nil", "<code>msg.seq</code>, <code>msg.delivered</code>, <code>msg.stream</code>."),
                ],
            },
            {
                "example": r'''
import nats from "fogu:nats"
const nc = nats.connect("nats://127.0.0.1:4222")?
const sub = nc.subscribe("orders.*")
nc.publish("orders.created", { id: 1 })
print(sub.next(1s)?.json())

const svc = nc.subscribe("math.double")
fg fn() { const req = svc.next()?; req.respond(str(int(req.data) * 2)) }()
print(nc.request("math.double", "21")?.data)

const js = nc.jetstream()
js.addStream({ name: "TASKS", subjects: ["tasks.>"] })?
print(js.publish("tasks.email", { to: "ana@x.com" }, { msgId: "t1" })?)
const c = js.consumer("TASKS", { durable: "workers" })?
for msg in c.fetch(10, 1s)? { print(msg.subject, msg.seq); msg.ack() }
''',
                "output": "{id: 1}\n42\n{duplicate: false, seq: 1, stream: TASKS}\ntasks.email 1",
                "output_title": "output (with a NATS server, -js)",
            },
            {
                "h3": "fogu() — the high-level layer",
                "text": "The same API as the usual <code>nats.js</code> wrapper (KV stores, <code>sub</code>/<code>pub</code> with handlers, "
                "services with auth, commands); it interoperates with it — same KV buckets, same <code>service:&lt;name&gt;</code> subjects and "
                "error payloads. Data: nil is sent empty, a string as is, the rest as JSON; received data is parsed as JSON when it can be. "
                "Handlers run as <code>fg</code> tasks.",
                "api": [
                    ("nats.fogu(url | config) · nc.fogu()", "NatsFogu · error", "Connects (or wraps an open connection)."),
                    ("f.store.createKV(name, fn?(entry), { history = 5, ttl })", "NatsKv · error", "A JetStream KV bucket (<code>KV_name</code>). With <code>fn</code>: a watch — the last value of each key, then every change: <code>{ bucket, key, value, operation: PUT|DEL|PURGE, revision }</code>; returning <code>false</code> stops it."),
                    ("f.store.kv(name) → create(key, data) · set · get · del · keys", "", "<code>create</code> → false when the key exists. Using a store before <code>createKV</code> is an error value."),
                    ("f.sub({ topic }, fn(data), { queue }) · f.pub({ topic }, data)", "nil", ""),
                    ("f.service(name, fn(input), auth?(headers), { queue })", "nil", "On <code>service:name</code>. <code>headers.get(n) · has(n)</code>. A falsy auth answers <code>{ error: \"unauthorized\", data: nil }</code>; a failing handler <code>{ error: \"internal_error\", data: nil }</code>; <code>nil</code> answers <code>{}</code>."),
                    ("f.command(name, data = {}, headers = {}, { timeout = 5000 ms })", "value · error", "No service → error value."),
                    ("f.connect(newConfig?) · f.close() · f.connection", "", "<code>connection</code> is the underlying <code>Nats</code>."),
                ],
                "example": r'''
import nats from "fogu:nats"
const f = nats.fogu("nats://127.0.0.1:4222")?
f.store.createKV("config", fn(e) { print("watch", e.operation, e.key, e.value) })?
const kv = f.store.kv("config")
kv.set("theme", { dark: true })?
print(kv.get("theme")?, kv.create("theme", 1)?, kv.keys()?)
f.sub({ topic: "orders.*" }, fn(data) { print("order", data) })
f.pub({ topic: "orders.new" }, { id: 1 })
f.service("math.double", fn(x) { return x * 2 }, fn(h) { return h.get("token") == "s3cret" })
print(f.command("math.double", 21, { token: "s3cret" })?)
print(f.command("math.double", 21)?)
''',
                "output": "watch PUT theme {dark: true}\n{dark: true} false [theme]\norder {id: 1}\n42\n{data: nil, error: unauthorized}",
                "output_title": "output (with nats-server -js)",
            },
        ],
    },
    {
        "id": "mod-rabbitmq",
        "group": M,
        "title": "fogu:rabbitmq",
        "import": 'import rabbitmq from "fogu:rabbitmq"',
        "intro": "RabbitMQ (AMQP 0-9-1, own client on the runtime's networking; <code>amqps://</code> uses TLS). Publishing waits for the broker's confirmation. A channel closed by an error "
        "(for example declaring a queue with other options) is reopened on the next call.",
        "blocks": [
            {
                "api": [
                    ("rabbitmq.connect(url?)", "RabbitMQ · error", "<code>amqp://user:pass@host:5672/vhost</code>, default <code>$AMQP_URL</code>."),
                    ("mq.exchange(name, kind = \"direct\", { durable, autoDelete, internal, arguments })", "nil · error", "<code>direct fanout topic headers</code>."),
                    ("mq.queue(name = \"\", { durable, exclusive, autoDelete, ttl, maxLength, deadLetter, passive })", "{ name, messages, consumers }", "\"\" gives a server-named queue."),
                    ("mq.bind(queue, exchange, key = \"\") · mq.unbind(…)", "nil · error", ""),
                    ("mq.publish(exchange, key, data, { persistent, headers, contentType, correlationId, replyTo, messageId, expiration, priority, mandatory })", "nil · error", "Maps are sent as JSON."),
                    ("mq.send(queue, data, opts?)", "nil · error", "Through the default exchange."),
                    ("mq.get(queue, { noAck })", "Message · nil", ""),
                    ("mq.consume(queue, { prefetch = 10, noAck, exclusive, tag })", "Consumer", "<code>next(timeout?)</code>, <code>tryNext()</code>, <code>cancel()</code>."),
                    ("msg.data · .json() · .routingKey · .exchange · .headers · .redelivered · …", "", "Properties: <code>contentType correlationId replyTo messageId type persistent</code>."),
                    ("msg.ack() · msg.nack({ requeue = true }) · msg.reject({ requeue })", "nil", ""),
                    ("mq.purge(queue) · mq.deleteQueue(name) · mq.deleteExchange(name) · mq.close()", "", ""),
                ],
                "example": r'''
import rabbitmq from "fogu:rabbitmq"
const mq = rabbitmq.connect("amqp://guest:guest@127.0.0.1:5672/%2f")?
mq.exchange("shop", "topic", { durable: true })?
mq.queue("emails", { durable: true })?
mq.bind("emails", "shop", "order.*")?
mq.publish("shop", "order.created", { id: 7 }, { persistent: true })?
const worker = mq.consume("emails", { prefetch: 5 })?
const msg = worker.next(2s)?
print(msg.routingKey, msg.json(), msg.persistent)
msg.ack()
''',
                "output": "order.created {id: 7} true",
                "output_title": "output (with RabbitMQ)",
            }
        ],
    },
    {
        "id": "mod-mqtt",
        "group": M,
        "title": "fogu:mqtt",
        "import": 'import mqtt from "fogu:mqtt"',
        "intro": "MQTT 3.1.1 and 5 (own client on the runtime's networking) with automatic reconnection "
        "(subscriptions are restored). QoS 0/1/2, retained messages, wills; with <code>version: 5</code> also message "
        "properties, subscription options and reason codes.",
        "blocks": [
            {
                "api": [
                    ("mqtt.connect(url?, { clientId, username, password, keepAlive, clean = true, will, timeout, version = 3, sessionExpiry })", "Mqtt · error", "<code>mqtt://</code> or <code>mqtts://</code>; <code>will: { topic, payload, qos, retain }</code>; <code>version: 5</code> speaks MQTT 5. A refused connection is an error with the broker's reason."),
                    ("m.publish(topic, data, { qos = 0, retain })", "nil · error", "QoS 1/2 wait for the broker's acknowledgement."),
                    ("MQTT 5 publish options: { contentType, responseTopic, correlationData, messageExpiry, userProperties }", "", "Ignored on 3.1.1."),
                    ("m.subscribe(filter | [filters], { qos, noLocal, retainAsPublished, retainHandling }) · m.unsubscribe(…)", "nil · error", "<code>+</code> one level, <code>#</code> the rest; the last three are MQTT 5."),
                    ("m.next(timeout?) · m.tryNext()", "Message · nil", "<code>{ topic, data, qos, retain, json(), matches(filter) }</code>; MQTT 5 adds <code>contentType responseTopic correlationData messageExpiry userProperties</code>."),
                    ("m.connected · m.clientId · m.version · m.subscriptions · m.close()", "", ""),
                    ("mqtt.matches(filter, topic)", "bool", ""),
                ],
                "example": r'''
import mqtt from "fogu:mqtt"
const m = mqtt.connect("mqtt://127.0.0.1:1883", { clientId: "monitor" })?
m.subscribe("home/+/temperature", { qos: 1 })?
m.publish("home/kitchen/temperature", "21.5", { qos: 1, retain: true })?
const msg = m.next(2s)?
print(msg.topic, msg.data, msg.qos, msg.matches("home/#"))
print(mqtt.matches("a/+/c", "a/b/c"))
''',
                "output": "home/kitchen/temperature 21.5 1 true\ntrue",
                "output_title": "output (with an MQTT broker)",
            }
        ],
    },
    {
        "id": "mod-broker",
        "group": M,
        "title": "fogu:broker",
        "import": 'import broker from "fogu:broker"   // also "fogu:mq"',
        "intro": "The Fogu broker inside the runtime: key/value store with TTLs, channels, pub/sub with wildcard topics "
        "(<code>chat.*</code>, <code>chat.**</code>), RPC (<code>act</code>/<code>call</code>), API keys with scoped permissions, "
        "persistence (SurrealKV), clusters and a web management UI (<code>b.admin</code>). It speaks the FoguMQ wire protocol, so the Bun, Go, Rust and Python clients "
        "(<code>fogu/client/</code>) and <a class=\"underline\" href=\"#mod-fogumq\">fogu:fogumq</a> talk to it. "
        "The inline client has the same API as a fogumq client, over an in-memory pipe.",
        "blocks": [
            {
                "api": [
                    ("broker.start(addr = \"127.0.0.1:7222\", { token, db, node, peers, secret, log, purgeEvery = 60s })", "Broker · error", "<code>token</code>: the root token (default <code>$FOGU_ROOT_TOKEN</code> or a random one). <code>db</code>: a SurrealKV folder (KV and API keys survive restarts, written in order by a background writer); in memory without it. A redb file of an earlier version is converted (kept as <code>&lt;db&gt;.redb.bak</code>). Also <code>broker.broker(…)</code> / <code>mq.broker(…)</code>."),
                    ("b.client({ token, requestTimeout })", "FoguMQ", "The inline client (root by default): <code>kv</code>, <code>channel(n).join/send/onMessage/sub/pub/act/call</code>, <code>apikeys</code>."),
                    ("b.admin(port | \"host:port\", { user = \"admin\", password = b.token, host, title, theme })", "{ url, port } · error", "The management UI (fogu:web + <a class=\"underline\" href=\"#mod-topcoat\">fogu:topcoat</a>): a sign-in page (session cookie; HTTP basic auth also works for scripts), a live overview with traffic per second and trend lines, channels with their members, a key-value browser/editor and API keys (create, copy the token once, revoke), light/dark themes. <code>theme</code>: <code>system</code> (default), <code>light</code>, <code>dark</code>. Keep the program alive with <code>Fogu.signal.wait()</code>."),
                    ("b.inspect() · b.keys({ prefix, limit = 200 })", "map · list", "<code>inspect</code>: <code>{ node, address, started, clients, channels: [{ name, members }], topics: [{ pattern, subscribers }], actions, remoteActions: [{ method, node }], peers, keys, apiKeys, messages, publications, calls, pendingCalls, persistent }</code>. <code>keys</code> → <code>[{ key, value, ttl }]</code> (ttl in seconds)."),
                    ("b.stats() · b.close() · b.url · b.address · b.port · b.node · b.token", "", "<code>stats()</code>: <code>{ node, clients, channels, topics, actions, remoteActions, peers, keys }</code>. <code>close()</code> ends every connection and releases the db."),
                    ("broker.connect(url, opts)", "FoguMQ · error", "A remote broker (same as <code>fogumq.connect</code>)."),
                    ("cluster: { node, peers: [addr…], secret }", "", "Nodes link to the <code>peers</code> they are given (one side is enough) and authenticate with <code>secret</code> (default: the token). Channel messages and publications reach every node; <code>call</code> goes to the node that registered the method; KV writes made while the nodes are linked are copied to every node (last write wins; <code>incr</code> is atomic per node; there is no catch-up of older keys)."),
                ],
                "example": r'''
import broker from "fogu:broker"
const b = broker.start("127.0.0.1:0", { token: "root1", node: "n1" })?
const c = b.client()
c.kv.set("visits", 10)?
print(c.kv.incr("visits")?)
const room = c.channel("chat")
const svc = room.act("add")?
fg fn() {
    while true {
        const [r, err] = svc.next()
        if err != nil { break }          // the broker closed
        r.reply(r.params.a + r.params.b)
    }
}()
print(room.call("add", { a: 1, b: 2 })?)
const sub = room.sub("news")?
room.pub("news", { id: 1 })?
print(sub.next(1s))

// a second node: RPC, publications and KV writes cross the link
const n2 = broker.start("127.0.0.1:0", { token: "root1", node: "n2", peers: [b.address] })?
Fogu.sleep(300ms)
const c2 = n2.client()
c.kv.set("shared", "from n1")?          // written while the nodes are linked
Fogu.sleep(100ms)
print(c2.channel("chat").call("add", { a: 20, b: 22 })?, c2.kv.get("shared")?)
print(n2.stats().remoteActions)
n2.close()
b.close()
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-kafka",
        "group": M,
        "title": "fogu:kafka",
        "import": 'import kafka from "fogu:kafka"',
        "intro": "Apache Kafka (and Redpanda) in pure Rust (rskafka): topics, producing and consuming, compression, TLS and "
        "SASL. Values: a string is sent as UTF-8, other values as JSON; received records have <code>value</code> as text. "
        "Without a <code>partition</code>, records with a key go to <code>hash(key) % partitions</code> (same key, same order), "
        "the others round-robin. Consumers keep their own offsets — Kafka consumer groups (coordinated rebalancing, "
        "offsets stored in Kafka) are not supported.",
        "blocks": [
            {
                "api": [
                    ("kafka.connect(brokers, { clientId, tls: true | { ca }, sasl: { mechanism, username, password }, timeout = 10s })", "Kafka · error", "<code>brokers</code>: a list or <code>\"h1:9092,h2:9092\"</code>. SASL: <code>PLAIN SCRAM-SHA-256 SCRAM-SHA-512</code>."),
                    ("k.topics() · k.createTopic(name, { partitions = 1, replication = 1 }) · k.deleteTopic(name) · k.offsets(topic)", "", "<code>createTopic</code> → false if it existed; <code>offsets</code> → <code>{ partition: { earliest, latest } }</code>."),
                    ("k.send(topic, value, { key, headers, partition, compression })", "{ partition, offset } · error", "<code>compression</code>: <code>none gzip lz4 snappy zstd</code>."),
                    ("k.sendBatch(topic, [value | { value, key, headers }], { partition, compression })", "list · error", ""),
                    ("k.consumer(topic, { from = \"latest\" | \"earliest\" | offset, partitions, maxWait = 500ms, maxBytes })", "KafkaConsumer · error", ""),
                    ("c.next(timeout = 1s) · c.poll(timeout = 1s) · c.seek(partition, offset) · c.offsets · c.close()", "", "<code>next</code> → a record or nil; <code>poll</code> → the records that arrived."),
                    ("record.topic · partition · offset · key · value · headers · timestamp · json() · bytes()", "", ""),
                ],
                "example": r'''
import kafka from "fogu:kafka"
const k = kafka.connect(["localhost:9092"])?
k.createTopic("orders", { partitions: 3 })?
const c = k.consumer("orders", { from: "earliest" })?
print(k.send("orders", { id: 1, total: 9.5 }, { key: "client-7", headers: { source: "fogu" } })?)
const r = c.next(3s)?
print(r.key, r.json(), r.headers)
''',
                "output": "{offset: 0, partition: 2}\\nclient-7 {id: 1, total: 9.5} {source: fogu}",
                "output_title": "output (with Kafka 3.9)",
            }
        ],
    },
    {
        "id": "mod-fogumq",
        "group": M,
        "title": "fogu:fogumq",
        "import": 'import fogumq from "fogu:fogumq"',
        "intro": "Client of the Fogu broker (binary protocol, JSON payloads): key/value store, channels, pub/sub, "
        "RPC and API keys. Methods that wait for the server let the other <code>fg</code> tasks run.",
        "blocks": [
            {
                "api": [
                    ("fogumq.connect(\"fogu://token@host:port\", { timeout, requestTimeout })", "FoguMQ · error", "Fields <code>connected authenticated isRoot url</code>; <code>ping() close()</code>."),
                    ("mq.kv.get(key) · set(key, value, ttl?) · del(key) · exists(key) · incr(key, n?) · decr(key, n?)", "value · error", "<code>get</code> gives nil for a missing key."),
                    ("mq.channel(name)", "Channel", "<code>join(alias?) leave() send(data) members() list()</code>."),
                    ("channel.onMessage()", "Listener", "<code>next(timeout?)</code> → <code>{ channel, data, ... }</code>; <code>tryNext() close()</code>."),
                    ("channel.subscribe(topic) · channel.publish(topic, data)", "Subscription · error", "Topics live under the channel: <code>name/topic</code>. Short forms: <code>sub</code>, <code>pub</code>."),
                    ("channel.act(method)", "Service · error", "<code>next()</code> → Request with <code>method params reply(result) fail(msg)</code>."),
                    ("channel.call(method, params?, timeout?)", "value · error", ""),
                    ("mq.apikeys.create(name, { channels, kv, topics, rpc }) · delete(id) · get(id) · list()", "value · error", "Root token only."),
                ],
                "example": r'''
import fogumq from "fogu:fogumq"
const mq = fogumq.connect(Fogu.env.FOGUMQ_URL)?
mq.kv.set("visits", 1)?
print(mq.kv.incr("visits")?)
const room = mq.channel("calc")
const svc = room.act("sum")?
fg fn() { const r = svc.next(); r.reply(r.params.reduce((a, n) => a + n, 0)) }()
print(room.call("sum", [1, 2, 3])?)
''',
                "output": "2\n6",
                "output_title": "output (with a Fogu broker)",
            }
        ],
    },
    {
        "id": "mod-foguMQ-client",
        "group": M,
        "title": "fogu:foguMQ-client",
        "import": 'import mqc from "fogu:foguMQ-client"',
        "intro": "Client of the FlowMQ v6 wire protocol (length-prefixed binary frames): numbered channels, pub/sub with "
        "wildcard topics (<code>*</code> one token, <code>&gt;</code> the rest), RPC, job queues and API keys. Each method "
        "sends exactly one protocol frame. The socket runs on the shared I/O runtime; methods that wait for the broker let "
        "the other <code>fg</code> tasks run, so one task can answer the requests another makes. Messages, requests and "
        "jobs are queued in <code>Subscription</code> / <code>Service</code> / <code>Worker</code> objects. Network and "
        "broker failures are <code>error</code> values. Payloads: a str is sent as is, any other value as JSON; received "
        "payloads are strs (use <code>json.parse</code>). <code>FLOWMQ_DEBUG=1</code> prints protocol errors to stderr.",
        "blocks": [
            {
                "h3": "Connection and channels",
                "api": [
                    ("mqc.connect(\"flowmq://token@host:7222?namespace=…\", { timeout, requestTimeout, clientId, autoReconnect })", "FoguMQClient · error",
                     "URL query: <code>clientId namespace requestTimeout connectTimeout reconnectBackoff</code> (ms) <code>maxFrame maxChannels autoReconnect</code>. "
                     "Defaults: port 7222, timeouts 5s. With <code>autoReconnect</code> the client reconnects and re-opens its channels, subscriptions, services and workers."),
                    ("mq.connected · namespace · authMode · keyId · sessionId · url", "", "From the broker's HELLO_OK. <code>authMode</code>: <code>master</code>, an API key mode, or <code>unknown</code>."),
                    ("mq.channelOpen(ch) · mq.channelClose(ch)", "nil · error", "Channels are ints 1..65535 and must be open before use; opening one twice is an error."),
                    ("mq.close()", "nil", "Sends CLOSE and ends the connection."),
                ],
            },
            {
                "h3": "Pub/sub",
                "api": [
                    ("mq.subscribe(ch, pattern, group?)", "Subscription · error", "<code>next(timeout?)</code> → <code>{ topic, group, data }</code> (nil after the timeout); <code>tryNext()</code>; fields <code>pattern group channel pending</code>; <code>close()</code> unsubscribes."),
                    ("mq.unsubscribe(ch, pattern, group?)", "nil · error", ""),
                    ("mq.publish(ch, topic, data)", "nil · error", "Fire and forget."),
                    ("mq.publishSync(ch, topic, data)", "nil · error", "Waits for the broker's ACK."),
                ],
            },
            {
                "h3": "RPC",
                "api": [
                    ("mq.serve(ch, service, topic)", "Service · error", "<code>next(timeout?)</code> → Request with <code>service topic data answered</code> and <code>reply(data) fail(msg)</code>. A request dropped without an answer fails on the caller's side. <code>close()</code> unserves."),
                    ("mq.unserve(ch, service, topic)", "nil · error", ""),
                    ("mq.request(ch, service, topic, data)", "str · error", "The reply payload; a <code>fail</code> or a missing handler is a broker error."),
                ],
            },
            {
                "h3": "Job queues",
                "api": [
                    ("mq.registerWorker(ch, queue, { filter = \">\", prefetch = 1, visibility = 30s })", "Worker · error", "<code>next(timeout?)</code> → Job with <code>id attempt deadline queue type data channel done</code>. <code>close()</code> sends WORKER_OFF."),
                    ("job.ack() · job.nack({ requeue = true, delay, reason }) · job.extend(duration)", "nil", "A job dropped without an answer is nacked and requeued."),
                    ("mq.workerOff(ch, queue)", "nil · error", ""),
                    ("mq.enqueue(ch, queue, type, data, { jobId, priority, delay, retries, ttl })", "{ jobId, seq } · error", "<code>delay</code> and <code>ttl</code>: durations or ms."),
                ],
            },
            {
                "h3": "API keys",
                "api": [
                    ("mq.apikeyCreate(ch, name, permissions, labels?)", "{ id, token, permissions } · error", "<code>permissions</code> is sent as JSON, e.g. <code>{ channels, services, queues, publish, subscribe, worker, enqueue }</code>; the key is created in the client's namespace."),
                    ("mq.apikeyRevoke(ch, id) · mq.apikeyGet(ch, id) · mq.apikeyList(ch)", "nil · str · str · error", "<code>get</code>/<code>list</code> return the broker's JSON text. All API key operations need a master token."),
                ],
                "example": r'''
import mqc from "fogu:foguMQ-client"
import json from "fogu:json"
const mq = mqc.connect(Fogu.env.FLOWMQ_URL)?
mq.channelOpen(1)?
mq.channelOpen(2)?
const sub = mq.subscribe(2, "orders.>")?
mq.publishSync(1, "orders.created", { id: 42 })?
print(json.parse(sub.next(2s).data))
const svc = mq.serve(2, "users", "get")?
fg fn() { const r = svc.next(); r.reply("user " + r.data) }()
print(mq.request(1, "users", "get", "42")?)
const jobs = mq.registerWorker(2, "emails")?
mq.enqueue(1, "emails", "welcome", "ana@example.com")?
const job = jobs.next(5s)
print(job.type, job.data)
job.ack()
mq.close()
''',
                "output": "{id: 42}\nuser 42\nwelcome ana@example.com",
                "output_title": "output (with a FlowMQ v6 broker)",
            },
        ],
    },
    {
        "id": "mod-bullmq",
        "group": M,
        "title": "fogu:bullmq",
        "import": 'import bullmq from "fogu:bullmq"',
        "intro": "Job queues on Redis, wire-compatible with <a class=\"underline\" href=\"https://docs.bullmq.io\">BullMQ</a> "
        "for Node.js v5 (on the <code>bullmq-rs</code> crate): Fogu and Node producers and workers can share the same "
        "queues, and dashboards such as Bull Board see them. The worker owns locks, stalled-job recovery, retries with "
        "backoff, concurrency and rate limits; each job is handed to the program, which finishes it with "
        "<code>complete</code> or <code>fail</code>. Redis failures are <code>error</code> values.",
        "blocks": [
            {
                "h3": "Queue",
                "api": [
                    ("bullmq.queue(name, { connection, prefix = \"bull\" })", "BullQueue · error", "<code>connection</code> is a <code>redis://</code> / <code>rediss://</code> URL (default <code>$REDIS_URL</code> or <code>redis://127.0.0.1:6379</code>)."),
                    ("q.add(name, data, opts?)", "job · error", "Options: <code>delay attempts backoff: { type: \"fixed\" | \"exponential\", delay, max } priority jobId ttl deduplication: { id, ttl }</code>."),
                    ("q.addBulk([{ name, data, opts }])", "list · error", ""),
                    ("q.getJob(id) · q.getJobs(states, start = 0, end = -1, asc = false)", "job | nil · list", "A job: <code>{ id name data state timestamp attemptsMade progress processedOn finishedOn failedReason returnvalue opts }</code>."),
                    ("q.counts() · q.count()", "map · int", "Per state: <code>wait active delayed prioritized completed failed paused waiting-children</code>."),
                    ("q.upsertJobScheduler(id, { every | pattern, tz, limit, immediately }, { name, data, opts })", "job · error", "Repeatable jobs (cron <code>pattern</code> or <code>every</code> duration)."),
                    ("q.removeJobScheduler(id)", "bool · error", ""),
                    ("q.pause() · resume() · isPaused() · drain() · remove(id) · clean(grace, limit, state) · obliterate({ force }) · getJobLogs(id)", "", ""),
                ],
            },
            {
                "h3": "Worker and events",
                "api": [
                    ("bullmq.worker(name, { connection, prefix, concurrency = 1, lockDuration, maxStalledCount, limiter: { max, duration } })", "BullWorker · error", ""),
                    ("w.next(timeout?) · w.tryNext()", "BullJob | nil · error", "Waits cooperatively (other <code>fg</code> tasks keep running); nil on timeout or after <code>close()</code>."),
                    ("job.complete(value?) · job.fail(reason)", "nil", "<code>fail</code> retries the job while <code>attempts</code> remain. A job dropped without an answer fails."),
                    ("job.progress(value) · job.log(text)", "nil", "Fields are those of the job map, plus <code>done</code>."),
                    ("w.pause() · w.resume() · w.close() · w.running · w.pending", "", ""),
                    ("bullmq.events(name, { connection, prefix, lastEventId })", "BullEvents · error", "<code>next(timeout?)</code> → <code>{ event, jobId, id, … }</code> (<code>added waiting active progress completed failed delayed stalled removed</code>)."),
                ],
                "text": "Limitation: the worker of <code>bullmq-rs</code> publishes <code>completed</code> events with an empty return "
                "value; <code>q.getJob(id).returnvalue</code> has the value given to <code>complete</code>.",
                "example": r"""
import bullmq from "fogu:bullmq"
const conn = { connection: "redis://127.0.0.1:6379" }
const q = bullmq.queue("emails", conn)?
q.add("welcome", { to: "ana@x.com" }, { attempts: 3, backoff: { type: "exponential", delay: 1s } })?
const w = bullmq.worker("emails", { connection: conn.connection, concurrency: 4 })?
while true {
    const job = w.next()?
    print(job.name, job.data.to, job.attemptsMade)
    job.progress(50)
    job.complete({ sent: true })          // or job.fail("smtp down") → retried
}
""",
                "output": "welcome ana@x.com 0",
                "output_title": "output (with Redis)",
            },
        ],
    },
]
