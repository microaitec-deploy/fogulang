import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { RefSection } from '../RefSection';

interface Props {
  onRun: (code: string) => void;
}

export const DataAndStorageSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= FOGU:CSV ================= */}
      <section id="mod-csv" data-group="Data formats" data-title="fogu:csv" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:csv</code></h2>
        <p>Rows with a header become maps; cells are typed (<code>42</code> → int, <code>3.5</code> → float, <code>true</code> → bool, empty → nil; codes like <code>007</code> stay text). The delimiter is guessed (<code>, ; tab |</code>).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import csv from "fogu:csv"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>csv.parse(text, {`{ header = true, delimiter, types = true, trim }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Maps (or lists without header).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>csv.stringify(rows, {`{ header = true, delimiter = ",", columns }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Rows: maps (a header line is written) or lists.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>csv.read(path, options?) · csv.write(path, rows, options?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · nil</td><td className="py-2">The same, from/to a file.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import csv from "fogu:csv"
const people = csv.parse("name;age;code;city\\nAna;30;007;Lisboa\\nBob;25.5;12;\\n")?
print(people[0])
print(people[1].age, people[1].city)
print(csv.stringify([{ name: "Ana", note: "says \\"hi\\", ok" }, { name: "Bob", age: 3 }]))
csv.write("out.csv", people, { columns: ["name", "age"] })?
print(csv.read("out.csv")?)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`{age: 30, city: Lisboa, code: 007, name: Ana}
25.5 nil
name,note,age
Ana,"says ""hi"", ok",
Bob,,3

[{age: 30, name: Ana}, {age: 25.5, name: Bob}]`}
        />
      </section>

      {/* ================= FOGU:EXCEL ================= */}
      <section id="mod-excel" data-group="Data formats" data-title="fogu:excel" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:excel</code></h2>
        <p>Reads <code>.xlsx .xlsm .xls .xlsb .ods</code>; writes <code>.xlsx</code> (bold, frozen header row, column widths fitted). Dates become ISO text.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import excel from "fogu:excel"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>excel.read(path, {`{ sheet, header = true }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · map · error</td><td className="py-2">The first sheet (or <code>sheet</code>: name or index) as rows; <code>sheet: "*"</code> → <code>{`{ name: rows }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>excel.sheets(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Sheet names.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>excel.write(path, rows | {`{ Sheet: rows }`}, {`{ header = true, columns }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">One sheet per map key.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import excel from "fogu:excel"
const products = [
    { id: 1, name: "Café", price: 4.5, stock: true },
    { id: 2, name: "Pão", price: 0.8, stock: false },
]
excel.write("shop.xlsx", { Products: products, Notes: [["checked", true]] })?
print(excel.sheets("shop.xlsx")?)
print(excel.read("shop.xlsx")?)
print(excel.read("shop.xlsx", { sheet: "Notes", header: false })?)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`[Notes, Products]
[]
[[checked, true]]`}
        />
      </section>

      {/* ================= FOGU:YAML ================= */}
      <section id="mod-yaml" data-group="Data formats" data-title="fogu:yaml" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:yaml</code></h2>
        <DocCodeBlock lang="fogu" title="import" code={`import yaml from "fogu:yaml"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>yaml.parse(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">value · error</td><td className="py-2">Parse YAML text.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>yaml.parseAll(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Every document of a <code>---</code> stream.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>yaml.stringify(value)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Convert to YAML string.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>yaml.read(path) · yaml.write(path, value)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">value · nil</td><td className="py-2">Read or write YAML files.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import yaml from "fogu:yaml"
const cfg = yaml.parse("server:\\n  host: 0.0.0.0\\n  ports: [80, 443]\\ndebug: false\\n")?
print(cfg.server.ports, cfg.debug)
print(yaml.stringify({ app: "fogu", tags: ["web", "cli"] }))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`[80, 443] false
app: fogu
tags:
- web
- cli`}
        />
      </section>

      {/* ================= FOGU:TOML ================= */}
      <section id="mod-toml" data-group="Data formats" data-title="fogu:toml" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:toml</code></h2>
        <p>Dates become text. TOML has no null: <code>nil</code> values are left out when writing; the top level must be a map.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import toml from "fogu:toml"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toml.parse(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2">Parse TOML document.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toml.stringify(map)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Serialise map to TOML.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>toml.read(path) · toml.write(path, map)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil</td><td className="py-2">File I/O helpers.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import toml from "fogu:toml"
const t = toml.parse("title = \\"app\\"\\n[db]\\nurl = \\"sqlite://x.db\\"\\npool = 5\\n[[users]]\\nname = \\"ana\\"\\n")?
print(t.title, t.db.pool, t.users[0].name)
print(toml.stringify({ name: "fogu", version: 5, server: { port: 8080 } }))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`app 5 ana
name = "fogu"
version = 5

[server]
port = 8080`}
        />
      </section>

      {/* ================= FOGU:XML ================= */}
      <section id="mod-xml" data-group="Data formats" data-title="fogu:xml" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:xml</code></h2>
        <p>An element is <code>{`{ name, attrs, children, text }`}</code>; <code>children</code> holds elements (maps) and text (str); <code>text</code> joins the element's own text.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import xml from "fogu:xml"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>xml.parse(text, {`{ whitespace }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2">The root element.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>xml.stringify(element, {`{ pretty = true, declaration = true }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">XML document string.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>xml.read(path) · xml.write(path, element, options?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil</td><td className="py-2">File operations.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import xml from "fogu:xml"
const feed = xml.parse("<feed lang=\\"pt\\"><item id=\\"1\\">Olá &amp; adeus</item><item id=\\"2\\"/></feed>")?
print(feed.name, feed.attrs.lang, feed.children.map((c) => c.attrs.id), feed.children[0].text)
print(xml.stringify({ name: "note", attrs: { to: "Ana" }, children: [{ name: "body", text: "1 < 2" }] }))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`feed pt [1, 2] Olá & adeus
<?xml version="1.0" encoding="UTF-8"?>
<note to="Ana">
  <body>1 &lt; 2</body>
</note>`}
        />
      </section>

      {/* ================= FOGU:KV ================= */}
      <section id="mod-kv" data-group="Storage" data-title="fogu:kv" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:kv</code></h2>
        <p>An embedded key-value store in a single file (redb, ACID). Values are any Fogu value (stored as JSON).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import kv from "fogu:kv"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>kv.open(path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Store · error</td><td className="py-2">Creates the file if needed.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.set(key, value) · store.get(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · value</td><td className="py-2"><code>get</code> → nil when missing.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.has(key) · store.delete(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool</td><td className="py-2">Check or delete keys.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.keys() · store.len() · store.clear()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · int · nil</td><td className="py-2">Metadata and reset.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.path · store.close()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · nil</td><td className="py-2">Close store.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import kv from "fogu:kv"
const db = kv.open("app.db")?
db.set("user:1", { name: "Ana", roles: ["admin"] })
db.set("visits", 41)
db.set("visits", db.get("visits") + 1)
print(db.get("user:1").name, db.get("visits"), db.has("nope"), db.keys(), db.len())
db.close()`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`Ana 42 false [user:1, visits] 2`}
        />
      </section>
      <RefSection id="mod-surrealkv" onRun={onRun} />

      {/* ================= FOGU:SQL ================= */}
      <section id="mod-sql" data-group="Storage" data-title="fogu:sql" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:sql</code></h2>
        <p>SQLite, PostgreSQL and MySQL/MariaDB by URL: <code>sqlite://app.db</code>, <code>sqlite::memory:</code>, <code>postgres://user:pass@host/db</code>, <code>mysql://user:pass@host/db</code>. Parameters: <code>?</code> (SQLite/MySQL) or <code>$1</code> (PostgreSQL).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import sql from "fogu:sql"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>sql.open(url)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Database · error</td><td className="py-2">Open database connection.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.run(sql, params?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int · error</td><td className="py-2">One statement with parameters; returns the affected rows (alias <code>execute</code>).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.query(sql, params?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Rows as maps.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.one(sql, params?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil · error</td><td className="py-2">The first row.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.exec(sql)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Raw SQL without parameters — may hold several statements (migrations).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.begin() · db.commit() · db.rollback()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Transactions.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.driver · db.url · db.close()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Metadata and cleanup.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import sql from "fogu:sql"
const db = sql.open("sqlite::memory:")?
db.exec("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INT)")?
const n = db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Ana", 30])?
db.run("INSERT INTO users (name, age) VALUES (?, ?)", ["Bob", 25])?
print("inserted:", n)
print(db.query("SELECT * FROM users WHERE age > ? ORDER BY name", [20])?)
print(db.one("SELECT COUNT(*) AS n FROM users")?.n, db.driver)
const [_, err] = db.query("SELECT * FROM nope")
print(err != nil)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`inserted: 1
[{age: 30, id: 1, name: Ana}, {age: 25, id: 2, name: Bob}]
2 sqlite
true`}
        />
      </section>

      {/* ================= FOGU:ORM ================= */}
      <section id="mod-orm" data-group="Storage" data-title="fogu:orm" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:orm</code></h2>
        <p>A Prisma-style ORM for SQLite, PostgreSQL and MySQL. Models are described in a map; <code>migrate: "auto"</code> creates tables (with foreign keys) and adds new columns. Each model is reached as <code>db.&lt;model in camelCase&gt;</code>. Relations load with one query per level.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import { create_client } from "fogu:orm"`} />
        
        <h3 className="text-lg font-semibold mt-6">Client and schema</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>create_client({`{ schema, migrate, log }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Client · error</td><td className="py-2"><code>schema: {`{ provider, url, models }`}</code>; <code>log: true</code> prints every SQL statement.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>field: {`{ type, primary, autoincrement, required, unique, defaultValue }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Types <code>Int BigInt String Bool Float DateTime Json</code>; defaults <code>"now"</code>, <code>"uuid"</code> or a value.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>relation: {`{ model, kind, foreignKey, references }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>one-to-many</code>, <code>many-to-one</code>, <code>one-to-one</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.query(sql, params?) · db.execute(sql, params?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · int</td><td className="py-2">Raw SQL.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>db.begin() · db.commit() · db.rollback() · db.migrate() · db.disconnect()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Transactions and migrations.</td></tr>
            </tbody>
          </table>
        </div>

        <h3 className="text-lg font-semibold mt-6">Model methods</h3>
        <p><code>where</code>: <code>{`{ field: value }`}</code>, <code>{`{ field: { equals, not, in, notIn, lt, lte, gt, gte, contains, startsWith, endsWith, mode: "insensitive" } }`}</code>, <code>AND</code>/<code>OR</code>/<code>NOT</code>, and <code>nil</code> for IS NULL. <code>orderBy</code>: <code>{`{ field: "asc" | "desc" }`}</code> or a list of them.</p>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>create({`{ data, include, select }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map</td><td className="py-2">Missing required fields stop the program; a duplicate is an error value.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>createMany({`{ data: [...] }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">{`{ count }`}</td><td className="py-2">Batch inserts.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>findMany({`{ where, select, include, orderBy, take, skip }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Filter and paginate.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>findFirst(opts) · findUnique(opts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil</td><td className="py-2">Find single row.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>update({`{ where, data }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>data</code> values may be <code>{`{ increment, decrement, multiply, divide, set }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>updateMany({`{ where, data }`}) · deleteMany({`{ where }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">{`{ count }`}</td><td className="py-2">Bulk mutations.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>upsert({`{ where, create, update }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map</td><td className="py-2">Atomic upsert.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>delete({`{ where }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2">The deleted record.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>count({`{ where }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Total records count.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import { create_client } from "fogu:orm"

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
print(db.user.update({ where: { id: ana.id }, data: { role: "admin" } })?.role, db.post.count()? )`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`{email: ana@x.com, id: 1, role: user}
[{title: Hello}, {title: Fogu}]
ana@x.com
admin 2`}
        />
      </section>

      {/* ================= FOGU:REDIS ================= */}
      <section id="mod-redis" data-group="Storage" data-title="fogu:redis" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:redis</code></h2>
        <p>Redis, Valkey and KeyDB. <strong>Every Redis command is a method</strong> (<code>r.lpush(…)</code>, <code>r.xadd(…)</code>) or goes through <code>r.command("CMD", …)</code>; maps become <code>field value</code> pairs. Redis errors (<code>WRONGTYPE</code>, <code>NOAUTH</code>) are error values.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import redis from "fogu:redis"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>redis.connect(url | {`{ host, port, username, password, db, tls, timeout, name }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Redis · error</td><td className="py-2"><code>redis://[user:pass@]host[:port][/db]</code>, <code>rediss://</code> for TLS, default <code>$REDIS_URL</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.set(key, value, {`{ ex, px, exat, nx, xx, keepTtl, get }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · value</td><td className="py-2"><code>false</code> when <code>nx</code>/<code>xx</code> prevented it.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.get(key) · r.del(keys) · r.incr(key) · r.expire(key, s) · r.ttl(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Key commands.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.hset(key, map) · r.hget(key, f) · r.hgetall(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int · str · map</td><td className="py-2">Hash commands.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.zrange(key, start, stop, {`{ withScores, rev, byScore }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">With scores: <code>[{`{ member, score }`}]</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.setJson(key, value, opts?) · r.getJson(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · value</td><td className="py-2">JSON values.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.pipeline([[cmd, …args], …])</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">One round trip; a failed command is an error value in its place.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.transaction([...])</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2"><code>MULTI … EXEC</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.subscribe(...channels) · r.psubscribe(...patterns)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Subscription</td><td className="py-2">Its own connection: <code>next(timeout?)</code> → <code>{`{ channel, message, pattern? }`}</code>, <code>tryNext()</code>, <code>close()</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.publish(channel, message)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">int</td><td className="py-2">Receivers.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>r.close()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Close client.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import redis from "fogu:redis"
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
print(sub.next(1s)?)`}
        />
        <DocCodeBlock
          lang="text"
          title="output (with a Redis server)"
          code={`true Ana 60
false
{age: 30, name: Ana}
[{member: bob, score: 25.0}, {member: ana, score: 10.0}]
[1, 2, 2]
{channel: news, message: hello}`}
        />
      </section>

      {/* ================= FOGU:S3 ================= */}
      <section id="mod-s3" data-group="Storage" data-title="fogu:s3" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:s3</code></h2>
        <p>AWS S3 and compatible storage (MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces, Wasabi) with AWS Signature V4. Credentials default to <code>$AWS_ACCESS_KEY_ID</code>, <code>$AWS_SECRET_ACCESS_KEY</code>, <code>$AWS_REGION</code>, <code>$AWS_ENDPOINT_URL</code>.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import s3 from "fogu:s3"`} />
        <p>Every method also takes <code>{`{ bucket }`}</code> in its options.</p>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>s3.connect({`{ endpoint, region, accessKey, secretKey, sessionToken, bucket, pathStyle, timeout }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">S3</td><td className="py-2">Path-style URLs when an <code>endpoint</code> is given.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.put(key, body, {`{ contentType, metadata, cacheControl, acl }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">{`{ etag }`} · error</td><td className="py-2"><code>body</code>: str, list of bytes, or any value (JSON).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.upload(path, key?, opts?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">{`{ etag }`} · error</td><td className="py-2">A local file.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.get(key) · store.getBytes(key) · store.download(key, path)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · list · nil</td><td className="py-2">Download content.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.head(key) · store.exists(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · nil · bool</td><td className="py-2"><code>{`{ size, etag, contentType, lastModified, metadata }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.list(prefix?, {`{ delimiter, max }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2"><code>{`{ key, size, etag, lastModified }`}</code>; folders <code>{`{ key, dir: true }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.copy(from, to, {`{ fromBucket }`}) · store.delete(key | keys)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Copy or remove objects.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.presign(key, {`{ method = "GET", expires = 1h }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">A temporary URL that needs no credentials.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.url(key) · store.withBucket(name)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · S3</td><td className="py-2">URL or bucket wrapper.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>store.createBucket(name?) · store.deleteBucket(name?) · store.listBuckets()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Bucket management.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import s3 from "fogu:s3"
const store = s3.connect({ endpoint: "http://127.0.0.1:9000", accessKey: "fogu", secretKey: "secret", bucket: "docs" })?
store.put("notes/hello.txt", "Olá, S3!", { metadata: { author: "ana" } })?
print(store.get("notes/hello.txt")?)
print(store.head("notes/hello.txt")?.metadata, store.exists("nope")?)
print(store.list("notes/")?.map((o) => o.key))
print(store.presign("notes/hello.txt", { expires: 15m }).contains("X-Amz-Signature="))`}
        />
        <DocCodeBlock
          lang="text"
          title="output (with an S3 server)"
          code={`Olá, S3!
{author: ana}
false
[notes/hello.txt]
true`}
        />
      </section>

      {/* ================= FOGU:SCYLLA ================= */}
      <RefSection id="mod-scylla" onRun={onRun} />
      <RefSection id="mod-surrealdb" onRun={onRun} />
      <RefSection id="mod-etcd" onRun={onRun} />
    </>
  );
};
