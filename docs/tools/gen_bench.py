#!/usr/bin/env python3
"""Turns bench/results.json (written by `fogu bench/run.fg`) into the benchmark
pages: docs/benchmarks.md and docs/view/src/data/benchmarks.generated.ts.

    fogu bench/run.fg && python docs/tools/gen_bench.py
"""
import datetime
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
RESULTS = ROOT / "bench" / "results.json"
# an earlier run kept for the "before" column (fogu bench/run.fg, then copied here)
BASELINE = ROOT / "bench" / "baseline.json"
NET = ROOT / "bench" / "results_net.json"
# the servers/concurrency run kept for the "Fogu (before)" rows (a copy of an earlier results_net.json)
NET_BASELINE = ROOT / "bench" / "baseline_net.json"

# what "memory per task" counts, per runtime (peak RSS with N tasks − with 0, ÷ N)
MEMORY_NOTES = [
    ("Fogu", "a task is a coroutine with its own stack (not a future): the stack pages it touched (2 pages of 4 KB "
             "for this program; huge pages are turned off on task stacks), the closure and its environment, its "
             "scheduler entry and its wait entry on the channel."),
    ("Python", "the coroutine frame, the asyncio Task object and the waiter future on the Event."),
    ("Bun", "the async function's suspended state and its promises."),
    ("All", "plus indirect allocations and memory the allocator keeps after freeing: the number is what the "
            "operating system sees, not only the task's own structures."),
]

NET_METHOD = [
    "HTTP: GET /hello returning a small JSON body; load from oha (64 connections, 5 s) on the same machine, so the "
    "load generator competes for the CPU too. Python is measured with its standard library (a thread per connection) "
    "and with aiohttp (asyncio).",
    "WebSocket: an echo server; the same Bun client for every server opens 50 connections, each sends an 89-byte "
    "message and waits for its echo, for 5 s.",
    "Memory per task: 20,000 tasks parked on a channel / event / promise, then released.",
    "Concurrent waits: 10,000 tasks each wait 100 ms; the ideal is 100 ms, the rest is scheduling cost.",
    "Ping-pong: 100,000 round trips between two tasks through channels (Python: asyncio.Queue; JavaScript has no "
    "channels, so a minimal promise queue).",
    "Fogu (before): the same benchmarks from bench/baseline_net.json, a run made before the server, WebSocket and "
    "task changes listed above, on the same machine.",
]
MD = ROOT / "docs" / "benchmarks.md"
TS = ROOT / "docs" / "view" / "src" / "data" / "benchmarks.generated.ts"
HTML = ROOT / "docs" / "docs.html"
COLORS = {"Fogu": "#f97316", "Fogu (no JIT)": "#fcd34d", "Bun": "#ec4899", "Go": "#14b8a6", "Python": "#0ea5e9"}

ABOUT = {
    "startup": "Start the runtime and print one line.",
    "fib": "Recursive calls: fib(32).",
    "loop": "An integer loop: 20 million iterations.",
    "mandel": "Floating point: a 300×300 Mandelbrot grid, 50 iterations.",
    "strings": "Build 200k strings, join, split and measure them.",
    "maps": "Insert 300k keys in a map, then read them back.",
    "records": "JSON: stringify and parse 50k records.",
    "sort": "Generate and sort 300k pseudo-random integers.",
}
# Bun is the main rival (a scripting runtime); Go is the ceiling
LANGS = ["Fogu", "Fogu (no JIT)", "Bun", "Go", "Python"]

