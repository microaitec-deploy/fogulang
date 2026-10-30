import React from 'react';
import { DocCodeBlock } from '../DocCodeBlock';
import { RefSection } from '../RefSection';

interface Props {
  onRun: (code: string) => void;
}

export const IntegrationsSection: React.FC<Props> = ({ onRun }) => {
  return (
    <>
      {/* ================= FOGU:MAIL ================= */}
      <RefSection id="mod-mail" onRun={onRun} />

      {/* ================= FOGU:AI ================= */}
      <section id="mod-ai" data-group="Integrations" data-title="fogu:ai" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:ai</code></h2>
        <p>LLMs over HTTP: OpenAI-compatible APIs (OpenAI, Groq, OpenRouter, DeepSeek, Mistral, xAI, Together, Fireworks, Perplexity, Ollama, LM Studio), Anthropic and Google Gemini — chat, streaming, tools, agents, embeddings. Keys default to <code>$OPENAI_API_KEY</code>, <code>$ANTHROPIC_API_KEY</code>, <code>$GEMINI_API_KEY</code>…</p>
        <DocCodeBlock lang="fogu" title="import" code={`import ai from "fogu:ai"`} />
        
        <h3 className="text-lg font-semibold mt-6">Client</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ai.client({`{ provider, model, apiKey, baseUrl, timeout, headers, maxTokens }`} | "provider/model")</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">AiClient</td><td className="py-2">Provider <code>compatible</code> + <code>baseUrl</code> for any OpenAI-style server.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>client.chat(prompt | {`{ messages, prompt, system, model, temperature, maxTokens, topP, stop, tools, toolChoice, json }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>{`{ text, toolCalls, finishReason, usage: { inputTokens, outputTokens }, model, message }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>client.stream(same)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">AiStream · error</td><td className="py-2"><code>next(timeout?)</code> → the next piece of text or nil at the end; <code>collect()</code> → like <code>chat</code>; fields <code>text done result</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>client.models()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2"><code>[{`{ id, name }`}]</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>client.embed(text | [texts], {`{ model }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2">Vectors (OpenAI-style and Gemini).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>message: {`{ role: "system" | "user" | "assistant" | "tool", content, toolCalls, toolCallId, name }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>result.message</code> can be appended to continue the conversation.</td></tr>
            </tbody>
          </table>
        </div>

        <h3 className="text-lg font-semibold mt-6">Tools and agents</h3>
        <p>A tool that returns an <code>error</code> value sends <code>error: …</code> back to the model, which can recover.</p>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ai.tool(name, description, parameters, run?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map</td><td className="py-2"><code>parameters</code> is a JSON schema; also a plain map <code>{`{ name, description, parameters, run }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ai.agent({`{ client, model, instructions, tools, maxSteps = 10, temperature, maxTokens, memory, onStep }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Agent</td><td className="py-2">Tools need <code>run(args)</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>agent.run(input)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2">Calls the model and the tools until it answers: <code>{`{ text, steps, messages, usage, finishReason }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>agent.reset() · agent.history</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">With <code>memory: true</code> the conversation continues across runs.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import ai from "fogu:ai"
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
print(answer.steps[0].toolCalls[0].name, answer.steps[0].toolCalls[0].arguments)`}
        />
        <DocCodeBlock
          lang="text"
          title="output (the model's words vary)"
          code={`Olá
1, 2, 3
Yes — it is 21 °C in Lisbon, a pleasant temperature.
weather {city: Lisbon}`}
        />
      </section>

      {/* ================= FOGU:SSH (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-ssh" onRun={onRun} />

      {/* ================= FOGU:SHELL ================= */}
      <section id="mod-shell" data-group="Integrations" data-title="fogu:shell" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:shell</code></h2>
        <p>A small POSIX-style shell: pipes, <code>&amp;&amp;</code> <code>||</code> <code>;</code>, redirections (<code>&gt; &gt;&gt; &lt; 2&gt; 2&gt;&amp;1 &amp;&gt;</code>), <code>$VAR</code>, <code>$(…)</code>, quotes, globs and <code>~</code>. The usual Linux commands are built in, so scripts behave the same on Windows: <code>echo cat ls pwd cd mkdir rm cp mv touch grep head tail wc sort uniq cut tr tee basename dirname which env export true false exit sleep seq</code>. Other commands run as programs.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import shell from "fogu:shell"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.run(command, {`{ cwd, env, input, native = true }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">{`{ ok, code, stdout, stderr }`}</td><td className="py-2">A non-zero exit is not an error; <code>native: false</code> uses system programs only.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.exec(command, opts?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">same</td><td className="py-2">Output goes to the terminal as it happens.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.text(command, opts?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">stdout without the final newline; a failure is an error value.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.lines(command, opts?) · shell.json(command, opts?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · value</td><td className="py-2">Lines array or parsed JSON.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.quote(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Safe to interpolate into a command.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.cd(dir) · shell.cwd()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · str</td><td className="py-2">The shell's own working folder.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell.which(program) · shell.builtins</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · list</td><td className="py-2">Find binary path and builtin list.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import shell from "fogu:shell"
shell.run("mkdir -p data && echo banana > data/fruits.txt && echo apple >> data/fruits.txt")?
print(shell.text("cat data/fruits.txt | sort | head -n 1")?)
print(shell.lines("ls data")?, shell.text("cat data/*.txt | wc -l")?)
const name = "file with spaces.txt"
shell.run(\`echo ok > data/\${shell.quote(name)}\`)?
print(shell.text("echo \\"$WHO has $(cat data/fruits.txt | wc -l) fruits\\"", { env: { WHO: "Ana" } })?)
const r = shell.run("grep kiwi data/fruits.txt || echo 'no kiwi'")?
print(r.ok, r.stdout.trim())
const [_, err] = shell.text("cat missing.txt")
print(err)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`apple
[fruits.txt] 2
Ana has 2 fruits
true no kiwi
error: shell.text: exit code 1: cat: missing.txt: No such file or directory`}
        />
      </section>

      {/* ================= FOGU:FFI ================= */}
      <section id="mod-ffi" data-group="Integrations" data-title="fogu:ffi" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:ffi</code></h2>
        <p>Calls functions of native libraries (C ABI): <code>.dll</code>, <code>.so</code>, <code>.dylib</code>. <strong>Unsafe by nature</strong>: a wrong signature can crash the program. Supported signatures: up to 4 arguments mixing integers and floats, or up to 8 integer/pointer arguments.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import ffi from "fogu:ffi"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ffi.open(name)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Library · error</td><td className="py-2">A path or a name (<code>"user32"</code> → <code>user32.dll</code> / <code>libuser32.so</code>); <code>"c"</code> and <code>"m"</code> are the C runtime and the math library.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>lib.func(symbol, [argTypes], returnType = "void")</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">fn · error</td><td className="py-2">A Fogu function that calls the symbol.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>lib.has(symbol) · lib.name</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · str</td><td className="py-2">Inspect symbols.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>types</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>i8 i16 i32 i64 u8 u16 u32 u64 isize usize bool f64 ptr str buffer</code>; results also <code>f32 void</code>. <code>str</code> passes a NUL-terminated copy; a <code>str</code> result reads a C string.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ffi.alloc(size)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Buffer</td><td className="py-2">Memory for out-parameters: <code>text() bytes(start?, len?) write(offset, str | bytes) i32/u32/i64/f64(offset)</code>, fields <code>size address</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>ffi.types · ffi.suffix</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · str</td><td className="py-2">Supported types and dynamic library extension.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import ffi from "fogu:ffi"
const libc = ffi.open("c")?
const strlen = libc.func("strlen", ["str"], "usize")?
const atoi = libc.func("atoi", ["str"], "i32")?
print(strlen("hello"), strlen("olá"), atoi("42abc"))
const pow = ffi.open("m")?.func("pow", ["f64", "f64"], "f64")?
print(pow(2.0, 10))
const strcpy = libc.func("strcpy", ["buffer", "str"], "ptr")?
const buf = ffi.alloc(16)
strcpy(buf, "copied")
print(buf.text(), buf.bytes(0, 3))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`5 4 42
1024.0
copied [99, 111, 112]`}
        />
      </section>

      {/* ================= FOGU:PLAYBOOK ================= */}
      <section id="mod-playbook" data-group="Integrations" data-title="fogu:playbook" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:playbook</code></h2>
        <p>Configuration management in the style of Ansible: an <strong>inventory</strong> of hosts and <strong>playbooks</strong> of idempotent tasks, run over SSH (the system's OpenSSH client) or on the local machine (through <a className="underline" href="#mod-shell">fogu:shell</a>, so it also works on Windows). Hosts run in parallel; the output and the <code>PLAY RECAP</code> follow Ansible's. From the command line: <code>fogu play site.yml -i inventory.yml [--check] [-l hosts] [-t tags] [--skip-tags t] [-e key=value] [-f forks] [-v]</code> (exit code 2 when a host failed or was unreachable).</p>
        <DocCodeBlock lang="fogu" title="import" code={`import playbook from "fogu:playbook"`} />
        
        <h3 className="text-lg font-semibold mt-6">Inventory</h3>
        <p><code>vars</code> for every host; <code>hosts</code>: name → <code>{`{ host, port, user, key, password, become_password, connection: "local", …vars }`}</code>; <code>groups</code>: name → list of hosts or <code>{`{ hosts, vars }`}</code>. Patterns in <code>hosts:</code> — <code>all</code>, a host, a group, <code>web:db</code>, <code>web:!web2</code>.</p>
        <DocCodeBlock
          lang="yaml"
          title="inventory.yml"
          code={`hosts:
  web1: { host: 192.0.2.10, user: deploy, key: ~/.ssh/id_ed25519 }
  laptop: { connection: local }
groups:
  web: { hosts: [web1], vars: { port: 8080 } }`}
        />

        <h3 className="text-lg font-semibold mt-6">Playbooks and tasks</h3>
        <p>Templates: <code>{`{{ expr }}`}</code> with variables (<code>item</code>, <code>facts</code>, <code>inventory_hostname</code>, <code>groups</code>, registered results), <code>. []</code>, <code>+ - * / % ~</code>, comparisons, <code>and or not in</code>, <code>is defined</code>, and the filters <code>default upper lower trim length join first last replace split int float string bool to_json basename dirname</code>.</p>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>play: {`{ name, hosts, vars, become, gather_facts = true, tags, tasks, handlers }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Facts: <code>facts.os arch hostname distribution distribution_version pkg_mgr service_mgr user</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>task keys: name, when, loop, register, notify, ignore_errors, changed_when, check_mode, become, tags</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>when</code> is an expression; <code>loop</code> sets <code>item</code>; <code>register</code> keeps the result.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>shell / command: "cmd" | {`{ cmd, chdir, creates, removes, stdin }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Skipped in check mode (unless <code>check_mode: false</code>); <code>{`{ rc, stdout, stderr, stdout_lines }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>copy: {`{ content | src, dest, mode }`} · template: {`{ src, dest, mode }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Written only when the content or mode differs; <code>template</code> renders <code>{`{{ }}`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>file: {`{ path, state: directory | absent | touch | file, mode }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Manage files and folders.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>lineinfile: {`{ path, line, regexp, state, create, mode }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Replaces the last line matching <code>regexp</code>, or appends.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>package: {`{ name, state: present | absent, update_cache, use }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">apt, dnf, yum, zypper, apk, pacman (from the facts).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>service: {`{ name, state: started | stopped | restarted | reloaded, enabled }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">systemd.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>debug: {`{ msg }`} | {`{ var }`} · set_fact: {`{ … }`} · fail: {`{ msg }`} · wait_for: {`{ port | path, host, timeout }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Debugging and synchronization helpers.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          lang="yaml"
          title="site.yml"
          code={`- name: Web servers
  hosts: web
  become: true
  vars: { app: shop }
  tasks:
    - name: App folder
      file: { path: "/srv/{{ app }}", state: directory, mode: "0755" }
    - name: Config
      template: { src: templates/app.conf, dest: "/srv/{{ app }}/app.conf" }
      notify: restart nginx
    - name: Packages
      package: { name: [curl, nginx], state: present }
      when: facts.os == 'linux'
  handlers:
    - name: restart nginx
      service: { name: nginx, state: restarted }`}
        />

        <h3 className="text-lg font-semibold mt-6">From Fogu</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>playbook.run(plays | path, {`{ inventory, vars, check, limit, tags, skipTags, forks, verbose, quiet }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>{`{ ok, failed: [hosts], recap: { host: { ok, changed, unreachable, failed, skipped, ignored } } }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>playbook.inventory(path | map)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map</td><td className="py-2">The hosts and groups it describes.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>playbook.render(text, vars)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">The template engine on its own.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import playbook from "fogu:playbook"
const plays = [{
    hosts: "all",
    gather_facts: false,
    vars: { dir: "site", pages: ["index", "about"] },
    tasks: [
        { name: "pages", copy: { content: "<h1>{{ item | upper }}</h1>", dest: "{{ dir }}/{{ item }}.html" }, loop: "{{ pages }}" },
        { debug: { msg: "{{ pages | length }} pages in {{ dir }}" } },
    ],
}]
const first = playbook.run(plays)?
const again = playbook.run(plays, { quiet: true })?
print(first.ok, again.recap.localhost.changed)
print(playbook.render("{{ name | default('anon') | upper }}: {{ n * 3 }}", { n: 7 }))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`PLAY [all] *************************************************************

TASK [pages] ***********************************************************
changed: [localhost] => (item=index)
changed: [localhost] => (item=about)

TASK [debug] ***********************************************************
ok: [localhost] => 2 pages in site

PLAY RECAP *************************************************************
localhost : ok=2    changed=1    unreachable=0    failed=0    skipped=0    ignored=0
true 0
ANON: 21`}
        />
      </section>

      {/* ================= EMBEDDING (C API) (generated: docs/tools/reference) ================= */}
      <RefSection id="embed" onRun={onRun} />

      {/* ================= FOGU:VM (generated: docs/tools/reference) ================= */}
      <RefSection id="mod-vm" onRun={onRun} />

      {/* ================= FOGU:CLI ================= */}
      <section id="mod-cli" data-group="Integrations" data-title="fogu:cli" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:cli</code></h2>
        <p>Terminal interaction: prompts (<code>dialoguer</code>), colors and terminal control (<code>console</code>), spinners and progress bars (<code>indicatif</code>), tables (<code>comfy-table</code>) and argument parsing (<code>clap</code>). Without a terminal on stdin (pipes, CI) the prompts read plain lines, so <code>echo Ana | fogu app.fg</code> works; spinners and bars draw on stderr, only on a terminal. Colors are off when stdout is not a terminal or <code>NO_COLOR</code> is set.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import cli from "fogu:cli"`} />
        
        <h3 className="text-lg font-semibold mt-6">Prompts</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.input(prompt, {`{ default, allowEmpty, type: "str" | "int" | "float" }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str | int | float · error</td><td className="py-2">Prompt text input.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.password(prompt, {`{ confirm: "Repeat password", allowEmpty }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">Hidden input.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.confirm(prompt, {`{ default }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · error</td><td className="py-2">Yes/No prompt.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.select(prompt, items, {`{ default, fuzzy, index }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">value | nil · error</td><td className="py-2">Items are strings or <code>{`{ label, value }`}</code>; nil when cancelled with Esc; <code>fuzzy</code> filters while typing.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.multiSelect(prompt, items, {`{ defaults, index }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list | nil · error</td><td className="py-2">Without a terminal: names or numbers separated by commas.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import cli from "fogu:cli"
const name = cli.input("Your name", { default: "Ana" })?
const age = cli.input("Age", { type: "int" })?
const lang = cli.select("Language", ["Fogu", "Rust", { label: "Go lang", value: "go" }])?
if cli.confirm(\`Save \${name}?\`, { default: true })? { print(name, age, lang) }`}
        />
        <DocCodeBlock
          lang="text"
          title="output (answers piped)"
          code={`$ printf 'Bia\\n30\\n3\\ny\\n' | fogu app.fg
Bia 30 go`}
        />

        <h3 className="text-lg font-semibold mt-6">Styles, terminal, spinners and tables</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.style(text, ...styles)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2"><code>bold dim italic underline strike inverse</code>, colors <code>red green yellow blue magenta cyan white black gray</code>, <code>brightRed</code>, backgrounds <code>bgRed</code>, 256 colors <code>"208"</code> / <code>"bg208"</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.red(text) · green · yellow · blue · magenta · cyan · white · black · gray · bold · dim · italic · underline · strike · inverse</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Convenience color functions.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.colors(on?) · cli.stripAnsi(s) · cli.textWidth(s) · cli.link(text, url)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · str · int · str</td><td className="py-2"><code>colors(true)</code> forces colors.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.isTTY("stdin" | "stdout" | "stderr") · cli.size()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">bool · {`{ columns, rows }`} | nil</td><td className="py-2">Terminal metrics.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.clear() · clearLine() · hideCursor() · showCursor() · readKey()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>readKey</code> → a character or <code>Enter Escape Backspace Tab ArrowUp ArrowDown ArrowLeft ArrowRight Home End PageUp PageDown Delete</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.spinner(text, {`{ frames }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Spinner</td><td className="py-2"><code>text(t) succeed(msg?) fail(msg?) warn(msg?) info(msg?) stop()</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.progress(total, {`{ message, prefix, template, chars }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">ProgressBar</td><td className="py-2"><code>inc(n = 1) set(n) length(n) message(m) println(line) finish(msg?)</code>, fields <code>position length elapsed finished</code>. Templates of <code>indicatif</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.table(rows, {`{ headers, style: "rounded" | "utf8" | "ascii" | "markdown" | "none", align, width }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Rows are lists or maps; numbers are right aligned.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import cli from "fogu:cli"
print(cli.red("error"), cli.style("ok", "bold", "green"))
print(cli.table([["ana", 3], ["rui", 15]], { headers: ["name", "jobs"] }))
print(cli.table([{ name: "fogu", stars: 9 }], { style: "markdown" }))
const bar = cli.progress(3)
for i in 0..3 { bar.inc() }
bar.finish()
print(bar.position, cli.stripAnsi(cli.red("x")))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`error ok
╭──────┬──────╮
│ name ┆ jobs │
╞══════╪══════╡
│ ana  ┆    3 │
│ rui  ┆   15 │
╰──────┴──────╯
| name | stars |
|------|-------|
| fogu |     9 |
3 x`}
        />

        <h3 className="text-lg font-semibold mt-6">Arguments</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>cli.parse(spec, argv = Fogu.args, {`{ exit = true }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">map · error</td><td className="py-2"><code>spec</code>: <code>{`{ name, version, about, args: [...], commands: [{ name, about, args }], commandRequired }`}</code>. <code>--help</code> and <code>--version</code> print and exit; a usage error prints and exits with 2, or is an <code>error</code> value with <code>{`{ exit: false }`}</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>arg: {`{ name, type, short, long, help, positional, required, multiple, default, choices, env }`}</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Types <code>str int float bool count</code>; <code>multiple</code> gives a list. A subcommand sets <code>command</code> and its values in <code>args</code>.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import cli from "fogu:cli"
const opts = cli.parse({
    name: "deploy", version: "1.0.0", about: "Deploys the app",
    args: [
        { name: "env", positional: true, required: true, choices: ["dev", "prod"] },
        { name: "force", short: "f", type: "bool", help: "skip the checks" },
        { name: "tag", type: "str", multiple: true },
        { name: "verbose", short: "v", type: "count" },
    ],
    commands: [{ name: "logs", args: [{ name: "lines", short: "n", type: "int", default: 10 }] }],
}, ["prod", "-f", "--tag", "a", "--tag", "b", "-vv", "logs", "-n", "5"])
print(opts)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`{args: {lines: 5}, command: logs, env: prod, force: true, tag: [a, b], verbose: 2}`}
        />
      </section>

      {/* ================= FOGU:HTML ================= */}
      <section id="mod-html" data-group="HTML &amp; browser" data-title="fogu:html" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:html</code></h2>
        <p>Parse HTML and query it with CSS selectors (scraping), render Jinja templates (minijinja), sanitize untrusted HTML (ammonia) and convert between HTML, plain text and Markdown. An invalid selector stops the program; template errors are error values.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import html from "fogu:html"`} />
        
        <h3 className="text-lg font-semibold mt-6">Parsing and selectors</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.parse(text) · html.fragment(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">HtmlDocument</td><td className="py-2">A full document or a fragment.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>doc.select(css) · doc.find(css)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · HtmlElement | nil</td><td className="py-2">All matches · the first one.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>doc.title · doc.text · doc.html · doc.root</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Document properties.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>el.tag · el.text · el.html · el.outerHtml · el.id · el.classes · el.attrs · el.children · el.parent</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2"><code>text</code> has whitespace collapsed.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>el.attr(name) · el.hasClass(c) · el.select(css) · el.find(css) · el.closest(css) · el.matches(css)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Element traversal.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.select(text, css)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list</td><td className="py-2">Parse + select in one call.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import html from "fogu:html"
import http from "fogu:http"
const doc = html.parse("<ul id='news'><li><a href='/a'>Fogu 1.0</a> <b>new</b></li><li><a href='/b'>JIT notes</a></li></ul>")
for a in doc.select("#news li > a") {
    print(a.text, "→", a.attr("href"))
}
print(doc.find("b").closest("li").find("a").text, doc.find(".missing"))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`Fogu 1.0 → /a
JIT notes → /b
Fogu 1.0 nil`}
        />

        <h3 className="text-lg font-semibold mt-6">Templates (Jinja)</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.render(source, data)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">Autoescaped; <code>{`{{ x | markdown }}`}</code> and <code>| striptags</code> are added to the Jinja filters.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.template(source, {`{ autoescape = true }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Template</td><td className="py-2"><code>t.render(data)</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.templates(folder, {`{ autoescape }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Templates</td><td className="py-2"><code>render(name, data)</code>, <code>renderString(source, data)</code>, <code>has(name)</code>; <code>extends</code>/<code>include</code>/<code>import</code> load from the folder; autoescape by extension.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import html from "fogu:html"
const page = html.render("<h1>{{ title }}</h1>{% for p in items %}<li>{{ loop.index }}. {{ p.name | upper }}</li>{% endfor %}", {
    title: "Tom & Jerry",
    items: [{ name: "café" }, { name: "pão" }],
})?
print(page)
const [_, err] = html.render("{% if %}", {})
print(err)`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`<h1>Tom &amp; Jerry</h1><li>1. CAFÉ</li><li>2. PÃO</li>
error: html.render: syntax error: unexpected end of block (in <string>:1)`}
        />

        <h3 className="text-lg font-semibold mt-6">Sanitizing and conversions</h3>
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.escape(text) · html.unescape(text)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Entities: named, decimal and hex.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.sanitize(text, {`{ tags, addTags, removeTags, attributes, stripComments }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Removes scripts, event handlers and dangerous URLs.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.text(text) · el.toText()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Plain text with paragraphs, bullets and tab-separated cells.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.toMarkdown(text) · el.toMarkdown()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">Headings, emphasis, links, images, lists, code blocks, quotes, tables.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>html.fromMarkdown(md)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str</td><td className="py-2">CommonMark + tables, strikethrough, task lists, footnotes.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import html from "fogu:html"
print(html.sanitize("<p onclick='steal()'>hi<script>alert(1)</script></p>"))
print(html.toMarkdown("<h2>Notes</h2><p>Some <b>bold</b> and <a href='https://fogu.dev'>a link</a></p><ul><li>one</li><li>two</li></ul>"))
print(html.fromMarkdown("**fogu** is *fun*"))`}
        />
        <DocCodeBlock
          lang="text"
          title="output"
          code={`<p>hi</p>
## Notes

Some **bold** and [a link](https://fogu.dev)

- one
- two
<p><strong>fogu</strong> is <em>fun</em></p>`}
        />
      </section>

      {/* ================= FOGU:WEBVIEW ================= */}
      <section id="mod-webview" data-group="HTML &amp; browser" data-title="fogu:webview" className="scroll-mt-20 mt-16">
        <h2 className="text-2xl font-bold"><code>fogu:webview</code></h2>
        <p>Drives a real browser (Chrome, Edge, Chromium or Brave) through the DevTools protocol: render JavaScript-heavy pages, click, type, run scripts, take screenshots and PDFs. The browser is found in <code>FOGU_CHROME</code>, the <code>path</code> option, the PATH or the usual install folders. Every command runs on a worker thread; the browser processes are closed when the program ends.</p>
        <DocCodeBlock lang="fogu" title="import" code={`import webview from "fogu:webview"`} />
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
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>webview.launch({`{ headless = true, path, args, width = 1280, height = 800, userAgent, timeout = 30s }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Browser · error</td><td className="py-2">Launch browser.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>webview.render(url, {`{ wait, timeout }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">The HTML after the page's JavaScript ran (shared headless browser).</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>webview.find()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str | nil</td><td className="py-2">The browser that would be used.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>browser.open(url?, {`{ wait = "load" | "domcontentloaded" | "networkidle" | "none", timeout }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Page · error</td><td className="py-2">A new tab.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>browser.version() · browser.close()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Browser info and close.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.goto(url, opts) · reload() · back() · forward()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Navigation.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.title() · url() · content() · text(css?)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">str · error</td><td className="py-2">Page info.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.eval(js, ...args)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">value · error</td><td className="py-2">An expression (promises are awaited) or a function called with JSON arguments.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.click(css) · type(css, text) · fill(css, value) · press(key)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Real mouse/keyboard events; <code>press("Enter")</code>.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.waitFor(css, timeout) · waitForNavigation(opts)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Wait conditions.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.select(css)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">list · error</td><td className="py-2"><code>[{`{ tag, text, html, attrs }`}]</code> from the live DOM.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.setContent(html) · setViewport(w, h) · setHeaders(map)</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil · error</td><td className="py-2">Page setup.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.screenshot({`{ fullPage, element }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">Image · error</td><td className="py-2">A <a className="underline" href="#mod-image">fogu:image</a> image.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.pdf(path?, {`{ format = "A4", landscape, background = true, scale }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil | bytes · error</td><td className="py-2">Print to PDF.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.cookies() · setCookie({`{ name, value, url | domain }`})</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap"></td><td className="py-2">Cookies management.</td></tr>
              <tr className="align-top"><td className="py-2 pr-4 whitespace-nowrap"><code>page.close()</code></td><td className="py-2 pr-4 text-slate-500 whitespace-nowrap">nil</td><td className="py-2">Close tab.</td></tr>
            </tbody>
          </table>
        </div>
        <DocCodeBlock
          title="example.fg"
          onRun={onRun}
          code={`import webview from "fogu:webview"
const browser = webview.launch()?
const page = browser.open("https://example.com")?
print(page.title()?)
print(page.eval("document.querySelectorAll('a').length")?)
page.screenshot({ fullPage: true })?.save("example.png")?
page.pdf("example.pdf")?
browser.close()`}
        />
        <DocCodeBlock
          lang="text"
          title="output (with Chrome installed)"
          code={`Example Domain
1`}
        />
      </section>
    </>
  );
};
