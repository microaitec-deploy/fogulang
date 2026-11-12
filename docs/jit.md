# Fogu's Cranelift JIT

The (tree-walking) interpreter is the semantic reference. The JIT compiles only
a numeric subset and, if something is not supported, the function/loop keeps
running in the interpreter.

## Usage

```text
fogu script.fg                # JIT on
fogu --no-jit script.fg       # interpreter only
fogu --jit-stats script.fg    # statistics at the end
fogu --jit-debug script.fg    # shows what it compiles / why it gives up
```

Environment: `FOGU_JIT=0`, `FOGU_JIT_DEBUG`, `FOGU_JIT_CALLS` (default 2), `FOGU_JIT_LOOPS` (default 200).

## How it works

- **Functions**: after N interpreted calls, a specialization for the argument
  types (Int/Float/Bool) is compiled; the return type is inferred. Direct
  recursion is supported.
- **Loops (OSR)**: a hot `while` is compiled mid-execution; long `for a..b`
  loops are compiled right away. Free numeric variables go through a slot
  array (written back at the end).
- **Lists, maps and texts in loops (speculation)**: a list or map the loop
  uses goes through a slot as a pointer. `xs.push(…)`, `m[k] = v`, `xs.len()`
  and `m.len()` call runtime helpers. `for x in list`, `xs[i]` and `m[k]`
  *speculate* a type (that of the first element/value when the loop starts)
  and every read is checked by its helper. When a check fails (another type,
  a missing key, an index out of range) the helper sets `ctx.deopt`, the loop
  restores the variables as they were at the start of that iteration, returns
  the iteration index, and the interpreter redoes it with its own rules
  (`LoopOutcome::Deopt`). This is only sound before any side effect of the
  iteration, so a guard emitted after a `push`, a map write, a `print` or a
  call makes the whole loop bail, as does a nested loop with both guards and
  effects. Text elements (`Ty::Str`) are pointers into the list snapshot the
  loop walks: `p.len()`, `"<" + p + ">"`, `print(p)`, keys of `m[p]`.
- **Errors** (overflow, division by zero, negative pow, stack, non-exhaustive
  match) have the same messages as the interpreter (`ops.rs`).
- **Type annotations** cost nothing: specializations are per argument type, so
  a matching `x: int` / `: float` annotation is checked at compile time; a
  mismatch makes the JIT give up and the interpreter reports the error.
- **Sized numbers** (`int8` … `uint64`, `float32`): an annotation is one
  unsigned compare (`range_guard`) whose failure raises the interpreter's own
  message (`ops::custom_code`); a conversion is a truncation plus a sign or
  zero extension (`float32`: `fdemote` + `fpromote`).
- **Calls** are a plain native call: the stack limit is one compare of the
  stack pointer (`get_stack_pointer`) against `JitCtx::stack_limit`, with no
  counter to update, and the current line lives in a register — it is written
  to `JitCtx::line` only by the cold block of an error (0 = the function had
  not reached its first statement: the caller's line stands).
- **Deadlines** (`fogu:vm` timeouts): the limit of the stack check is `jit::Watch::limit`, which a
  watchdog thread raises when the deadline of the running native code passes. Code compiled for a
  run with a deadline (`checked` specializations) also checks it at every loop iteration; the
  failure is reported as the run's timeout. The limit is read with an atomic load so the
  optimizer never reuses an older value.
- **`fogu:ffi`**: a C function with a numeric signature has a typed entry built by
  `ffi_stub::direct` and is called like any typed native; it is *fallible* (a callback may have
  failed), so the call is followed by `typed_check` and the usual error branch.
- **Typed natives**: a native function with a `TypedNative` entry (numbers in,
  a number out, cannot fail — e.g. `time.monotonic`, `time.now`) is called
  directly with `call_indirect`; no `Value` is built.

## Supported subset

int/float/bool, checked arithmetic, comparisons, if/while/for a..b, match,
calls to JIT-able functions, `math.sqrt/abs/floor/ceil/pow`, `int()`,
`float()`, `print` of scalars/literals, type annotations of numeric types.
Inside loops: `for x in list`, `xs.push(number | text | [...] | {...})`,
`xs[i]`, `xs.len()`, `m[k]`, `m[k] = v`, `m.len()` (keys: literals, templates,
`str(n)`, `+` of texts, a text element), `s.len()` of a text element
(`tests/spec/jit_list_push.fg`, `jit_push_values.fg`, `jit_for_list.fg`,
`jit_map_index.fg`).

- **Mutual recursion** (`is_even`/`is_odd`): the return type of a function being
  compiled is assumed; whatever depends on it is provisional and dropped if the
  guess fails.
- **Errors**: the variables modified by a native loop are written back even on
  error; the error's line number comes from the native code (`ctx.line`).

## Limitations

Text variables (`let s = "a" + x`), structs, list/map values outside the forms
above and lists or maps returned from functions stay in the interpreter (they
would need reference-counted values inside native code). So do `?`, `error`,
destructuring, `fg`, default parameters that are actually used, and variadic
functions. A loop that deopts finishes in the interpreter (it is not
recompiled with another speculation).

## Optimizations of 0.7.5

- **Inlining**: the IR of every compiled function of at most 120 instructions is kept
  (`Inner::bodies`) and copied into its call sites by Cranelift's inliner (`Context::inline`), up
  to 480 instructions per caller, one level deep. A function that calls itself receives its own
  body at each call site. Error handling is unchanged: the inlined body stores the error in the
  context and the caller's check after the (former) call propagates it.
- **Division and remainder by a constant** (`div_const`): shift and mask for powers of two,
  multiply-high by a magic number otherwise (`div_magic`); no zero / overflow checks, since the
  divisor is known. `-1`, `0` and `i64::MIN` take the general path.
- **Ranges** (`Gen::ranges`): the result of `x % c`, `x & mask`, constants and the variable of a
  `for` over constant bounds are known to lie in a range; `+ - *` between ranged values whose
  result fits an int are emitted without the overflow check. Conditions add scoped facts about
  variables (`Gen::facts`: `while n < 50`, `if x <= 9`, joined by `&&`), valid in the body until
  the variable is assigned or a loop starts.
- **Branch order** (`order_branches`): a conditional branch that can leave a loop leaves through
  its first target. Cranelift's elaboration visits the second target first and forgets a loop at
  the first block outside of it, so this is what makes its loop-invariant code motion work (and
  keeps it from sinking outer values into inner loops).
- **Conditions** (`branch_on`): `&&` / `||` in `if` / `while` are control flow.
- Release builds turn the IR verifier and unwind tables off.

`fogu --jit-debug` prints the start and compile times; `FOGU_JIT_DEBUG=2` the IR, `=3` also the
optimized IR and the machine code.

## Benchmarks (release)

| test | interpreter | JIT |
| --- | --- | --- |
| fib(32) | 3.02 s | 0.025 s |
| loop 50M | 16.7 s | 0.069 s |
| primes | 5.2 s | 0.051 s |
| mandelbrot | 3.5 s | 0.013 s |
| collatz | 16.5 s | 0.137 s |

Validated differentially against the interpreter on ~1100 random programs.
