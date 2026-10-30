"""Standard library reference: HTML, browsers, images and charts."""

W = "HTML & browser"
M = "Images & charts"

SECTIONS = [
    {
        "id": "mod-html",
        "group": W,
        "title": "fogu:html",
        "import": 'import html from "fogu:html"',
        "intro": "Parse HTML and query it with CSS selectors (scraping), render Jinja templates (minijinja), sanitize "
        "untrusted HTML (ammonia) and convert between HTML, plain text and Markdown. An invalid selector stops the "
        "program; template errors are error values.",
        "blocks": [
            {
                "h3": "Parsing and selectors",
                "api": [
                    ("html.parse(text) · html.fragment(text)", "HtmlDocument", "A full document or a fragment."),
                    ("doc.select(css) · doc.find(css)", "list · HtmlElement | nil", "All matches · the first one."),
                    ("doc.title · doc.text · doc.html · doc.root", "", ""),
                    ("el.tag · el.text · el.html · el.outerHtml · el.id · el.classes · el.attrs · el.children · el.parent", "", "<code>text</code> has whitespace collapsed."),
                    ("el.attr(name) · el.hasClass(c) · el.select(css) · el.find(css) · el.closest(css) · el.matches(css)", "", ""),
                    ("html.select(text, css)", "list", "Parse + select in one call."),
                ],
                "example": r'''
import html from "fogu:html"
import http from "fogu:http"
const doc = html.parse("<ul id='news'><li><a href='/a'>Fogu 1.0</a> <b>new</b></li><li><a href='/b'>JIT notes</a></li></ul>")
for a in doc.select("#news li > a") {
    print(a.text, "→", a.attr("href"))
}
print(doc.find("b").closest("li").find("a").text, doc.find(".missing"))
// scraping a real page: html.parse(http.get(url)?.text)
''',
                "run": True,
            },
            {
                "h3": "Templates (Jinja)",
                "api": [
                    ("html.render(source, data)", "str · error", "Autoescaped; <code>{{ x | markdown }}</code> and <code>| striptags</code> are added to the Jinja filters."),
                    ("html.template(source, { autoescape = true })", "Template", "<code>t.render(data)</code>."),
                    ("html.templates(folder, { autoescape })", "Templates", "<code>render(name, data)</code>, <code>renderString(source, data)</code>, <code>has(name)</code>; <code>extends</code>/<code>include</code>/<code>import</code> load from the folder; autoescape by extension."),
                ],
                "example": r'''
import html from "fogu:html"
const page = html.render("<h1>{{ title }}</h1>{% for p in items %}<li>{{ loop.index }}. {{ p.name | upper }}</li>{% endfor %}", {
    title: "Tom & Jerry",
    items: [{ name: "café" }, { name: "pão" }],
})?
print(page)
const [_, err] = html.render("{% if %}", {})
print(err)
''',
                "run": True,
            },
            {
                "h3": "Sanitizing and conversions",
                "api": [
                    ("html.escape(text) · html.unescape(text)", "str", "Entities: named, decimal and hex."),
                    ("html.sanitize(text, { tags, addTags, removeTags, attributes, stripComments })", "str", "Removes scripts, event handlers and dangerous URLs."),
                    ("html.text(text) · el.toText()", "str", "Plain text with paragraphs, bullets and tab-separated cells."),
                    ("html.toMarkdown(text) · el.toMarkdown()", "str", "Headings, emphasis, links, images, lists, code blocks, quotes, tables."),
                    ("html.fromMarkdown(md)", "str", "CommonMark + tables, strikethrough, task lists, footnotes."),
                ],
                "example": r'''
import html from "fogu:html"
print(html.sanitize("<p onclick='steal()'>hi<script>alert(1)</script></p>"))
print(html.toMarkdown("<h2>Notes</h2><p>Some <b>bold</b> and <a href='https://fogu.dev'>a link</a></p><ul><li>one</li><li>two</li></ul>"))
print(html.fromMarkdown("**fogu** is *fun*"))
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-webview",
        "group": W,
        "title": "fogu:webview",
        "import": 'import webview from "fogu:webview"',
        "intro": "Drives a real browser (Chrome, Edge, Chromium or Brave) through the DevTools protocol: render "
        "JavaScript-heavy pages, click, type, run scripts, take screenshots and PDFs. The browser is found in "
        "<code>FOGU_CHROME</code>, the <code>path</code> option, the PATH or the usual install folders. Every command "
        "runs on a worker thread; the browser processes are closed when the program ends.",
        "blocks": [
            {
                "api": [
                    ("webview.launch({ headless = true, path, args, width = 1280, height = 800, userAgent, timeout = 30s })", "Browser · error", ""),
                    ("webview.render(url, { wait, timeout })", "str · error", "The HTML after the page's JavaScript ran (shared headless browser)."),
                    ("webview.find()", "str | nil", "The browser that would be used."),
                    ("browser.open(url?, { wait = \"load\" | \"domcontentloaded\" | \"networkidle\" | \"none\", timeout })", "Page · error", "A new tab."),
                    ("browser.version() · browser.close()", "", ""),
                    ("page.goto(url, opts) · reload() · back() · forward()", "nil · error", ""),
                    ("page.title() · url() · content() · text(css?)", "str · error", ""),
                    ("page.eval(js, ...args)", "value · error", "An expression (promises are awaited) or a function called with JSON arguments."),
                    ("page.click(css) · type(css, text) · fill(css, value) · press(key)", "nil · error", "Real mouse/keyboard events; <code>press(\"Enter\")</code>."),
                    ("page.waitFor(css, timeout) · waitForNavigation(opts)", "nil · error", ""),
                    ("page.select(css)", "list · error", "<code>[{ tag, text, html, attrs }]</code> from the live DOM."),
                    ("page.setContent(html) · setViewport(w, h) · setHeaders(map)", "nil · error", ""),
                    ("page.screenshot({ fullPage, element })", "Image · error", "A <a href=\"#mod-image\">fogu:image</a> image."),
                    ("page.pdf(path?, { format = \"A4\", landscape, background = true, scale })", "nil | bytes · error", ""),
                    ("page.cookies() · setCookie({ name, value, url | domain })", "", ""),
                    ("page.close()", "nil", ""),
                ],
                "example": r'''
import webview from "fogu:webview"
const browser = webview.launch()?
const page = browser.open("https://example.com")?
print(page.title()?)
print(page.eval("document.querySelectorAll('a').length")?)
page.screenshot({ fullPage: true })?.save("example.png")?
page.pdf("example.pdf")?
browser.close()
''',
                "output": "Example Domain\n1",
                "output_title": "output (with Chrome installed)",
            }
        ],
    },
    {
        "id": "mod-image",
        "group": M,
        "title": "fogu:image",
        "import": 'import image from "fogu:image"',
        "intro": "Create, transform, draw on and encode images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF), render SVG "
        "(resvg) and HTML, and build animated GIFs. Transformations return a <strong>new</strong> image; drawing "
        "methods change the image in place. Colors: <code>\"#rgb\"</code>, <code>\"#rrggbb(aa)\"</code>, CSS names, "
        "<code>\"rgb(r,g,b)\"</code>, <code>\"rgba(r,g,b,a)\"</code> or <code>[r, g, b, a?]</code>. Encoded data comes back as a <a class=\"underline\" href=\"#values\">buffer</a>; inputs also take a list of byte ints, base64 or a <code>data:</code> URL.",
        "blocks": [
            {
                "api": [
                    ("image.new(w, h, background?) · image.open(path) · image.decode(buffer | bytes | base64 | dataUrl)", "Image · error", ""),
                    ("image.fromSvg(svg, { width, height, scale, background })", "Image · error", "Text uses the system fonts."),
                    ("image.fromHtml(html, { width = 800, height = 600, fullPage = true })", "Image · error", "Rendered by a headless Chrome/Edge (<a href=\"#mod-webview\">fogu:webview</a>)."),
                    ("image.info(path)", "{ width, height, format } · error", "Without decoding the pixels."),
                    ("image.gif(frames, { delay = 100ms, loop = 0, speed }) · image.writeGif(path, frames, opts)", "buffer · nil", "Frames: images or <code>{ image, delay }</code>."),
                    ("image.frames(path | buffer)", "list · error", "<code>[{ image, delay }]</code> of an animated GIF."),
                    ("image.color(c)", "list", "<code>[r, g, b, a]</code>."),
                    ("img.width · img.height", "int", ""),
                    ("img.resize(w, h?, { fit = \"fill\" | \"contain\" | \"cover\", filter }) · thumbnail(w, h?) · crop(x, y, w, h)", "Image", "One side keeps the aspect ratio."),
                    ("img.rotate(degrees, { background }) · flipH() · flipV()", "Image", "Any angle; 90/180/270 are exact."),
                    ("img.grayscale() · invert() · blur(sigma) · sharpen(sigma, threshold)", "Image", ""),
                    ("img.brightness(d) · contrast(c) · hue(deg) · saturate(f) · opacity(f)", "Image", ""),
                    ("img.fill(c) · setPixel(x, y, c) · getPixel(x, y)", "nil · str", ""),
                    ("img.rect(x, y, w, h, { fill, stroke, width, radius }) · circle(cx, cy, r, opts) · ellipse(cx, cy, rx, ry, opts)", "nil", "Anti-aliased (tiny-skia)."),
                    ("img.line(x1, y1, x2, y2, { color, width }) · polygon([[x, y], …], { fill, stroke, closed })", "nil", ""),
                    ("img.text(str, x, y, { size = 16, color, font, bold, align }) · measureText(str, opts)", "nil · { width, height }", "<code>font</code>: a .ttf/.otf path or a family (<code>\"sans-serif\"</code>, <code>\"monospace\"</code>…)."),
                    ("img.paste(other, x, y, { opacity }) · drawSvg(svg, x, y, opts)", "nil", "Alpha-blended."),
                    ("img.save(path, { quality, background }) · encode(format) · toBase64(format) · toDataUrl(format)", "", "Format from the extension; JPEG flattens alpha over white. <code>encode</code> → buffer."),
                ],
                "example": r'''
import image from "fogu:image"
const card = image.new(320, 160, "#1e293b")
card.rect(16, 16, 96, 96, { fill: "#f97316", radius: 16 })
card.circle(250, 64, 40, { stroke: "white", width: 4 })
card.text("Fogu", 130, 110, { size: 28, color: "white", bold: true })
card.save("card.png")?
print(card.width, card.height, card.getPixel(30, 30))
const thumb = card.resize(160).grayscale()
print(thumb.width, thumb.height, image.info("card.png")?)
const frames = []
for i in 0..4 {
    const f = image.new(32, 32, "white")
    f.circle(16, 16, 4 + i * 3, { fill: "#f97316" })
    frames.push(f)
}
image.writeGif("pulse.gif", frames, { delay: 80ms })?
print(len(image.frames("pulse.gif")?))
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-plot",
        "group": M,
        "title": "fogu:plot",
        "import": 'import plot from "fogu:plot"',
        "intro": "Charts in the style of matplotlib — line, scatter, area, bar (grouped), histogram and pie/donut — drawn "
        "as SVG and saved as SVG, PNG, JPEG or WebP. Works with lists and ranges (arrays and data frames from the <code>datascience</code> lib: "
        "pass <code>a.toList()</code> / <code>df.col(name)</code>).",
        "blocks": [
            {
                "api": [
                    ("plot.figure({ title, xlabel, ylabel, width = 800, height = 450, theme = \"light\" | \"dark\", grid = true, legend = true, xmin, xmax, ymin, ymax, background })", "Figure", ""),
                    ("fig.line(x?, y, { label, color, width, dash, marker, size, opacity }) · scatter(…) · area(…)", "nil", "<code>line(y)</code> uses 0, 1, 2… as x."),
                    ("fig.bar(categories, values, { label, color })", "nil", "Several <code>bar</code> calls are grouped."),
                    ("fig.hist(values, { bins = 10 }) · fig.pie(labels, values, { donut })", "nil", ""),
                    ("fig.hline(y, { color }) · vline(x) · text(x, y, str) · title(t) · xlabel(t) · ylabel(t)", "nil", ""),
                    ("fig.svg() · fig.image({ scale }) · fig.save(path, { scale = 2, quality })", "str · Image · nil", ""),
                    ("plot.line(x, y, options) · plot.bar · plot.scatter · plot.area · plot.hist · plot.pie", "Figure", "One-series shortcuts (options also take the figure options)."),
                ],
                "example": r'''
import plot from "fogu:plot"
const fig = plot.figure({ title: "Sales", xlabel: "month", ylabel: "€" })
fig.area([1, 2, 3, 4, 5, 6], [10, 14, 9, 17, 21, 19], { label: "2025" })
fig.line([1, 2, 3, 4, 5, 6], [8, 11, 13, 15, 14, 22], { label: "2026", dash: true, marker: true })
fig.save("sales.png")?
const langs = plot.figure({ title: "Languages", theme: "dark" })
langs.bar(["Fogu", "Rust", "Go"], [42, 30, 25], { label: "2025" })
langs.bar(["Fogu", "Rust", "Go"], [55, 33, 22], { label: "2026" })
langs.save("langs.svg")?
print(plot.pie(["A", "B"], [70, 30], { donut: true }).svg().starts_with("<svg"))
''',
                "run": True,
            },
        ],
    },
]
