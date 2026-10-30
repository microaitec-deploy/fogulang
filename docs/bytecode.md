# The bytecode VM (design, Phase 4)

Status: **design**. The tree-walking interpreter (`runtime/interpreter.rs`) is the
semantic reference and stays so until the VM passes the whole spec suite with
identical output. This document fixes the instruction set and the execution
model before the code is written, so the compiler, the VM and the JIT (which
will compile *from bytecode*, Phase 5) agree.

## Why

Measured on the tree-walker (no JIT, 1M iterations, `FOGU_PPROF=1`): a simple
`while` iteration costs ≈ 240 ns, a call ≈ 500 ns, a map or list access ≈ 370 ns.
The profile is flat: variable access (walk up the scopes, linear scan, `Value`
clone) ≈ 25%, the `Result<Value, String>` moves of `?` ≈ 20%, dispatch and
`Flow` ≈ 20%, allocation of scopes and argument vectors the rest. None of it is
one hot spot; all of it is the shape of a tree-walker over hash-mapped scopes.
A register VM with slots resolved at compile time removes the scope walks, the
per-node `Result`, the `Rc<RefCell<Environment>>` per block and most clones.
The target of Phase 4 is **≥ 5× on `strings`, `maps`, `records`, `sort`** with
the JIT off, and no regression with it on.

## Execution model

### Values

`Value` stays as it is (24 bytes, `Rc` for strings, lists, maps, structs,
closures). The VM does not change the value model; Phase 3's compact value is
independent of it.

### Functions and frames

Every Fogu function (`fn`, methods, constructors, arrow functions, the top level
of a module) compiles to a `Proto`:

```text
Proto {
    name, file, params: u8, slots: u16,          // registers of a frame
    code: Vec<Op>, lines: Vec<(pc, line)>,       // line table for errors
    consts: Vec<Value>,                          // literals (int/float/str/duration/nil/bool)
    syms: Vec<Sym>,                              // names: globals, fields, methods
    protos: Vec<Rc<Proto>>,                      // nested functions (closures)
    upvals: Vec<UpvalDesc>,                      // what a closure captures
}
```

A call pushes a `Frame { proto, base, pc, ret_reg }` on the VM's frame stack and
reserves `slots` registers on one value stack (`Vec<Value>`): parameters first,
then every local of the body (one register per declaration; blocks reuse the
registers of the blocks that ended). Registers are addressed as `base + n`.
There is no `Environment` for VM frames.

### Variables

The resolver (`runtime/resolver.rs` already walks scopes) assigns every name one
of three homes at compile time:

| Home | Access | When |
| --- | --- | --- |
| **local** `Rn` | the register | declared in this function (params, `let`, loop variables, match bindings) |
| **upvalue** `Un` | a cell shared with the defining frame | a local of an enclosing function read or written by a closure |
| **global** `Gsym` | the module's `Environment` (hash map by `Sym`) | top-level declarations, imports, natives |

Locals captured by a closure live in a `Rc<RefCell<Value>>` cell from the moment
they are declared (the compiler knows a name is captured because the resolver
saw the inner function use it): `GetUpval`/`SetUpval` read and write the cell.
Non-captured locals are plain registers. A closure value is a `Value::Fn` whose
`FnValue` carries the proto and the captured cells (the `closure: Environment`
field becomes `Option` for tree-walker functions only, and goes away with the
tree-walker in the last step).

The top level of a module keeps the `Environment` for its globals: `import`,
`export`, the REPL, `fogu:vm` contexts and `Fogu.host` all read and write it.
Reads of globals from compiled code go through `GetGlobal` (one hash of an
integer); hot loops read globals into a register once when the resolver proves
the global is a `const` (or a function/native, which the program cannot
reassign without redefining it: the redefine epoch invalidates compiled code,
as the JIT does today).

### Errors and control

A runtime error is `VmError { message, line, file }`, returned as `Err` from
the run loop and converted to today's `file:line: runtime error: msg` text.
`Op` execution never returns `Result` per instruction: the run loop is one
`match` in a `loop`, and a failing instruction jumps to the error exit with the
current `pc` (the line table gives the line).

`?` (`Try`) compiles to a test of the value (`Value::Error`, or a `[value, error]`
pair) and a conditional **return of the pair** from the function: no sentinel
error travels through the stack any more. `?.` compiles to a null test and a
jump to the end of the chain. `break`/`continue` are jumps. `return` is `Ret`.

### Tasks and the scheduler

