#!/usr/bin/env python3
"""Generates the standard library reference of docs/docs.html.

Every module is described in docs/tools/reference/*.py (a list SECTIONS of
dicts). Examples marked `run: True` are executed with the fogu binary and
their real output is shown under them, so the documentation cannot drift
from what the language does.

    python docs/tools/gen_reference.py              # uses target/release or target/debug
    python docs/tools/gen_reference.py --check      # only runs the examples

The generated HTML replaces everything between the markers
<!-- STDLIB REFERENCE: BEGIN --> and <!-- STDLIB REFERENCE: END -->.
The same reference is also written as Markdown to skills/fogu/stdlib.md, the
API guide that AI agents read (see skills/fogu/SKILL.md), and as data for the
docs site (docs/view/src/data/reference.generated.ts, rendered by RefSection).
"""

import html
import importlib.util
import re
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs" / "docs.html"
REF_DIR = pathlib.Path(__file__).resolve().parent / "reference"
BEGIN = "<!-- STDLIB REFERENCE: BEGIN -->"
END = "<!-- STDLIB REFERENCE: END -->"
STDLIB_MD = ROOT / "skills" / "fogu" / "stdlib.md"


def fogu_binary() -> pathlib.Path:
    exe = ".exe" if os.name == "nt" else ""
    found = [p for p in (ROOT / "target" / prof / f"fogu{exe}" for prof in ("release", "debug")) if p.exists()]
    if not found:
        sys.exit("build fogu first: cargo build")
    # the most recently built one
    return max(found, key=lambda p: p.stat().st_mtime)


def load_sections():
    sections = []
    for f in sorted(REF_DIR.glob("*.py")):
        spec = importlib.util.spec_from_file_location(f.stem, f)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        sections.extend(mod.SECTIONS)
    return sections


def run_example(binary, code, files):
    """Runs an example in a fresh folder; returns its output."""
    with tempfile.TemporaryDirectory(prefix="fogu-doc-") as tmp:
        for name, content in (files or {}).items():
            p = pathlib.Path(tmp) / name
            p.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, bytes):
                p.write_bytes(content)
            else:
                p.write_text(content, encoding="utf-8")
        main = pathlib.Path(tmp) / "example.fg"
        main.write_text(code.strip() + "\n", encoding="utf-8")
        env = dict(os.environ, NO_COLOR="1", FOGU_LOG="info")
        r = subprocess.run([str(binary), "run", "example.fg"], cwd=tmp, capture_output=True, timeout=120, env=env)
        out = (r.stdout + r.stderr).decode("utf-8", "replace").replace("\r\n", "\n").rstrip()
        if r.returncode != 0:
            raise RuntimeError(f"example failed ({r.returncode}):\n{code}\n--- output ---\n{out}")
        return out


def e(s):
    return html.escape(s, quote=False)


def render_api(api):
    rows = []
    for item in api:
        sig, ret, desc = item
        rows.append(
            '<tr class="align-top"><td class="py-2 pr-4 whitespace-nowrap"><code>%s</code></td>'
            '<td class="py-2 pr-4 text-slate-500 whitespace-nowrap">%s</td><td class="py-2">%s</td></tr>'
            % (e(sig), e(ret), desc)
        )
    return (
        '<div class="overflow-x-auto mt-4"><table class="w-full text-sm border-collapse">'
        '<thead><tr class="text-left border-b border-slate-200 dark:border-slate-800">'
        '<th class="py-2 pr-4">Function</th><th class="py-2 pr-4">Returns</th><th class="py-2">Description</th></tr></thead>'
        '<tbody class="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">'
        + "".join(rows)
        + "</tbody></table></div>"
    )


