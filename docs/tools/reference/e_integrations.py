"""Standard library reference: mail, AI, SSH and shell."""

I = "Integrations"

SECTIONS = [
    {
        "id": "mod-mail",
        "group": I,
        "title": "fogu:mail",
        "import": 'import mail from "fogu:mail"',
        "intro": "E-mail over SMTP (presets for Gmail, Outlook, Office 365, Yahoo, iCloud, Zoho, Fastmail, AOL, GMX, "
        "Amazon SES, Mailtrap, Mailpit) or the HTTP APIs of Resend, SendGrid, Mailgun, Postmark, Brevo, MailerSend "
        "and Mailjet. The SMTP client and the MIME builder are Fogu's own (STARTTLS, implicit TLS, AUTH PLAIN/LOGIN, "
        "attachments, inline images, UTF-8 headers). The <code>console</code> provider only prints (development and tests).",
        "blocks": [
            {
                "api": [
                    ("mail.connect({ provider, … } | url)", "Mailer", "No connection is made yet."),
                    ("SMTP: { provider: \"gmail\" | \"smtp\" | …, host, port, secure, user, password, region }", "", "<code>secure</code>: <code>tls</code> (465), <code>starttls</code> (587), <code>none</code>; URLs <code>smtp://user:pass@host:587</code>, <code>smtps://…</code>."),
                    ("APIs: { provider: \"resend\" | \"sendgrid\" | …, apiKey, domain, region, baseUrl }", "", "<code>apiKey</code> defaults to <code>$RESEND_API_KEY</code>, <code>$SENDGRID_API_KEY</code>…; Mailgun needs <code>domain</code>, Mailjet <code>apiSecret</code>."),
                    ("common: { from, timeout }", "", "Default sender."),
                    ("mailer.send(message)", "{ id, provider } · error", "A rejected message, a bad address or a network failure is an error value."),
                    ("mailer.sendMany([messages])", "list · error", ""),
                    ("mailer.verify()", "bool · error", "SMTP: connects and authenticates."),
                    ("mailer.render(message)", "str", "The RFC 5322 text that SMTP would send."),
                    ("message: { from, to, cc, bcc, replyTo, subject, text, html, attachments, headers }", "", "Addresses: <code>\"a@x.com\"</code>, <code>\"Ana &lt;a@x.com&gt;\"</code>, <code>{ name, email }</code> or a list."),
                    ("attachment: { filename, content | path, contentType, cid, encoding }", "", "<code>content</code>: str or list of bytes; <code>cid</code> makes it inline (<code>&lt;img src=\"cid:logo\"&gt;</code>)."),
                ],
                "example": r'''
import mail from "fogu:mail"
const mailer = mail.connect({ provider: "console", from: "Shop <shop@example.com>" })?
const r = mailer.send({
    to: ["ana@example.com", { name: "Bob", email: "bob@example.com" }],
    subject: "Your order",
    text: "Thanks for buying!",
    attachments: [{ filename: "invoice.csv", content: "item,price\nbook,10\n" }],
})?
print(r)
const gmail = mail.connect({ provider: "gmail", user: "me@gmail.com", password: "app-password" })?
print(gmail.host, gmail.port, gmail.secure)
const [_, err] = mailer.send({ to: "not-an-address", subject: "x", text: "x" })
print(err)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-ai",
        "group": I,
        "title": "fogu:ai",
        "import": 'import ai from "fogu:ai"',
        "intro": "LLMs over HTTP: OpenAI-compatible APIs (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI, Together, "
        "Fireworks, Perplexity, Ollama, LM Studio), Anthropic and Google Gemini — chat, streaming, tools, agents, "
        "embeddings, and MCP: serve tools, resources and prompts to Claude and other clients, or use any MCP server's "
        "tools in an agent. Keys default to <code>$OPENAI_API_KEY</code>, <code>$ANTHROPIC_API_KEY</code>, <code>$GEMINI_API_KEY</code>…",
        "blocks": [
            {
                "h3": "Client",
                "api": [
                    ("ai.client({ provider, model, apiKey, baseUrl, timeout, headers, maxTokens } | \"provider/model\")", "AiClient", "Provider <code>compatible</code> + <code>baseUrl</code> for any OpenAI-style server."),
                    ("client.chat(prompt | { messages, prompt, system, model, temperature, maxTokens, topP, stop, tools, toolChoice, json })", "map · error", "<code>{ text, toolCalls, finishReason, usage: { inputTokens, outputTokens }, model, message }</code>."),
                    ("client.stream(same)", "AiStream · error", "<code>next(timeout?)</code> → the next piece of text or nil at the end; <code>collect()</code> → like <code>chat</code>; fields <code>text done result</code>."),
                    ("client.models()", "list · error", "<code>[{ id, name }]</code>."),
                    ("client.embed(text | [texts], { model })", "list · error", "Vectors (OpenAI-style and Gemini)."),
                    ("message: { role: \"system\" | \"user\" | \"assistant\" | \"tool\", content, toolCalls, toolCallId, name }", "", "<code>result.message</code> can be appended to continue the conversation."),
                ],
            },
            {
                "h3": "Tools and agents",
                "api": [
                    ("ai.tool(name, description, parameters, run?)", "map", "<code>parameters</code> is a JSON schema; also a plain map <code>{ name, description, parameters, run }</code>."),
                    ("ai.agent({ client, model, instructions, tools, maxSteps = 10, temperature, maxTokens, memory, onStep })", "Agent", "Tools need <code>run(args)</code>."),
                    ("agent.run(input)", "map · error", "Calls the model and the tools until it answers: <code>{ text, steps, messages, usage, finishReason }</code>."),
                    ("agent.reset() · agent.history", "", "With <code>memory: true</code> the conversation continues across runs."),
                ],
                "text": "A tool that returns an <code>error</code> value sends <code>error: …</code> back to the model, which can recover.",
                "example": r'''
import ai from "fogu:ai"
const llm = ai.client({ provider: "anthropic" })?          // $ANTHROPIC_API_KEY
print(llm.chat("Say hi in Portuguese, one word")?.text)

const s = llm.stream("Count to 3")?
while true { const t = s.next()?; if t == nil { break }; Fogu.write(t) }
print()

const weather = ai.tool("weather", "Current temperature of a city",
    { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
    (args) => ({ city: args.city, celsius: 21 }))
const agent = ai.agent({ client: llm, instructions: "Use the tools", tools: [weather] })
const answer = agent.run("Is it warm in Lisbon?")?
print(answer.text)
print(answer.steps[0].toolCalls[0].name, answer.steps[0].toolCalls[0].arguments)
''',
                "output": "Olá\n1, 2, 3\nYes — it is 21 °C in Lisbon, a pleasant temperature.\nweather {city: Lisbon}",
                "output_title": "output (the model's words vary)",
            },
            {
                "h3": "MCP servers and clients",
                "api": [
                    ("ai.mcpServer({ name, version, title, instructions })", "McpServer", "A <a class=\"underline\" href=\"https://modelcontextprotocol.io\">Model Context Protocol</a> server (protocol 2025-06-18, also 2025-03-26 and 2024-11-05) that Claude Desktop, Claude Code, IDEs and agents can use."),
                    ("server.tool(ai.tool(…)) · server.tool(name, description, schema, run)", "McpServer", "A tool; <code>run(args)</code> returns a value (sent as text, maps also as <code>structuredContent</code>) or an <code>error</code> (sent as <code>isError</code>). A runtime error in a tool is reported to the client, the server keeps running."),
                    ("server.resource(uri, { name, description, mimeType }, fn() → text) · server.prompt(name, { description, arguments: [{ name, required }] }, fn(args) → text | [{ role, text }])", "McpServer", ""),
                    ("server.serveStdio()", "nil", "Serves on stdin/stdout until stdin closes — the transport of local servers. Do not <code>print</code> anything else in this mode (use <code>log</code> on stderr)."),
                    ("server.listen(port, { path = \"/mcp\", host = \"127.0.0.1\", token, origins })", "int · error", "Streamable HTTP (JSON answers). <code>token</code> requires <code>Authorization: Bearer</code>; requests from browsers are only accepted from <code>localhost</code> or <code>origins</code>."),
                    ("server.handle(message)", "map · list · nil", "One JSON-RPC message (str or map) → the answer: tests, or another transport."),
                    ("ai.mcpClient({ command, args, env, cwd, stderr } | { url, headers }, { timeout = 30s })", "McpClient · error", "Starts a local server (stdio) or connects to a remote one (Streamable HTTP, JSON or SSE answers); fields <code>serverInfo protocolVersion capabilities instructions closed</code>."),
                    ("mcp.listTools() · callTool(name, args) · listResources() · readResource(uri) · listPrompts() · getPrompt(name, args) · ping() · close()", "", "<code>callTool</code> → the structured result, else the text; a tool's <code>isError</code> is an error value."),
                    ("mcp.tools()", "list", "The server's tools as <code>ai.tool</code> values: <code>ai.agent({ client, tools: mcp.tools()? })</code> lets a model use any MCP server."),
                ],
                "example": r'''
import ai from "fogu:ai"
import json from "fogu:json"

const mcp = ai.mcpServer({ name: "notes", version: "1.0.0" })
mcp.tool("add", "Adds two numbers", { type: "object", properties: { a: { type: "number" }, b: { type: "number" } } },
    (args) => args.a + args.b)
mcp.resource("notes://today", { name: "Today" }, () => "- ship MCP")

// what a client sends (mcp.serveStdio() or mcp.listen(8000) serve it for real)
const call = (method, params) => mcp.handle({ jsonrpc: "2.0", id: 1, method: method, params: params })
print(call("initialize", { protocolVersion: "2025-06-18" }).result.serverInfo)
print(json.stringify(call("tools/call", { name: "add", arguments: { a: 2, b: 3 } }).result.content))
print(call("resources/read", { uri: "notes://today" }).result.contents[0].text)
''',
                "run": True,
            },
        ],
    },
    {
        "id": "mod-ssh",
        "group": I,
        "title": "fogu:ssh",
        "import": 'import ssh from "fogu:ssh"',
        "intro": "Commands, file copies and tunnels over the system's OpenSSH client (key or password).",
        "blocks": [
            {
                "api": [
                    ("ssh.connect(\"user@host:port\" | { host, user, port, key, password, strictHostKey, knownHosts, timeout, options, jump })", "Ssh · error", "Checks the connection; <code>strictHostKey</code> defaults to <code>accept-new</code>; <code>jump</code> (another connection or <code>\"user@host\"</code>) goes through a bastion with that connection's key and settings."),
                    ("s.run(command, { input })", "{ ok, code, stdout, stderr } · error", "A non-zero exit code is not an error."),
                    ("s.upload(local, remote) · s.download(remote, local)", "nil · error", "scp, folders included."),
                    ("s.forward(localPort | 0, host, port)", "Tunnel · error", "<code>-L</code>: a local port that reaches <code>host:port</code> through the server."),
                    ("s.reverse(remotePort, host, localPort)", "Tunnel · error", "<code>-R</code>: a port on the server that reaches a local service."),
                    ("tunnel.localPort · .remotePort · .alive() · .close()", "", ""),
                    ("s.mount(remote, local, { readonly, keep, options, tunnel, jump })", "Mount · error", "Like sshfs: the remote folder appears as a local folder. Uses the connection's user, port, key (or password, sent on stdin), <code>knownHosts</code> and <code>strict</code>; <code>options</code> are extra <code>-o</code> sshfs options. Needs <code>sshfs</code> (Linux: <code>apt install sshfs</code>; macOS: macFUSE + sshfs; or <code>$FOGU_SSHFS</code>). <code>tunnel</code>: through an open <code>s.forward(…)</code>; <code>jump</code>: through a bastion. Unmounted when the program ends — also on Ctrl+C/SIGTERM — unless <code>keep</code>. Windows: SSHFS-Win (+ WinFsp), the local side is a drive letter (<code>\"X:\"</code>), only <code>%USERPROFILE%\\.ssh\\id_rsa</code> or a password, no <code>readonly</code>."),
                    ("mount.path · .remote · .host · .mounted · .unmount()", "", "A missing remote folder, a refused key or a busy mount point is an error value."),
                    ("s.shell({ pty, shell = \"sh\", cwd, env })", "SshShell · error", "One persistent remote shell: <code>sh.run(cmd, { timeout })</code> → <code>{ ok, code, stdout, stderr }</code> keeps the working directory and variables between commands (one command at a time per shell)."),
                    ("sh.send(text) · sh.expect(text | regex, { timeout = 30s }) · sh.read({ timeout = 200ms }) · sh.alive() · sh.close()", "", "Expect-style control of programs that ask questions; with <code>pty: true</code> the remote side gets a terminal (sudo prompts…)."),
                    ("s.interactive(command?)", "int · error", "Hands the user's terminal to the remote shell (<code>ssh -t</code>); returns its exit code."),
                    ("FOGU_SSH", "", "The ssh client to use (another build, a wrapper)."),
                    ("s.close()", "nil", ""),
                ],
                "example": r'''
import ssh from "fogu:ssh"
import fs from "fogu:fs"
const s = ssh.connect("deploy@server.example.com:22", { key: "~/.ssh/id_ed25519" })?
print(s.run("uname -s")?.stdout.trim())
s.upload("dist/app", "/srv/app")?
const db = s.forward(0, "127.0.0.1", 5432)?          // the server's PostgreSQL, locally
print("postgres://app@127.0.0.1:" + str(db.localPort) + "/app")
db.close()
const www = s.mount("/var/www", "./www", { readonly: true })?   // a local folder backed by the server
print(fs.list("./www")?)
www.unmount()?
s.close()
''',
                "output": "Linux\npostgres://app@127.0.0.1:53110/app\n[index.html, static]",
                "output_title": "output (with an SSH server)",
            },
            {
                "example": r'''
import ssh from "fogu:ssh"
const s = ssh.connect("deploy@server.example.com")?
const sh = s.shell({ cwd: "/srv/app", env: { APP_ENV: "prod" } })?
sh.run("git pull -q && export VERSION=$(git rev-parse --short HEAD)")?
print(sh.run("echo $APP_ENV $VERSION; pwd")?.stdout)
const t = s.shell({ pty: true })?
t.send("sudo -k; sudo systemctl restart app\n")
t.expect("password")?
t.send(Fogu.env.SUDO_PASS + "\n")
print(t.expect("\\$ ")?.contains("restart"))
''',
                "output": "prod 3f9c2a1\n/srv/app\n\ntrue",
                "output_title": "output (with an SSH server)",
            }
        ],
    },
    {
        "id": "mod-shell",
        "group": I,
        "title": "fogu:shell",
        "import": 'import shell from "fogu:shell"',
        "intro": "A small POSIX-style shell: pipes, <code>&amp;&amp;</code> <code>||</code> <code>;</code>, redirections "
        "(<code>&gt; &gt;&gt; &lt; 2&gt; 2&gt;&amp;1 &amp;&gt;</code>), <code>$VAR</code>, <code>$(…)</code>, quotes, globs and "
        "<code>~</code>. The usual Linux commands are built in, so scripts behave the same on Windows: <code>echo cat ls "
        "pwd cd mkdir rm cp mv touch grep head tail wc sort uniq cut tr tee basename dirname which env export true "
        "false exit sleep seq</code>. Other commands run as programs.",
        "blocks": [
            {
                "api": [
                    ("shell.run(command, { cwd, env, input, native = true })", "{ ok, code, stdout, stderr }", "A non-zero exit is not an error; <code>native: false</code> uses system programs only."),
                    ("shell.exec(command, opts?)", "same", "Output goes to the terminal as it happens."),
                    ("shell.text(command, opts?)", "str · error", "stdout without the final newline; a failure is an error value."),
                    ("shell.lines(command, opts?) · shell.json(command, opts?)", "list · value", ""),
                    ("shell.quote(text)", "str", "Safe to interpolate into a command."),
                    ("shell.cd(dir) · shell.cwd()", "nil · str", "The shell's own working folder."),
                    ("shell.which(program) · shell.builtins", "str · list", ""),
                ],
                "example": r'''
import shell from "fogu:shell"
shell.run("mkdir -p data && echo banana > data/fruits.txt && echo apple >> data/fruits.txt")?
print(shell.text("cat data/fruits.txt | sort | head -n 1")?)
print(shell.lines("ls data")?, shell.text("cat data/*.txt | wc -l")?)
const name = "file with spaces.txt"
shell.run(`echo ok > data/${shell.quote(name)}`)?
print(shell.text("echo \"$WHO has $(cat data/fruits.txt | wc -l) fruits\"", { env: { WHO: "Ana" } })?)
const r = shell.run("grep kiwi data/fruits.txt || echo 'no kiwi'")?
print(r.ok, r.stdout.trim())
const [_, err] = shell.text("cat missing.txt")
print(err)
''',
                "run": True,
            }
        ],
    },
]