The VM is re-entrant per task as the interpreter is now: each `fg` task runs on
its own coroutine stack with its own VM state (frame stack + register stack).
Phase 6 (stackless tasks) later turns a task into *just* those two stacks, which
is why the VM keeps all its state in them and never on the Rust stack across a
yield point (`Recv`, `Send`, `Select`, blocking natives).

## Instruction set

Register machine, three-address, 32-bit instructions: 8-bit opcode and three
8-bit operands (`A B C`), or one 8-bit and one 16-bit (`A Bx`), or a signed
16-bit jump offset. Constants and symbols beyond 255 use an `ExtArg` prefix.

### Loads and moves

| Op | Meaning |
| --- | --- |
| `Move A B` | `R[A] = R[B]` |
| `LoadK A Bx` | `R[A] = consts[Bx]` |
| `LoadNil A` · `LoadTrue A` · `LoadFalse A` · `LoadInt A sBx` | small literals without the constant table |
| `GetGlobal A Bx` · `SetGlobal A Bx` | by `syms[Bx]` in the module environment; a missing name is the usual error |
| `GetUpval A B` · `SetUpval A B` | the closure's cells |
| `Line Bx` | the source line for errors (emitted when the line changes, folded into the line table of the proto; no runtime cost) |

### Arithmetic and comparison

| Op | Meaning |
| --- | --- |
| `Add Sub Mul Div Rem Pow A B C` | `R[A] = R[B] op R[C]` with the interpreter's semantics (`ops.rs`: checked ints, int→float promotion, string `+`, duration arithmetic, list `+`) |
| `AddK …` variants | the right operand is a constant (`i + 1`) |
| `Neg A B` · `Not A B` | |
| `Eq Ne Lt Le Gt Ge A B C` | `values_equal` / ordering; a struct with `equals` calls it |
| `Test A sBx` | jump if `R[A]` is falsy (`false`/`nil` only) |
| `TestNot A sBx` | jump if truthy |
| `Jmp sBx` | |
| `Coalesce A B sBx` | `R[A] = R[B]`; jump when it is not nil (`??`) |

Fused compare-and-jump (`LtJmp B C sBx`…) and integer fast paths (`AddII` on
two ints, overflow-checked) come in the second step, driven by the benchmarks.

### Collections and fields

| Op | Meaning |
| --- | --- |
| `NewList A B` | `R[A] = []` with capacity `B` |
| `ListPush A B` | append `R[B]` to the list in `R[A]` (list literals and `xs.push(x)` of one argument) |
| `NewMap A B` | |
| `MapSet A Bx C` | `R[A][syms[Bx]] = R[C]` (map literals with literal keys) |
| `Index A B C` | `R[A] = R[B][R[C]]` (list, map, str, buffer, module) |
| `SetIndex A B C` | `R[A][R[B]] = R[C]` |
| `GetField A B Cx` | `R[A] = R[B].syms[Cx]` (map, module, struct field or bound method, object field, error, duration) with the interpreter's privacy rule for `_names` |
| `SetField A Bx C` | |
| `Len A B` | |
| `Iter A B` · `Next A B sBx` | `for x in …`: `R[A]` holds an iterator over a snapshot of the list, the sorted keys of a map, the chars of a string, the bytes of a buffer, a channel or a `TcpListener`; `Next` writes the item to `R[B]` or jumps when done. `for k, v in` uses `Next2`. Ranges are compiled to an integer counter (`ForPrep`/`ForLoop`, Lua-style), the shape the loop JIT already knows |

Inline caches for `GetField`/`SetField` on structs (type id → field index)
come in the second step.

### Calls