def render_section(binary, s, check_only):
    parts = []
    parts.append(
        '    <section id="%s" data-group="%s" data-title="%s" class="scroll-mt-20 mt-16">\n'
        % (s["id"], e(s["group"]), e(s["title"]))
    )
    parts.append('      <h2 class="text-2xl font-bold">%s</h2>\n' % s.get("heading", "<code>%s</code>" % e(s["title"])))
    if s.get("intro"):
        parts.append("      <p>%s</p>\n" % s["intro"])
    if s.get("import"):
        parts.append('      <script type="text/x-fogu" data-title="import">%s</script>\n' % s["import"])
    for block in s.get("blocks", []):
        if "h3" in block:
            parts.append('      <h3 class="text-lg font-semibold">%s</h3>\n' % block["h3"])
        if "text" in block:
            parts.append("      <p>%s</p>\n" % block["text"])
        if "api" in block:
            parts.append("      " + render_api(block["api"]) + "\n")
        if "example" in block:
            code = block["example"].strip("\n")
            title = block.get("title", "example.fg")
            lang = ' data-lang="%s"' % block["lang"] if block.get("lang") else ""
            parts.append('      <script type="text/x-fogu"%s data-title="%s">%s</script>\n' % (lang, e(title), code))
            if block.get("run"):
                out = run_example(binary, code, block.get("files"))
                block["_output"] = out
                if not check_only:
                    parts.append('      <script type="text/x-fogu" data-lang="text" data-title="output">%s</script>\n' % (out or "(no output)"))
            elif block.get("output"):
                parts.append(
                    '      <script type="text/x-fogu" data-lang="text" data-title="%s">%s</script>\n'
                    % (e(block.get("output_title", "output")), block["output"].strip("\n"))
                )
        if "note" in block:
            parts.append(
                '      <div class="mt-4 rounded-lg border border-ember-200 dark:border-ember-900 bg-ember-50/60 dark:bg-ember-900/10 px-4 py-3 text-sm">%s</div>\n'
                % block["note"]
            )
    parts.append("    </section>\n")
    return "".join(parts)


def md(text):
    """Inline HTML of the reference → Markdown."""
    text = re.sub(r"<code>(.*?)</code>", lambda m: "`" + html.unescape(m.group(1)) + "`", text, flags=re.S)
    text = re.sub(r"<strong>(.*?)</strong>", r"**\1**", text, flags=re.S)
    # links to other modules point to their Markdown headings (#mod-json → #fogujson)
    link = lambda m: "[%s](%s)" % (m.group(2), re.sub(r"^#mod-", "#fogu", m.group(1)))
    text = re.sub(r'<a\s[^>]*?href="([^"]+)"[^>]*>(.*?)</a>', link, text, flags=re.S)
    text = re.sub(r"</(tr|p|div|li|h\d)>", "\n", text)
    # tags (attribute values may contain '>', e.g. Tailwind's [&>tr]:…)
    text = re.sub(r"""<[^>"']*(?:"[^"]*"[^>"']*|'[^']*'[^>"']*)*>""", " ", text)
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r" *\n[ \n]*", "\n", text)
    return html.unescape(text).strip()


def render_section_md(s):
    parts = ["## %s\n" % s["title"]]
    if s.get("import"):
        parts.append("```fogu\n%s\n```\n" % s["import"])
    if s.get("intro"):
        parts.append(md(s["intro"]) + "\n")
    for block in s.get("blocks", []):
        if "h3" in block:
            parts.append("### %s\n" % md(block["h3"]))
        if "text" in block:
            parts.append(md(block["text"]) + "\n")
        if "api" in block:
            lines = []
            for sig, ret, desc in block["api"]:
                line = "- `%s`" % html.unescape(sig)
                if ret:
                    line += " → %s" % md(ret)
                if desc:
                    line += " — %s" % md(desc)
                lines.append(line)
            parts.append("\n".join(lines) + "\n")
        if "example" in block:
            lang = block.get("lang") or "fogu"
            parts.append("```%s\n%s\n```\n" % (lang, block["example"].strip("\n")))
            out = block.get("_output") if block.get("run") else block.get("output")
            if out:
                title = "output" if block.get("run") else block.get("output_title", "output")
                parts.append("%s:\n\n```text\n%s\n```\n" % (title, out.strip("\n")))
        if "note" in block:
            parts.append("> %s\n" % md(block["note"]))
    return "\n".join(parts)