# what was optimized, in order (shown on every benchmark page)
CHANGES = [
    ("Native modules built on first import",
     "The runtime used to build all 70 standard-library modules at start-up; now each one is built the first "
     "time a program imports it. A script starts in about 8 ms instead of about 50 ms."),
    ("list.sort(): merge sort and fast paths",
     "sort() was a binary insertion sort: few comparisons but O(n²) element moves (300k numbers took minutes). "
     "It is now a stable merge sort, and lists of only integers, only strings or only numbers use the "
     "standard library's sorts directly. Results and comparator rules are unchanged."),
    ("JSON: one buffer and a byte parser",
     "json.stringify wrote a string per value and joined them; it now writes into one buffer, escapes strings "
     "as JSON requires (control characters became invalid Rust-style escapes before; NaN and infinity become "
     "null). json.parse copied the whole text into a list of characters; it now reads the bytes in place and "
     "copies strings without escapes in one step."),
    ("Strings: shared, copy-on-write",
     "A string value owned its text, so reading a variable, passing an argument or iterating a list copied it. "
     "Strings are now reference-counted and copied only when modified; `s = s + x` and `s += x` append in place "
     "(they copied the whole string each time); the number of characters is cached, so `s.len()` and `s[i]` on "
     "ASCII text are O(1) (`s[i]` used to collect every character first)."),
    ("Loops reuse their scope",
     "Every iteration of a loop allocated two environments (the loop variable's and the body's); they are now "
     "reused unless a closure or task captured them. Variable lookups hash names with FxHash instead of SipHash."),
    ("Tasks: pooled stacks, smaller frames",
     "Finished `fg` tasks give their stack back to a pool instead of an mmap/munmap per task (or per Socket.IO "
     "event). The interpreter's eval / exec / call dispatch keeps each kind of expression in its own function, "
     "so a call uses ~1.8 KB of native stack instead of ~3 KB: a parked task takes about half the memory."),
    ("Servers: no waiting for the program",
     "HTTP requests were only served between the program's calls, so a server kept alive with "
     "`time.delay(100ms)` answered up to 100 ms late; now the server wakes a task that serves them at once, and "
     "every handler runs in its own task. WebSocket connections are tasks on the shared runtime instead of a "
     "thread each polling every 2 ms, and `app.ws` uses the upgraded connection directly."),
    ("list.join(): one buffer",
     "join() built a temporary string per item and joined them; it now writes every item into one buffer."),
]


def main():
    r = json.loads(RESULTS.read_text(encoding="utf-8"))
    before = {}
    if BASELINE.exists():
        b = json.loads(BASELINE.read_text(encoding="utf-8"))
        before = {item["bench"]: item["times"].get("Fogu") for item in b["results"]}
    date = datetime.datetime.fromtimestamp(r["date"] / 1000, datetime.timezone.utc).strftime("%Y-%m-%d")
    rows = []
    for item in r["results"]:
        times = {k: round(v, 1) for k, v in item["times"].items()}
        old = before.get(item["bench"])
        rows.append({"bench": item["bench"], "about": ABOUT.get(item["bench"], ""), "times": times,
                     "before": None if old is None else round(old, 1)})

    # docs/benchmarks.md
    lines = [
        "# Benchmarks: Fogu vs Bun vs Go vs Python",
        "",
        "Generated from `bench/results.json` by `docs/tools/gen_bench.py` — do not edit by hand.",
        "",
        f"Machine: {r['cpu']} ({r['cores']} logical CPUs), {r['os']}. Date: {date}.",
        "Versions: " + ", ".join(r["versions"].values()) + ". " + fmt_load(r.get("load")),
        f"Each number is the **median of {r['runs']} runs**, in milliseconds, of the whole process",
        "(start-up included). Lower is better. The languages are interleaved (each repetition runs",
        "every language once), so changes in the machine's load affect all of them alike. Every",
        "program prints the same result in every language; the runner checks it. Bun is the main rival, Go the ceiling.",
        "",
        "| benchmark | what it measures | " + " | ".join(LANGS) + " |",
        "|---|---|" + "|".join("---:" for _ in LANGS) + "|",
    ]
    for row in rows:
        cells = []
        for lang in LANGS:
            t = row["times"].get(lang)
            cells.append("—" if t is None else f"{t:,.0f}")
        lines.append(f"| {row['bench']} | {row['about']} | " + " | ".join(cells) + " |")
    if before:
        lines += [
            "",
            "## Before and after the optimizations",
            "",
            "Fogu (with the JIT), median in ms: the run kept in `bench/baseline.json` (before the string, loop,",
            "task and server changes below) against this one — two separate runs on the same machine, so",
            "differences of a few percent are noise.",
            "",
            "| benchmark | before | now | change |",
            "|---|---:|---:|---:|",
        ]
        for row in rows:
            now = row["times"].get("Fogu")
            if row["before"] and now:
                pct = (now - row["before"]) / row["before"] * 100
                lines.append(f"| {row['bench']} | {row['before']:,.0f} | {now:,.0f} | {pct:+.0f}% |")
    lines += ["", "## Optimizations made", ""]
    for title, detail in CHANGES:
        lines.append(f"- **{title}.** {detail}")
    lines += [
        "",
        "## Reading the numbers",
        "",
        "- **Fogu** runs with its Cranelift JIT (the default): hot functions and loops over numbers",
        "  are compiled to machine code. **Fogu (no JIT)** is the tree-walking interpreter alone",
        "  (`fogu --no-jit`), the semantic reference.",
        "- Work that stays in the runtime (strings, maps, JSON, sorting) runs in Rust code in all",
        "  modes; what matters there is the cost of the calls around it.",
        "- Start-up: native modules are built on their first import, so a script starts in a few",
        "  milliseconds.",
        "",
        "## Run them yourself",
        "",
        "```bash",
        "cargo build --release",
        "target/release/fogu bench/run.fg            # 5 runs per case (needs python3 and bun)",
        "target/release/fogu bench/run.fg 3 fib loop  # fewer runs, some cases",
        "python docs/tools/gen_bench.py              # refresh this page and docs/view",
        "```",
        "",
        "The programs are in `bench/` (`<name>.fg`, `<name>.py`, `<name>.js`).",
        "",
    ]
    net = with_before(json.loads(NET.read_text(encoding="utf-8")) if NET.exists() else None)
    if net:
        lines += net_markdown(net)
    MD.write_text("\n".join(lines), encoding="utf-8")

    data = {
        "date": date, "cpu": r["cpu"], "cores": r["cores"], "os": r["os"], "runs": r["runs"],
        "versions": r["versions"], "langs": LANGS, "rows": rows,
        "changes": [{"title": t, "detail": d} for t, d in CHANGES],
        "load": r.get("load"),
        "net": net_data(net) if (net := (with_before(json.loads(NET.read_text(encoding="utf-8")) if NET.exists() else None))) else None,
    }
    TS.write_text(
        "// Generated by docs/tools/gen_bench.py from bench/results.json — do not edit.\n"
        "export const BENCHMARKS = " + json.dumps(data, indent=2, ensure_ascii=False) + " as const;\n",
        encoding="utf-8",
    )
    write_html(r, date, rows, with_before(json.loads(NET.read_text(encoding="utf-8")) if NET.exists() else None))
    print(f"wrote {MD.relative_to(ROOT)}, {TS.relative_to(ROOT)} and the section of {HTML.relative_to(ROOT)}")