| Op | Meaning |
| --- | --- |
| `Call A B C` | `R[A] = R[A](R[A+1] … R[A+B])`, `C` results (0 or 1); the callee may be a Fogu proto (new frame, no Rust recursion), a native (the current `call_native_fn`, with the host's `intercept`/`before_call`), a bound method or a struct type |
| `CallMethod A Bx C` | `R[A] = R[A+1].syms[Bx](R[A+2] …)`: the receiver is not re-evaluated; list/map/str/buffer built-in methods through `collections::call_method`, otherwise `GetField` + `Call` |
| `TailCall` | `return f(...)`: reuses the frame (direct recursion and mutual recursion without growing the frame stack) |
| `Ret A` · `RetNil` · `RetPair A` | `RetPair` returns `[R[A], R[A+1]]` (`return a, b`) |
| `Closure A Bx` | `R[A]` = a closure of `protos[Bx]` capturing the listed cells/registers |
| `New A Bx C` | a struct instance (`new T(...)`) |
| `Spawn A B` | `fg f(args)`: evaluates callee and arguments, then hands the call to the scheduler |

### Channels, select, match

| Op | Meaning |
| --- | --- |
| `Chan A B` · `Send A B` · `Recv A B` · `Close A` | as today (cooperative waits in the scheduler) |
| `Select …` | compiled to a `SelectBegin` listing the cases' channels and values, a scheduler wait, then a jump table on the ready case |
| `match` | compiled to tests and jumps (`Eq` + `Test`), `MatchFail` raises the non-exhaustive error with the interpreter's message |
| `TypeCheck A Bx` | `let x: T =`, parameter and return annotations: the same `type_matches` as today |

### Modules and misc

| Op | Meaning |
| --- | --- |
| `Import A Bx` · `DynImport A B` | through `module.rs` (cache, `fogu:*`, packages) |
| `Export Bx A` | |
| `Template A B C` | concatenates `R[B]..R[B+C]` as text |
| `Try A sBx` | `expr?`: when `R[A]` is an error or a `[v, err]` pair with an error, record the trace line and return the pair (or the error alone when the declared return type is not a tuple); else `R[A]` = the value |
| `Deadline` | at loop back-edges and before calls: the `fogu:vm` timeout/cancel check, only when a deadline is set (one load) |

## The compiler

`runtime/vm/compiler.rs`: one pass over the resolved AST per function, producing
a `Proto`. Register allocation is a stack discipline (a block's registers are
freed when it ends; an expression's temporaries right after it). The resolver
provides, per name occurrence, its home (local/upvalue/global) and, per
declaration, whether it is captured.

Desugarings stay in the parser (default parameters as a `?? value` prologue,
annotations as `CheckType`/`ReturnType`), so the compiler sees what the
interpreter sees. Every construct of the language compiles: there is no "bail"
in Phase 4. The engines are selected per program: `--engine=tree|vm`
(`FOGU_ENGINE`), the tree-walker remaining the default until the VM is proven,
then the VM becomes the default and the tree-walker is removed (last step).

`fogu disasm file.fg` prints the protos, for the tests and for reading what a
program became.

## The JIT

The loop and function JIT compiles from the AST today and bails on anything
non-numeric. Phase 5 moves it to bytecode: a hot proto (or a hot loop in one:
OSR at `ForLoop`/`Jmp` back-edges) is compiled from its `Op`s, which is simpler
than from the AST (registers are already resolved; types come from the
speculation and guards that the current JIT already has, `jit.rs`:
`Ty::List/Map/Str`, deopt at iteration boundaries). Until Phase 5, the VM is
opt-in: with `--engine=vm` the JIT is off, and the benchmarks compare
tree-walker+JIT, VM, VM+JIT when it exists. This is why the VM is not switched
on by default in a half state: hot numeric loops would leave the JIT and get
slower.

## Testing

- The spec suite (`tests/spec.rs`) runs every program on both engines and
  compares the output byte for byte, including error messages and the
  `file:line` of errors; `.err` tests must fail on both.
- Differential fuzzing (as `../fogulang` did with ~1100 random programs):
  `bench/fuzz.fg` generates programs from the grammar, runs both engines, and
  diffs the outputs.
- `fogu disasm` output for a dozen programs is checked against committed
  listings, so a change to the compiler is visible in the diff.
- The benchmarks (`bench/run.fg`) get an `engine` column.

## Steps and exit criteria

1. `Proto`, `Op`, the VM loop, the compiler for expressions, locals,
   `if`/`while`/`for` ranges, calls of Fogu functions and natives, `return` —
   `fib`, `loop` and `mandel` run on `--engine=vm` with the same output;
   `fogu disasm`.
2. Lists, maps, strings, templates, `for x in list/map/str`, `?`, `return a, b`,
   destructuring, `match` — `strings`, `maps`, `records`, `sort` run; the
   first measurement against the tree-walker (target ≥ 5×).
3. Closures and upvalues, arrow functions, default and variadic parameters,
   structs, `embed`, interfaces, `new`, `equals`, type annotations.
4. Modules (`import`/`export`, packages, `import()`), native objects and bound
   methods, the host's intercepts, `fogu:vm` contexts, the REPL.
5. `fg`, channels, `select`, `timeout`, deadlock detection — the spec suite is
   green on both engines.
6. Fused compare-and-jump, integer fast paths, inline caches; the VM becomes
   the default; the tree-walker is removed.

Exit criterion of Phase 4: every spec test produces identical output on both
engines, and the VM is ≥ 5× faster than the tree-walker on `strings`, `maps`,
`records` and `sort` with the JIT off.