def write_stdlib_md(sections):
    head = (
        "# Fogu standard library\n\n"
        "Generated from `docs/tools/reference/*.py` by `docs/tools/gen_reference.py` — do not edit.\n"
        "Outputs marked `output` were produced by running the example. Signatures read\n"
        "`call(args) → result — notes`; `value · error` means the call returns an `error`\n"
        "value on operational failures (use `?` or `const [v, err] = …`).\n\n"
    )
    toc = "\n".join("- [%s](#%s)" % (s["title"], re.sub(r"[^a-z0-9 -]", "", s["title"].lower()).replace(" ", "-")) for s in sections)
    body = "\n".join(render_section_md(s) for s in sections)
    STDLIB_MD.parent.mkdir(parents=True, exist_ok=True)
    STDLIB_MD.write_text(head + toc + "\n\n" + body, encoding="utf-8", newline="\n")
    print(f"wrote {len(sections)} sections to {STDLIB_MD.relative_to(ROOT)}")


VIEW_DATA = ROOT / "docs" / "view" / "src" / "data" / "reference.generated.ts"


def write_view_data(sections):
    """The same reference for the docs site (docs/view): rendered by RefSection.tsx."""
    import json

    def block(b):
        o = {k: b[k] for k in ("h3", "text", "lang", "title", "note") if k in b}
        if "api" in b:
            o["api"] = [list(row) for row in b["api"]]
        if "example" in b:
            o["example"] = b["example"].strip("\n")
            out = b.get("_output") if b.get("run") else b.get("output")
            if out:
                o["output"] = out.strip("\n")
                o["outputTitle"] = "output" if b.get("run") else b.get("output_title", "output")
        return o

    data = {
        s["id"]: {
            "id": s["id"],
            "group": s.get("group", ""),
            "title": s["title"],
            "import": s.get("import"),
            "intro": s.get("intro"),
            "blocks": [block(b) for b in s.get("blocks", [])],
        }
        for s in sections
    }
    body = (
        "// Generated from docs/tools/reference/*.py by docs/tools/gen_reference.py — do not edit.\n"
        "// Outputs of examples marked `run` were produced by running them.\n"
        "import type { RefData } from '../components/RefSection';\n\n"
        "export const REFERENCE: Record<string, RefData> = " + json.dumps(data, ensure_ascii=False, indent=1) + ";\n"
    )
    VIEW_DATA.write_text(body, encoding="utf-8", newline="\n")
    print(f"wrote {len(sections)} sections to {VIEW_DATA.relative_to(ROOT)}")


def main():
    check_only = "--check" in sys.argv
    binary = fogu_binary()
    sections = load_sections()
    out = []
    failures = 0
    for s in sections:
        try:
            out.append(render_section(binary, s, check_only))
            print(f"  ok  {s['title']}")
        except Exception as ex:  # noqa: BLE001
            failures += 1
            print(f"FAIL  {s['title']}\n{ex}\n")
    if failures:
        sys.exit(f"{failures} section(s) failed")
    if check_only:
        return
    text = DOCS.read_text(encoding="utf-8")
    if BEGIN not in text or END not in text:
        sys.exit(f"markers not found in {DOCS}")
    head, rest = text.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    DOCS.write_text(head + BEGIN + "\n" + "\n".join(out) + "    " + END + tail, encoding="utf-8", newline="\n")
    print(f"wrote {len(sections)} sections to {DOCS.relative_to(ROOT)}")
    write_stdlib_md(sections)
    write_view_data(sections)


if __name__ == "__main__":
    main()