def fmt_load(load):
    if not load or load[0] is None:
        return ""
    return f"Machine load (1-minute average): {load[0]:.1f} at the start, {load[1]:.1f} at the end."


def with_before(n):
    """Adds a "Fogu (before)" row from bench/baseline_net.json after each Fogu row."""
    if not n or not NET_BASELINE.exists():
        return n
    b = json.loads(NET_BASELINE.read_text(encoding="utf-8"))
    n = dict(n)

    def merge(now, old):
        prev = next((x for x in old if x["name"] == "Fogu"), None)
        out = []
        for x in now:
            out.append(x)
            if x["name"] == "Fogu" and prev:
                out.append(dict(prev, name="Fogu (before)"))
        return out

    for key in ("http", "ws", "sleeps", "pingpong"):
        if n.get(key) and b.get(key):
            n[key] = merge(n[key], b[key])
    if n.get("tasks") and b.get("tasks"):
        n["tasks"] = dict(n["tasks"], perTask=merge(n["tasks"]["perTask"], b["tasks"]["perTask"]))
    return n


def net_data(n):
    return {
        "date": datetime.datetime.fromtimestamp(n["date"] / 1000, datetime.timezone.utc).strftime("%Y-%m-%d"),
        "rounds": n["rounds"], "versions": n["versions"], "load": n.get("load"),
        "http": n.get("http", []), "ws": n.get("ws", []),
        "tasks": n.get("tasks"), "sleeps": n.get("sleeps", []), "pingpong": n.get("pingpong", []),
        "memoryNotes": [{"name": a, "text": b} for a, b in MEMORY_NOTES], "method": NET_METHOD,
    }


def net_markdown(n):
    out = ["", "## Servers and concurrency", "",
           f"Median of {n['rounds']} rounds (each round runs every implementation once). "
           + ", ".join(n["versions"].values()) + ". " + fmt_load(n.get("load")), ""]
    if n.get("http"):
        out += ["### HTTP throughput", "", "| server | requests/s | p50 | p99 | non-200 |", "|---|---:|---:|---:|---:|"]
        for x in n["http"]:
            out.append(f"| {x['name']} | {x['rps']:,.0f} | {x['p50']:.2f} ms | {x['p99']:.2f} ms | {x['errors']} |")
        out.append("")
    if n.get("ws"):
        out += ["### WebSocket echo", "", "| server | messages/s | errors |", "|---|---:|---:|"]
        for x in n["ws"]:
            out.append(f"| {x['name']} | {x['mps']:,.0f} | {x['errors']} |")
        out.append("")
    if n.get("tasks"):
        out += [f"### Memory per task ({n['tasks']['count']:,} tasks)", "", "| runtime | per task |", "|---|---:|"]
        for x in n["tasks"]["perTask"]:
            out.append(f"| {x['name']} | {x['bytes'] / 1024:.1f} KB |")
        out += ["", "What it counts (peak RSS with N tasks minus with none, divided by N):", ""]
        out += [f"- **{a}:** {b}" for a, b in MEMORY_NOTES]
        out.append("")
    for key, title in (("sleeps", "10,000 concurrent waits of 100 ms"), ("pingpong", "100,000 channel round trips")):
        if n.get(key):
            out += [f"### {title}", "", "| runtime | total |", "|---|---:|"]
            out += [f"| {x['name']} | {x['ms']:,.0f} ms |" for x in n[key]]
            out.append("")
    out += ["### Method", ""] + [f"- {m}" for m in NET_METHOD]
    out += ["", "```bash", "fogu bench/run_net.fg            # needs Linux, bun, oha and bench/.venv (aiohttp, websockets)",
            "python docs/tools/gen_bench.py", "```", ""]
    return out


