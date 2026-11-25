#!/usr/bin/env python3
"""Writes the "Grammar" section of docs/docs.html from docs/grammar.md.

    python docs/tools/gen_grammar.py           # rewrite the section
    python docs/tools/gen_grammar.py --check   # run every `fogu` example through `fogu check`

docs/grammar.md is the source (it is also read as it is on GitHub). The section
sits between the GRAMMAR markers of docs.html; the site serves it at
/docs/grammar. Markdown understood here: `##` / `###` headings, paragraphs,
`-` and `1.` lists, fenced code blocks, pipe tables, `code`, **bold**, *italics*
and [links](url).
"""
import html
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCE = os.path.join(ROOT, "docs", "grammar.md")
TARGET = os.path.join(ROOT, "docs", "docs.html")
BEGIN = "<!-- GRAMMAR: BEGIN -->"
END = "<!-- GRAMMAR: END -->"


def inline(text):
    parts = re.split(r"(`[^`]*`)", text)
    out = []
    for part in parts:
        if len(part) >= 2 and part.startswith("`") and part.endswith("`"):
            out.append("<code>" + html.escape(part[1:-1], quote=False) + "</code>")
            continue
        t = html.escape(part, quote=False)
        t = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", t)
        t = re.sub(r"(?<![\w*])\*([^*\s][^*]*)\*(?![\w*])", r"<em>\1</em>", t)
        t = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", lambda m: '<a class="underline" href="%s">%s</a>' % (m.group(2), m.group(1)), t)
        out.append(t)
    return "".join(out)


def slug(text):
    return "grammar-" + re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def blocks(lines):
    """Yields (kind, payload) for each block of the Markdown source."""
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        m = re.match(r"^```(\w*)\s*$", line)
        if m:
            j = i + 1
            while j < len(lines) and not lines[j].startswith("```"):
                j += 1
            yield "code", (m.group(1) or "text", "\n".join(lines[i + 1:j]))
            i = j + 1
            continue
        m = re.match(r"^(#{1,4})\s+(.*)$", line)
        if m:
            yield "heading", (len(m.group(1)), m.group(2).strip())
            i += 1
            continue
        if line.lstrip().startswith("|"):
            j = i
            while j < len(lines) and lines[j].lstrip().startswith("|"):
                j += 1
            yield "table", lines[i:j]
            i = j
            continue
        m = re.match(r"^(\s*)([-*]|\d+\.)\s+", line)
        if m:
            ordered = m.group(2)[0].isdigit()
            items = []
            j = i
            while j < len(lines) and lines[j].strip():
                m2 = re.match(r"^\s*([-*]|\d+\.)\s+(.*)$", lines[j])
                if m2 and (lines[j][0] not in " \t" or not items):
                    items.append(m2.group(2))
                elif m2 and len(lines[j]) - len(lines[j].lstrip()) < 2:
                    items.append(m2.group(2))
                else:
                    items[-1] += " " + lines[j].strip()
                j += 1
            yield "list", (ordered, items)
            i = j
            continue
        j = i
        while j < len(lines) and lines[j].strip() and not re.match(r"^(```|#{1,4}\s|\s*\|)", lines[j]):
            j += 1
        yield "para", " ".join(l.strip() for l in lines[i:j])
        i = j


def cells(row):
    row = row.strip()
    if row.startswith("|"):
        row = row[1:]
    if row.endswith("|") and not row.endswith("\\|"):
        row = row[:-1]
    # (a pipe inside a cell is written \|)
    return [c.strip().replace("\\|", "|") for c in re.split(r"(?<!\\)\|", row)]


