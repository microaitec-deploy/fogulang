# Closures stored in their own scope

Fogu values are reference counted (`Rc`). A function value keeps the scope it
was created in (`FnValue::closure`), so a function stored *in that same scope*
forms a cycle that reference counting never frees:

```text
call scope E ──binding "g"──▶ Value::Fn(g) ──closure──▶ E
```

That is the common shape of a request handler with a local helper:

```fogu
app.get("/x", (ctx) => {
    const fmt = (x) => `item ${x}`     // fmt keeps the handler's scope, which keeps fmt
    return { message: fmt(ctx.path) }
})
```

Before 0.8.1 each call leaked its whole scope (parameters included): about
500 bytes per call for a bare helper, ~4 KB per request in the tester's
`fogu:web` server (1.5 GB in 21 s).

## What creates the cycle

| Construct | Cycle | Freed now? |
|---|---|---|
| `const g = (x) => …` / `fn helper() {}` in a function, a block or a loop body | scope → g → scope | yes |
| a closure stored in a struct or map held only by the scope (`h.f = …`, `m.f = fn() { m.n }`) | scope → struct/map → fn → scope | yes |
| the same with the closure returned, sent on a channel, captured by a task | not garbage | kept (correct) |
| a closure passed straight to a call (`xs.map((x) => …)`) | none | always was |
| data that contains itself (`l.push(l)`, `a.next = b; b.next = a`) | list/map/struct cycle | **no** (needs a cycle collector) |
| a closure that captures itself through an outer variable of a scope that lives on | depends | no |

## The fix: break the cycle when the scope ends

`Environment::break_cycles(env, holders)` runs when a call scope is released,
when a block ends (`ScopeEnd`), and when a loop body's scope is reused or
dropped (`LoopScope`) — only when something besides the owner still holds the
scope (`Rc::strong_count > holders`), so a call that created no closure pays
one comparison.

It counts the references to the scope that come from function values that
nothing else holds (strong count 1), found directly in the scope's bindings or
inside lists, maps and structs that nothing else holds (a bounded walk of 256
values). If that count accounts for *every* extra reference, nothing outside
can reach the scope or those functions: they are garbage. The bindings are then
dropped, which breaks the cycle. Otherwise nothing changes. Any reference it
cannot see (a task, a child scope, an escaped closure, a weak reference) makes
the count disagree, so the rule only ever frees what is unreachable.

## Options considered

| Option | Fixes | Does not fix | Cost / risk |
|---|---|---|---|
| **Break env→fn→env at scope end** (done) | local helpers, nested fns, closures in local structs/maps, blocks, loop bodies | data cycles; closures reachable through big (>256 values) local structures | O(bindings) only at the end of a scope that a closure captured; no semantic change |
| Do not capture the scope when a function uses no free variable | helpers that use nothing from the scope | helpers that do (the common `fmt` above reads nothing, but `(x) => x + base` does) | resolver must know free variables; small |
| Capture only the free variables (upvalue cells, as `docs/bytecode.md` designs) | every env cycle | a closure that captures *itself* through its cell; data cycles | the VM's Phase 4 work; changes the closure model in the tree-walker too |
| Cycle collector over environments only | every env cycle, including escaped-then-dropped ones | data cycles | trial deletion over scopes; pauses proportional to live scopes; needs a trigger |
| General cycle collector (Bacon–Rajan) | everything | — | touches every `Rc` of `Value`, write barriers or periodic scans; measurable slowdown; a fundamental change (memory model) |

Data cycles are left as they are: they are explicit in the program (`l.push(l)`)
and can be broken by hand (`a.next = nil`).