def net_html(n):
    if not n:
        return ""
    def table(head, rows):
        th = "".join(f'<th class="py-2 pr-4{" text-right" if i else ""}">{esc(h)}</th>' for i, h in enumerate(head))
        body = "".join("<tr>" + "".join(f'<td class="py-2 pr-4{" text-right tabular-nums" if i else ""}">{esc(c)}</td>' for i, c in enumerate(r)) + "</tr>" for r in rows)
        return (f'<div class="overflow-x-auto mt-3"><table class="w-full text-sm"><thead><tr class="text-left border-b border-slate-200 dark:border-slate-800">{th}</tr></thead>'
                f'<tbody class="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">{body}</tbody></table></div>')
    h = ['<h3 class="text-lg font-semibold mt-10">Servers and concurrency</h3>',
         f'<p class="mt-2 text-sm text-slate-500">Median of {n["rounds"]} rounds · {esc(", ".join(n["versions"].values()))} · {esc(fmt_load(n.get("load")))}</p>']
    if n.get("http"):
        h += ['<h4 class="font-semibold mt-6">HTTP throughput</h4>', table(["server", "requests/s", "p50", "p99", "non-200"],
              [[x["name"], f'{x["rps"]:,.0f}', f'{x["p50"]:.2f} ms', f'{x["p99"]:.2f} ms', x["errors"]] for x in n["http"]])]
    if n.get("ws"):
        h += ['<h4 class="font-semibold mt-6">WebSocket echo</h4>', table(["server", "messages/s", "errors"], [[x["name"], f'{x["mps"]:,.0f}', x["errors"]] for x in n["ws"]])]
    if n.get("tasks"):
        h += [f'<h4 class="font-semibold mt-6">Memory per task ({n["tasks"]["count"]:,} tasks)</h4>',
              table(["runtime", "per task"], [[x["name"], f'{x["bytes"] / 1024:.1f} KB'] for x in n["tasks"]["perTask"]]),
              '<ul class="list-disc pl-6 mt-2 text-sm space-y-1">' + "".join(f"<li><b>{esc(a)}:</b> {esc(b)}</li>" for a, b in MEMORY_NOTES) + "</ul>"]
    for key, title in (("sleeps", "10,000 concurrent waits of 100 ms"), ("pingpong", "100,000 channel round trips")):
        if n.get(key):
            h += [f'<h4 class="font-semibold mt-6">{title}</h4>', table(["runtime", "total"], [[x["name"], f'{x["ms"]:,.0f} ms'] for x in n[key]])]
    h += ['<h4 class="font-semibold mt-6">Method</h4><ul class="list-disc pl-6 mt-2 text-sm space-y-1">' + "".join(f"<li>{esc(m)}</li>" for m in NET_METHOD) + "</ul>"]
    return "".join(h)


def esc(t):
    return str(t).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def before_after_html(rows):
    body = ""
    for row in rows:
        now = row["times"].get("Fogu")
        if not (row.get("before") and now):
            continue
        pct = (now - row["before"]) / row["before"] * 100
        color = "text-emerald-600" if pct < -3 else ("text-red-600" if pct > 3 else "text-slate-500")
        body += (f'<tr><td class="py-2 pr-4"><code>{esc(row["bench"])}</code></td><td class="py-2 pr-4 text-right tabular-nums">{row["before"]:.0f}</td>'
                 f'<td class="py-2 pr-4 text-right tabular-nums">{now:.0f}</td><td class="py-2 text-right tabular-nums {color}">{pct:+.0f}%</td></tr>')
    if not body:
        return ""
    return ('      <h3 class="text-lg font-semibold mt-8">Before and after the optimizations</h3>'
            '<p class="mt-2 text-sm text-slate-500">Fogu with the JIT, median in ms: bench/baseline.json against this run.</p>'
            '<div class="overflow-x-auto mt-3"><table class="w-full text-sm"><thead><tr class="text-left border-b border-slate-200 dark:border-slate-800">'
            '<th class="py-2 pr-4">benchmark</th><th class="py-2 pr-4 text-right">before</th><th class="py-2 pr-4 text-right">now</th><th class="py-2 text-right">change</th></tr></thead>'
            f'<tbody class="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">{body}</tbody></table></div>')


