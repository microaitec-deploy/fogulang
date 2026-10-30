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

## Benchmarks (release)

| test | interpreter | JIT |
| --- | --- | --- |
| fib(32) | 3.02 s | 0.025 s |
| loop 50M | 16.7 s | 0.069 s |
| primes | 5.2 s | 0.051 s |
| mandelbrot | 3.5 s | 0.013 s |
| collatz | 16.5 s | 0.137 s |

Validated differentially against the interpreter on ~1100 random programs.
