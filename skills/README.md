# Skills for AI agents

`skills/fogu/` teaches an AI agent to write, run, test and ship Fogu programs.
It follows the [Agent Skills](https://agentskills.io) format (a folder with a
`SKILL.md` whose frontmatter says when to use it), so it works with Claude
Code, the Claude apps and API, and other agents that read skills.

| File | Contents |
| --- | --- |
| `fogu/SKILL.md` | The rules an agent must know: syntax, errors as values, modules, concurrency and servers, the check/run/test loop, common mistakes coming from JavaScript/Python |
| `fogu/stdlib.md` | Every `fogu:*` function with its signature, return value and a tested example — **generated**, do not edit |

## Installing

```bash
# Claude Code, for every project
cp -r skills/fogu ~/.claude/skills/fogu
# Claude Code, only for one project
cp -r skills/fogu <project>/.claude/skills/fogu
```

On claude.ai, zip the `fogu` folder and upload it under Settings → Capabilities
→ Skills. Other agents can be pointed at `llms.txt` (repository root) or given
`SKILL.md` as context; for tools that read `AGENTS.md`, add a line such as
"To write Fogu code, follow skills/fogu/SKILL.md".

## Keeping it up to date

- `stdlib.md` is written by `python docs/tools/gen_reference.py` from
  `docs/tools/reference/*.py` (the same source as the docs.html reference), and
  its examples are run with the real `fogu` binary.
- `SKILL.md` is written by hand: when the language or the CLI changes, update it
  and run its examples (`fogu check` / `fogu` on each block).
