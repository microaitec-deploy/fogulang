"""Standard library reference: data formats and storage."""

D = "Data formats"
S = "Storage"

SECTIONS = [
    {
        "id": "mod-csv",
        "group": D,
        "title": "fogu:csv",
        "import": 'import csv from "fogu:csv"',
        "intro": "Rows with a header become maps; cells are typed (<code>42</code> → int, <code>3.5</code> → float, "
        "<code>true</code> → bool, empty → nil; codes like <code>007</code> stay text). The delimiter is guessed "
        "(<code>, ; tab |</code>).",
        "blocks": [
            {
                "api": [
                    ("csv.parse(text, { header = true, delimiter, types = true, trim })", "list · error", "Maps (or lists without header)."),
                    ("csv.stringify(rows, { header = true, delimiter = \",\", columns })", "str", "Rows: maps (a header line is written) or lists."),
                    ("csv.read(path, options?) · csv.write(path, rows, options?)", "list · nil", "The same, from/to a file."),
                ],
                "example": r'''
import csv from "fogu:csv"
const people = csv.parse("name;age;code;city\nAna;30;007;Lisboa\nBob;25.5;12;\n")?
print(people[0])
print(people[1].age, people[1].city)
print(csv.stringify([{ name: "Ana", note: "says \"hi\", ok" }, { name: "Bob", age: 3 }]))
csv.write("out.csv", people, { columns: ["name", "age"] })?
print(csv.read("out.csv")?)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-excel",
        "group": D,
        "title": "fogu:excel",
        "import": 'import excel from "fogu:excel"',
        "intro": "Reads <code>.xlsx .xlsm .xls .xlsb .ods</code>; writes <code>.xlsx</code> (bold, frozen header row, "
        "column widths fitted). Dates become ISO text.",
        "blocks": [
            {
                "api": [
                    ("excel.read(path, { sheet, header = true })", "list · map · error", "The first sheet (or <code>sheet</code>: name or index) as rows; <code>sheet: \"*\"</code> → <code>{ name: rows }</code>."),
                    ("excel.sheets(path)", "list · error", "Sheet names."),
                    ("excel.write(path, rows | { Sheet: rows }, { header = true, columns })", "nil · error", "One sheet per map key."),
                ],
                "example": r'''
import excel from "fogu:excel"
const products = [
    { id: 1, name: "Café", price: 4.5, stock: true },
    { id: 2, name: "Pão", price: 0.8, stock: false },
]
excel.write("shop.xlsx", { Products: products, Notes: [["checked", true]] })?
print(excel.sheets("shop.xlsx")?)
print(excel.read("shop.xlsx")?)
print(excel.read("shop.xlsx", { sheet: "Notes", header: false })?)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-yaml",
        "group": D,
        "title": "fogu:yaml",
        "import": 'import yaml from "fogu:yaml"',
        "blocks": [
            {
                "api": [
                    ("yaml.parse(text)", "value · error", ""),
                    ("yaml.parseAll(text)", "list · error", "Every document of a <code>---</code> stream."),
                    ("yaml.stringify(value)", "str", ""),
                    ("yaml.read(path) · yaml.write(path, value)", "value · nil", ""),
                ],
                "example": r'''
import yaml from "fogu:yaml"
const cfg = yaml.parse("server:\n  host: 0.0.0.0\n  ports: [80, 443]\ndebug: false\n")?
print(cfg.server.ports, cfg.debug)
print(yaml.stringify({ app: "fogu", tags: ["web", "cli"] }))
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-toml",
        "group": D,
        "title": "fogu:toml",
        "import": 'import toml from "fogu:toml"',
        "intro": "Dates become text. TOML has no null: <code>nil</code> values are left out when writing; the top level must be a map.",
        "blocks": [
            {
                "api": [
                    ("toml.parse(text)", "map · error", ""),
                    ("toml.stringify(map)", "str", ""),
                    ("toml.read(path) · toml.write(path, map)", "map · nil", ""),
                ],
                "example": r'''
import toml from "fogu:toml"
const t = toml.parse("title = \"app\"\n[db]\nurl = \"sqlite://x.db\"\npool = 5\n[[users]]\nname = \"ana\"\n")?
print(t.title, t.db.pool, t.users[0].name)
print(toml.stringify({ name: "fogu", version: 5, server: { port: 8080 } }))
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-xml",
        "group": D,
        "title": "fogu:xml",
        "import": 'import xml from "fogu:xml"',
        "intro": "An element is <code>{ name, attrs, children, text }</code>; <code>children</code> holds elements (maps) and "
        "text (str); <code>text</code> joins the element's own text.",
        "blocks": [
            {
                "api": [
                    ("xml.parse(text, { whitespace })", "map · error", "The root element."),
                    ("xml.stringify(element, { pretty = true, declaration = true })", "str", ""),
                    ("xml.read(path) · xml.write(path, element, options?)", "map · nil", ""),
                ],
                "example": r'''
import xml from "fogu:xml"
const feed = xml.parse("<feed lang=\"pt\"><item id=\"1\">Olá &amp; adeus</item><item id=\"2\"/></feed>")?
print(feed.name, feed.attrs.lang, feed.children.map((c) => c.attrs.id), feed.children[0].text)
print(xml.stringify({ name: "note", attrs: { to: "Ana" }, children: [{ name: "body", text: "1 < 2" }] }))
''',
                "run": True,
            }
        ],
    },
    # ------------------------------------------------------------------ storage
    {
        "id": "mod-kv",
        "group": S,
        "title": "fogu:kv",
        "import": 'import kv from "fogu:kv"',
        "intro": "An embedded key-value store in a single file (redb, ACID). Values are any Fogu value (stored as JSON).",
        "blocks": [
            {
                "api": [
                    ("kv.open(path)", "Store · error", "Creates the file if needed."),
                    ("store.set(key, value) · store.get(key)", "nil · value", "<code>get</code> → nil when missing."),
                    ("store.has(key) · store.delete(key)", "bool", ""),
                    ("store.keys() · store.len() · store.clear()", "list · int · nil", ""),
                    ("store.path · store.close()", "str · nil", ""),
                ],
                "example": r'''
import kv from "fogu:kv"
const db = kv.open("app.db")?
db.set("user:1", { name: "Ana", roles: ["admin"] })
db.set("visits", 41)
db.set("visits", db.get("visits") + 1)
print(db.get("user:1").name, db.get("visits"), db.has("nope"), db.keys(), db.len())
db.close()
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-sql",
        "group": S,
        "title": "fogu:sql",
        "import": 'import sql from "fogu:sql"',
        "intro": "SQLite, PostgreSQL and MySQL/MariaDB by URL: <code>sqlite://app.db</code>, <code>sqlite::memory:</code>, "
        "<code>postgres://user:pass@host/db</code>, <code>mysql://user:pass@host/db</code>. Parameters: <code>?</code> "
        "(SQLite/MySQL) or <code>$1</code> (PostgreSQL).",
        "blocks": [
            {
                "api": [
                    ("sql.open(url)", "Database · error", ""),
                    ("db.run(sql, params?)", "int · error", "One statement with parameters; returns the affected rows (alias <code>execute</code>)."),
                    ("db.query(sql, params?)", "list · error", "Rows as maps."),
                    ("db.one(sql, params?)", "map · nil · error", "The first row."),
                    ("db.exec(sql)", "nil · error", "Raw SQL without parameters — may hold several statements (migrations)."),
                    ("db.begin() · db.commit() · db.rollback()", "nil · error", "Transactions."),
                    ("db.driver · db.url · db.close()", "", ""),
                ],
                "example": r'''
import sql from "fogu:sql"
const db = sql.open("sqlite::memory:")?
db.exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INT)")?
const n = db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Ana", 30])?
db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Bob", 25])?
print("inserted:", n)
print(db.query("SELECT * FROM users WHERE age > ? ORDER BY name", [20])?)
print(db.one("SELECT COUNT(*) AS n FROM users")?.n, db.driver)
const [_, err] = db.query("SELECT * FROM nope")
print(err != nil)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-orm",
        "group": S,
        "title": "fogu:orm",
        "import": 'import { create_client } from "fogu:orm"',
        "intro": "A Prisma-style ORM for SQLite, PostgreSQL and MySQL. Models are described in a map; "
        "<code>migrate: \"auto\"</code> creates tables (with foreign keys) and adds new columns. Each model is "
        "reached as <code>db.&lt;model in camelCase&gt;</code>. Relations load with one query per level.",
        "blocks": [
            {
                "h3": "Client and schema",
                "api": [
                    ("create_client({ schema, migrate, log })", "Client · error", "<code>schema: { provider, url, models }</code>; <code>log: true</code> prints every SQL statement."),
                    ("field: { type, primary, autoincrement, required, unique, defaultValue }", "", "Types <code>Int BigInt String Bool Float DateTime Json</code>; defaults <code>\"now\"</code>, <code>\"uuid\"</code> or a value."),
                    ("relation: { model, kind, foreignKey, references }", "", "<code>one-to-many</code>, <code>many-to-one</code>, <code>one-to-one</code>."),
                    ("db.query(sql, params?) · db.execute(sql, params?)", "list · int", "Raw SQL."),
                    ("db.begin() · db.commit() · db.rollback() · db.migrate() · db.disconnect()", "", ""),
                ],
            },
            {
                "h3": "Model methods",
                "api": [
                    ("create({ data, include, select })", "map", "Missing required fields stop the program; a duplicate is an error value."),
                    ("createMany({ data: [...] })", "{ count }", ""),
                    ("findMany({ where, select, include, orderBy, take, skip })", "list", ""),
                    ("findFirst(opts) · findUnique(opts)", "map · nil", ""),
                    ("update({ where, data })", "map · error", "<code>data</code> values may be <code>{ increment, decrement, multiply, divide, set }</code>."),
                    ("updateMany({ where, data }) · deleteMany({ where })", "{ count }", ""),
                    ("upsert({ where, create, update })", "map", ""),
                    ("delete({ where })", "map · error", "The deleted record."),
                    ("count({ where })", "int", ""),
                ],
                "text": "<code>where</code>: <code>{ field: value }</code>, <code>{ field: { equals, not, in, notIn, lt, lte, gt, gte, "
                "contains, startsWith, endsWith, mode: \"insensitive\" } }</code>, <code>AND</code>/<code>OR</code>/<code>NOT</code>, "
                "and <code>nil</code> for IS NULL. <code>orderBy</code>: <code>{ field: \"asc\" | \"desc\" }</code> or a list of them.",
                "example": r'''
import { create_client } from "fogu:orm"

const db = create_client({
    schema: {
        provider: "sqlite",
        url: "sqlite::memory:",
        models: {
            User: {
                fields: {
                    id:    { type: "Int", primary: true, autoincrement: true },
                    email: { type: "String", required: true, unique: true },
                    role:  { type: "String", defaultValue: "user" },
                },
                relations: { posts: { model: "Post", kind: "one-to-many", foreignKey: "authorId" } }
            },
            Post: {
                fields: {
                    id:       { type: "Int", primary: true, autoincrement: true },
                    title:    { type: "String", required: true },
                    authorId: { type: "Int", required: true },
                },
                relations: { author: { model: "User", kind: "many-to-one", foreignKey: "authorId" } }
            }
        }
    },
    migrate: "auto",
})?

const ana = db.user.create({ data: { email: "ana@x.com" } })?
db.post.createMany({ data: [{ title: "Hello", authorId: ana.id }, { title: "Fogu", authorId: ana.id }] })?
print(ana)
const users = db.user.findMany({ include: { posts: { select: { title: true } } } })?
print(users[0].posts)
print(db.post.findFirst({ where: { title: { startsWith: "Fo" } }, include: { author: true } })?.author.email)
print(db.user.update({ where: { id: ana.id }, data: { role: "admin" } })?.role, db.post.count()?)
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-surrealkv",
        "group": S,
        "title": "fogu:surrealkv",
        "import": 'import surrealkv from "fogu:surrealkv"',
        "intro": "SurrealKV: an embedded, versioned LSM key-value store in a folder, with MVCC transactions (snapshot "
        "isolation, write conflicts as error values), the history of every key, checkpoints and encryption at rest "
        "(AES-256-GCM). Keys are strings, values any Fogu value (stored as JSON); times are ms since the epoch, like "
        "<code>time.now()</code>. Stores left open are flushed when the program exits.",
        "blocks": [
            {
                "api": [
                    ("surrealkv.open(dir, { versions, retention, sync, key, keyFile, vlog })", "SurrealKV · error", "<code>versions</code> keeps every version (for <code>retention</code>, a duration; 0 = forever); <code>sync</code> waits for fsync on each commit; <code>key</code> (64 hex digits) or <code>keyFile</code> encrypt the store. A folder already open is an error."),
                    ("db.get(key) · db.has(key) · db.set(key, value, { at })", "value · nil · bool", "<code>at</code> writes a version for a past time (versioned stores)."),
                    ("db.delete(key) · db.purge(key) · db.deleteRange(start, end) · db.deletePrefix(prefix)", "nil · error", "In a versioned store <code>delete</code> leaves a tombstone in the history; <code>purge</code> erases every version."),
                    ("db.scan({ prefix, start, end, limit, reverse, keys })", "list", "<code>[{ key, value }]</code> in key order (<code>end</code> excluded); <code>keys: true</code> → only the keys."),
                    ("db.keys(prefix?) · db.count(prefix?)", "list · int", ""),
                    ("db.getAt(key, at) · db.history(key, { limit, deleted })", "value · list", "Versioned stores. <code>history</code> → <code>[{ value, at, deleted }]</code>, newest first."),
                    ("db.batch({ set: { key: value }, delete: [keys] })", "nil · error", "One atomic commit."),
                    ("db.begin({ readOnly }) → tx · tx.get/set/delete/scan/keys/count/getAt/history · tx.commit() · tx.rollback()", "Txn", "Reads see a snapshot plus the transaction's own writes; <code>commit</code> → error value on a write conflict."),
                    ("db.checkpoint(dir) · db.restore(dir) · db.flush() · db.path · db.closed · db.close()", "", ""),
                ],
                "example": r'''
import surrealkv from "fogu:surrealkv"
const db = surrealkv.open("shop.db", { versions: true })?
db.set("user:1", { name: "Ana" })?
db.set("user:2", { name: "Bia" })?
print(db.scan({ prefix: "user:" }))
db.set("price", 10, { at: 1000 })?
db.set("price", 12, { at: 2000 })?
print(db.getAt("price", 1500), db.get("price"), db.history("price").len())

const t1 = db.begin()
const t2 = db.begin()
t1.set("stock", 5)
t2.set("stock", 7)
t1.commit()?
const [_, err] = t2.commit()
print(err != nil, db.get("stock"))
db.close()
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-redis",
        "group": S,
        "title": "fogu:redis",
        "import": 'import redis from "fogu:redis"',
        "intro": "Redis, Valkey and KeyDB. <strong>Every Redis command is a method</strong> (<code>r.lpush(…)</code>, "
        "<code>r.xadd(…)</code>) or goes through <code>r.command(\"CMD\", …)</code>; maps become <code>field value</code> "
        "pairs. Redis errors (<code>WRONGTYPE</code>, <code>NOAUTH</code>) are error values.",
        "blocks": [
            {
                "api": [
                    ("redis.connect(url | { host, port, username, password, db, tls, timeout, name })", "Redis · error", "<code>redis://[user:pass@]host[:port][/db]</code>, <code>rediss://</code> for TLS, default <code>$REDIS_URL</code>."),
                    ("r.set(key, value, { ex, px, exat, nx, xx, keepTtl, get })", "bool · value", "<code>false</code> when <code>nx</code>/<code>xx</code> prevented it."),
                    ("r.get(key) · r.del(keys) · r.incr(key) · r.expire(key, s) · r.ttl(key)", "", ""),
                    ("r.hset(key, map) · r.hget(key, f) · r.hgetall(key)", "int · str · map", ""),
                    ("r.zrange(key, start, stop, { withScores, rev, byScore })", "list", "With scores: <code>[{ member, score }]</code>."),
                    ("r.setJson(key, value, opts?) · r.getJson(key)", "bool · value", "JSON values."),
                    ("r.pipeline([[cmd, …args], …])", "list", "One round trip; a failed command is an error value in its place."),
                    ("r.transaction([...])", "list · error", "<code>MULTI … EXEC</code>."),
                    ("r.subscribe(...channels) · r.psubscribe(...patterns)", "Subscription", "Its own connection: <code>next(timeout?)</code> → <code>{ channel, message, pattern? }</code>, <code>tryNext()</code>, <code>close()</code>."),
                    ("r.publish(channel, message)", "int", "Receivers."),
                    ("r.close()", "nil", ""),
                ],
                "example": r'''
import redis from "fogu:redis"
const r = redis.connect("redis://127.0.0.1:6379/3")?
print(r.set("name", "Ana", { ex: 60 })?, r.get("name")?, r.ttl("name")?)
print(r.set("name", "x", { nx: true })?)              // exists: not set
r.hset("user:1", { name: "Ana", age: 30 })?
print(r.hgetall("user:1")?)
r.zadd("rank", [10, "ana", 25, "bob"])?
print(r.zrange("rank", 0, -1, { withScores: true, rev: true })?)
print(r.pipeline([["INCR", "n"], ["INCR", "n"], ["GET", "n"]])?)
const sub = r.subscribe("news")?
r.publish("news", "hello")?
print(sub.next(1s)?)
''',
                "output": "true Ana 60\nfalse\n{age: 30, name: Ana}\n[{member: bob, score: 25.0}, {member: ana, score: 10.0}]\n[1, 2, 2]\n{channel: news, message: hello}",
                "output_title": "output (with a Redis server)",
            }
        ],
    },
    {
        "id": "mod-s3",
        "group": S,
        "title": "fogu:s3",
        "import": 'import s3 from "fogu:s3"',
        "intro": "AWS S3 and compatible storage (MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces, Wasabi) with "
        "AWS Signature V4. Credentials default to <code>$AWS_ACCESS_KEY_ID</code>, <code>$AWS_SECRET_ACCESS_KEY</code>, "
        "<code>$AWS_REGION</code>, <code>$AWS_ENDPOINT_URL</code>.",
        "blocks": [
            {
                "api": [
                    ("s3.connect({ endpoint, region, accessKey, secretKey, sessionToken, bucket, pathStyle, timeout })", "S3", "Path-style URLs when an <code>endpoint</code> is given."),
                    ("store.put(key, body, { contentType, metadata, cacheControl, acl })", "{ etag } · error", "<code>body</code>: str, list of bytes, or any value (JSON)."),
                    ("store.upload(path, key?, opts?)", "{ etag } · error", "A local file."),
                    ("store.get(key) · store.getBytes(key) · store.download(key, path)", "str · list · nil", ""),
                    ("store.head(key) · store.exists(key)", "map · nil · bool", "<code>{ size, etag, contentType, lastModified, metadata }</code>."),
                    ("store.list(prefix?, { delimiter, max })", "list", "<code>{ key, size, etag, lastModified }</code>; folders <code>{ key, dir: true }</code>."),
                    ("store.copy(from, to, { fromBucket }) · store.delete(key | keys)", "", ""),
                    ("store.presign(key, { method = \"GET\", expires = 1h })", "str", "A temporary URL that needs no credentials."),
                    ("store.url(key) · store.withBucket(name)", "str · S3", ""),
                    ("store.createBucket(name?) · store.deleteBucket(name?) · store.listBuckets()", "", ""),
                ],
                "text": "Every method also takes <code>{ bucket }</code> in its options.",
                "example": r'''
import s3 from "fogu:s3"
const store = s3.connect({ endpoint: "http://127.0.0.1:9000", accessKey: "fogu", secretKey: "secret", bucket: "docs" })?
store.put("notes/hello.txt", "Olá, S3!", { metadata: { author: "ana" } })?
print(store.get("notes/hello.txt")?)
print(store.head("notes/hello.txt")?.metadata, store.exists("nope")?)
print(store.list("notes/")?.map((o) => o.key))
print(store.presign("notes/hello.txt", { expires: 15m }).contains("X-Amz-Signature="))
''',
                "output": "Olá, S3!\n{author: ana}\nfalse\n[notes/hello.txt]\ntrue",
                "output_title": "output (with an S3 server)",
            }
        ],
    },
    {
        "id": "mod-scylla",
        "group": S,
        "title": "fogu:scylla",
        "import": 'import scylla from "fogu:scylla"',
        "intro": "ScyllaDB and Apache Cassandra (CQL). Statements with parameters are prepared once and cached; "
        "parameters are converted to the column types (collections, UDTs, uuid, timestamp…).",
        "blocks": [
            {
                "api": [
                    ("scylla.connect(\"host:port\" | { hosts, keyspace, user, password, singleNode, addressMap })", "Scylla · error", "<code>addressMap</code> translates addresses of nodes behind Docker/NAT."),
                    ("db.execute(cql, params?)", "nil · error", ""),
                    ("db.query(cql, params?)", "list · error", "Rows as maps."),
                    ("db.one(cql, params?)", "map · nil · error", ""),
                    ("db.use(keyspace) · db.close()", "", ""),
                ],
                "example": r'''
import scylla from "fogu:scylla"
const db = scylla.connect({ hosts: ["127.0.0.1:9042"] })?
db.execute("CREATE KEYSPACE IF NOT EXISTS shop WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}")?
db.use("shop")?
db.execute("CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, tags set<text>)")?
db.execute("INSERT INTO users (id, name, tags) VALUES (?, ?, ?)", [1, "Ana", ["admin"]])?
print(db.one("SELECT * FROM users WHERE id = ?", [1])?)
''',
                "output": "{id: 1, name: Ana, tags: [admin]}",
                "output_title": "output (with a ScyllaDB server)",
            }
        ],
    },
    {
        "id": "mod-surrealdb",
        "group": S,
        "title": "fogu:surrealdb",
        "import": 'import surrealdb from "fogu:surrealdb"',
        "intro": "A SurrealDB client written on the runtime's networking (no SurrealDB crate): its JSON-RPC protocol over "
        "WebSocket (<code>ws://</code>, <code>wss://</code>: a session, live queries) or HTTP (<code>http://</code>, "
        "<code>https://</code>: <code>POST /rpc</code>, the session kept by the client). Tested with SurrealDB 3.3. "
        "Record ids are strings (<code>\"person:ana\"</code>); dates come back as ISO strings.",
        "blocks": [
            {
                "api": [
                    ("surrealdb.connect(url?, { namespace, database, user, pass, auth, token, define, timeout = 10s })", "SurrealDB · error", "Default <code>$SURREAL_URL</code> or <code>ws://127.0.0.1:8000</code> (<code>/rpc</code> is added); also <code>$SURREAL_NS $SURREAL_DB $SURREAL_USER $SURREAL_PASS</code>. <code>user</code>/<code>pass</code> sign in as a root user; <code>auth</code> is passed to <code>signin</code> as it is (<code>{ ns, db, ac, … }</code> for other levels and record access). <code>define: true</code> creates the namespace and database if needed (SurrealDB 3)."),
                    ("db.query(sql, vars?) · db.queryRaw(sql, vars?)", "list · error", "One value per statement; a failed statement is an error value (<code>statement 2: …</code>). <code>queryRaw</code> keeps <code>{ status, result, time }</code>."),
                    ("db.select(thing) · create(thing, data?) · insert(table, data) · insertRelation(table, data) · update(thing, data?) · upsert(thing, data?) · merge(thing, data) · patch(thing, ops, diff?) · delete(thing)", "value · error", "<code>thing</code>: a table (<code>\"person\"</code>) or a record (<code>\"person:ana\"</code>)."),
                    ("db.relate(from, relation, to, data?) · db.run(fn, args?)", "value · error", "<code>run(\"fn::total\", [1, 2])</code> or a built-in (<code>\"string::uppercase\"</code>)."),
                    ("db.live(table, { diff }) → live · db.listen(id) · live.next(timeout?) · live.tryNext() · live.kill() · live.id · live.done", "Live", "WebSocket only. Events <code>{ action: CREATE | UPDATE | DELETE | KILLED, record, result }</code>, one at a time; <code>nil</code> after the timeout. <code>listen</code> follows a <code>LIVE SELECT</code> run with <code>query</code>."),
                    ("db.use(ns, db) · signin(params) · signup(params) · authenticate(token) · invalidate() · set(name, value) · unset(name)", "", "<code>signin</code>/<code>signup</code> return the token and use it from then on; <code>set</code> defines a <code>$name</code> session variable."),
                    ("db.info() · db.version() · db.ping() · db.close() · db.url · db.transport · db.namespace · db.database · db.token · db.closed", "", ""),
                ],
                "example": r'''
import surrealdb from "fogu:surrealdb"
const db = surrealdb.connect("ws://127.0.0.1:8000", { namespace: "shop", database: "main", user: "root", pass: "root", define: true })?
db.create("person:ana", { name: "Ana", age: 30 })?
db.insert("person", [{ id: "bia", name: "Bia", age: 17 }, { id: "caio", name: "Caio", age: 41 }])?
const [adults, total] = db.query("SELECT name FROM person WHERE age >= $min ORDER BY name; RETURN count(SELECT * FROM person)", { min: 18 })?
print(adults, total)
const live = db.live("person")?
db.merge("person:bia", { age: 18 })?
const ev = live.next(2s)?
print(ev.action, ev.record, ev.result.age)
''',
                "output": "[{name: Ana}, {name: Caio}] 3\nUPDATE person:bia 18",
                "output_title": "output (with a SurrealDB server)",
            }
        ],
    },
    {
        "id": "mod-etcd",
        "group": S,
        "title": "fogu:etcd",
        "import": 'import etcd from "fogu:etcd"',
        "intro": "etcd v3 over gRPC (the official protos, no code generation): keys, ranges by prefix, leases, "
        "transactions, watches and cluster status. Calls go to every endpoint in turn; with <code>user</code> the "
        "token is renewed when it expires.",
        "blocks": [
            {
                "api": [
                    ("etcd.connect(url? | { endpoints, user, password, tls, timeout = 15s, connectTimeout = 5s })", "Etcd · error", "Default <code>$ETCDCTL_ENDPOINTS</code> or <code>http://127.0.0.1:2379</code>; <code>user</code> may be <code>\"name:pass\"</code> (also <code>$ETCDCTL_USER</code>/<code>$ETCDCTL_PASSWORD</code>)."),
                    ("db.get(key, { prefix, rangeEnd, limit, revision, keysOnly, countOnly, serializable, sort, sortTarget })", "{ kvs, count, more, revision }", "<code>kvs</code>: <code>[{ key, value, version, createRevision, modRevision, lease }]</code>."),
                    ("db.put(key, value, { lease, prevKv, ignoreValue, ignoreLease }) · db.delete(key, { prefix, rangeEnd, prevKv })", "{ revision, prevKv } · { deleted, prevKvs }", ""),
                    ("db.grant(ttl) · db.revoke(id) · db.ttl(id) · db.keepAlive(id)", "{ id, ttl } · … · Stream", "<code>keepAlive</code> refreshes the lease until <code>close()</code>; <code>next(timeout?)</code> gives <code>{ id, ttl }</code>."),
                    ("db.txn({ compare: [{ key, op, field, value }], success: [ops], failure: [ops] })", "{ succeeded, responses, revision }", "<code>field</code>: <code>value version createRevision modRevision lease</code>; ops <code>{ type: put | get | delete, key, … }</code>."),
                    ("db.watch(key, { prefix, rangeEnd, prevKv, revision, progressNotify })", "Watch", "<code>w.next(timeout?)</code> → <code>{ type: put | delete, kv, prevKv }</code>, one event at a time · <code>w.close()</code>."),
                    ("db.status() · db.memberList() · db.compact(revision) · db.endpoints · db.close()", "", ""),
                ],
                "example": r'''
import etcd from "fogu:etcd"
const db = etcd.connect("http://127.0.0.1:2379")?
db.put("/app/a", "1")?
db.put("/app/b", "2")?
print(db.get("/app/", { prefix: true })?.count)
const w = db.watch("/app/", { prefix: true })?
db.put("/app/a", "3")?
const ev = w.next(2s)?
print(ev.type, ev.kv.key, ev.kv.value)
w.close()
print(db.delete("/app/", { prefix: true })?.deleted)
''',
                "output": "2\nput /app/a 3\n2",
                "output_title": "output (with an etcd server)",
            }
        ],
    },
]