def write_html(r, date, rows, net=None):
    """The Benchmarks section of docs/docs.html, between its two markers."""
    text = HTML.read_text(encoding="utf-8")
    start, end = "<!-- BENCHMARKS:START", "<!-- BENCHMARKS:END -->"
    if start not in text or end not in text:
        return
    nl = "\r\n" if "\r\n" in text else "\n"
    legend = " ".join(
        f'<span class="inline-flex items-center gap-1.5 mr-4"><span style="background:{COLORS[l]}" class="inline-block w-3 h-3 rounded-sm"></span>{esc(l)}</span>'
        for l in LANGS
    )
    cards = []
    for row in rows:
        times = [row["times"][l] for l in LANGS if l in row["times"]]
        top = max(times)
        bars = []
        for l in LANGS:
            t = row["times"].get(l)
            if t is None:
                continue
            label = f"{t / 1000:.2f} s" if t >= 1000 else f"{t:.0f} ms"
            width = max(1.5, t / top * 100)
            bars.append(
                f'<div class="grid items-center gap-3 text-sm" style="grid-template-columns:8.5rem 1fr 5rem">'
                f'<span class="text-slate-600 dark:text-slate-300">{esc(l)}</span>'
                f'<div class="h-3 rounded bg-slate-100 dark:bg-slate-800 overflow-hidden"><div class="h-full rounded" style="width:{width:.1f}%;background:{COLORS[l]}"></div></div>'
                f'<span class="text-right tabular-nums">{label}</span></div>'
            )
        cards.append(
            f'<div class="rounded-xl border border-slate-200 dark:border-slate-800 p-4">'
            f'<div class="flex flex-wrap items-baseline justify-between gap-2"><h3 class="font-semibold"><code>{esc(row["bench"])}</code></h3>'
            f'<span class="text-sm text-slate-500">{esc(row["about"])}</span></div>'
            f'<div class="mt-3 grid gap-1.5">{"".join(bars)}</div></div>'
        )
    section = (
        f'{start} (generated by docs/tools/gen_bench.py) -->{nl}'
        f'    <section id="benchmarks" data-group="Performance" data-title="Benchmarks" class="scroll-mt-20 mt-16">{nl}'
        f'      <h2 class="text-2xl font-bold">Benchmarks: Fogu vs Bun vs Go vs Python</h2>{nl}'
        f'      <p class="mt-3">The same eight programs in Fogu, Python and JavaScript (Bun). Median of {r["runs"]} runs of the whole process, '
        f'start-up included, in milliseconds; lower is better. The languages are interleaved in every repetition, and the runner checks that the outputs are the same.</p>{nl}'
        f'      <p class="mt-2 text-sm text-slate-500">{esc(r["cpu"])} ({r["cores"]} logical CPUs) · {esc(r["os"])} · '
        f'{esc(" · ".join(r["versions"].values()))} · measured {date}</p>{nl}'
        f'      <div class="mt-5 text-sm">{legend}</div>{nl}'
        f'      <div class="mt-4 grid gap-5">{"".join(cards)}</div>{nl}'
        f'{before_after_html(rows)}{nl}'
        f'      {net_html(net)}{nl}'
        f'      <h3 class="text-lg font-semibold mt-8">Optimizations made</h3>{nl}'
        f'      <ul class="list-disc pl-6 mt-2 space-y-2">{"".join(f"<li><b>{esc(t)}.</b> {esc(d)}</li>" for t, d in CHANGES)}</ul>{nl}'
        f'      <p class="mt-4 text-sm">Fogu runs with its Cranelift JIT by default; <b>Fogu (no JIT)</b> is the interpreter alone. '
        f'Run them with <code>fogu bench/run.fg</code>, then <code>python docs/tools/gen_bench.py</code>. Details: <code>docs/benchmarks.md</code>.</p>{nl}'
        f'    </section>{nl}'
        f'    {end}'
    )
    i = text.index(start)
    j = text.index(end) + len(end)
    HTML.write_text(text[:i] + section + text[j:], encoding="utf-8", newline="")


if __name__ == "__main__":
    main()