def render(md):
    out = []
    title = "Grammar"
    for kind, data in blocks(md.splitlines()):
        if kind == "heading":
            level, text = data
            if level == 1:
                title = text
                continue
            tag = "h3" if level == 2 else "h4"
            cls = "text-lg font-semibold mt-8" if level == 2 else "font-semibold mt-6"
            out.append('      <%s id="%s" class="%s">%s</%s>' % (tag, slug(text), cls, inline(text), tag))
        elif kind == "para":
            out.append('      <p class="mt-3">%s</p>' % inline(data))
        elif kind == "list":
            ordered, items = data
            tag = "ol" if ordered else "ul"
            cls = "list-decimal" if ordered else "list-disc"
            out.append('      <%s class="%s pl-6 mt-3 space-y-1">' % (tag, cls))
            out.extend("        <li>%s</li>" % inline(item) for item in items)
            out.append("      </%s>" % tag)
        elif kind == "code":
            lang, code = data
            # (inside a <script> element only its own end tag needs care)
            code = code.replace("</script", "<\\/script")
            attr = "" if lang == "fogu" else ' data-lang="%s"' % html.escape(lang)
            title_attr = ' data-title="EBNF"' if lang == "ebnf" else ""
            out.append('      <script type="text/x-fogu"%s%s>%s</script>' % (attr, title_attr, code))
        elif kind == "table":
            rows = [cells(r) for r in data if not re.match(r"^\s*\|[\s:|-]+\|\s*$", r)]
            out.append('      <div class="overflow-x-auto mt-4">')
            out.append('        <table class="w-full text-sm border-collapse">')
            head = "".join('<th class="py-2 pr-4">%s</th>' % inline(c) for c in rows[0])
            out.append('          <thead><tr class="text-left border-b border-slate-200 dark:border-slate-800">%s</tr></thead>' % head)
            out.append('          <tbody class="[&>tr]:border-b [&>tr]:border-slate-100 dark:[&>tr]:border-slate-800/60">')
            for row in rows[1:]:
                out.append("            <tr>%s</tr>" % "".join('<td class="py-2 pr-4">%s</td>' % inline(c) for c in row))
            out.append("          </tbody>")
            out.append("        </table>")
            out.append("      </div>")
    head = [
        '    <section id="grammar" data-group="Language" data-title="Grammar" class="scroll-mt-20 mt-16">',
        '      <h2 class="text-2xl font-bold">%s</h2>' % inline(title),
    ]
    return "\n".join(head + out + ["    </section>"])


def examples(md):
    return [code for kind, (lang, code) in ((k, d) for k, d in blocks(md.splitlines()) if k == "code") if lang == "fogu"]


def check(md):
    fogu = os.environ.get("FOGU") or os.path.join(ROOT, "target", "release", "fogu")
    failed = 0
    codes = examples(md)
    with tempfile.TemporaryDirectory() as tmp:
        for n, code in enumerate(codes, 1):
            path = os.path.join(tmp, "example%d.fg" % n)
            with open(path, "w", encoding="utf-8") as f:
                f.write(code + "\n")
            r = subprocess.run([fogu, "check", path], capture_output=True, text=True)
            if r.returncode != 0:
                failed += 1
                print("example %d does not pass `fogu check`:\n%s\n%s" % (n, code, r.stdout + r.stderr))
    print("%d examples, %d failed" % (len(codes), failed))
    return failed == 0


def main():
    with open(SOURCE, encoding="utf-8", newline="") as f:
        md = f.read().replace("\r\n", "\n")
    if "--check" in sys.argv:
        sys.exit(0 if check(md) else 1)
    with open(TARGET, encoding="utf-8", newline="") as f:
        page = f.read()
    newline = "\r\n" if "\r\n" in page else "\n"
    a, b = page.find(BEGIN), page.find(END)
    if a < 0 or b < 0:
        sys.exit("docs.html has no GRAMMAR markers")
    section = (BEGIN + "\n" + render(md) + "\n    ").replace("\n", newline)
    page = page[:a] + section + page[b:]
    with open(TARGET, "w", encoding="utf-8", newline="") as f:
        f.write(page)
    print("wrote the grammar section (%d examples) to docs/docs.html" % len(examples(md)))


if __name__ == "__main__":
    main()
