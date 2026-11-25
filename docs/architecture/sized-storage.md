# Design note: storage sized by the annotation

Status: implemented (0.8.2); compiled loops and `list<int>` / `list<float>` / `list<bool>` in 0.8.3 · changes a fundamental (the value representation),
so it follows `engineering-principles.md`: this note, measurements before and
after, the full suite.

## Problem

Sized annotations (`uint8`, `int32`, `float32` …, 0.7.4) only *check*: a value
annotated `uint8` is still a 24-byte `Value`. A list of a million bytes takes
about 64 MB, where the data is 1 MB. Programs that hold numeric data — sensor
samples, pixels, protocol buffers, matrices — pay 3 to 24 times the memory and
the cache misses that come with it. The only compact storage is `buffer`, which
is bytes only and has its own methods.

## Decision

1. **Short names** for the sized types, as aliases of the long ones in
   annotations and conversions: `i8 i16 i32 i64 u8 u16 u32 u64 f32 f64`.
2. **A list annotated with a sized element type is stored with that size.**
   `let xs: list<u8> = []` holds a `Vec<u8>`; `list<f32>` a `Vec<f32>`;
   `list<i64>` a `Vec<i64>`; `list<char>` a `Vec<char>` (4 bytes per character,
   an element reads as a one-character `str`). A new `Value::Array(Rc<RefCell<TypedArray>>)`
   carries it (`Value` stays 24 bytes).
3. **It behaves as a list.** `typeof` says `list`; indexing, `len`, iteration,
   `push`/`pop`/`insert`/`remove`/`clear`, `first`/`last`, `contains`/`index_of`,
   `slice`, `reverse`, `sort`, display, equality and JSON are the list's. Methods
   that build new lists of any values (`map`, `filter`, `concat` …) return
   ordinary lists. Reading an element gives an `int` (a `float` for `f32`/`f64`);
   storing one checks it like the annotation does — an `int` out of range is the
   same type mismatch as `let x: u8 = 300`, a float into an integer array is an
   error, an `int` into a float array is converted.
4. **When it applies.** The value given to a `let`/`const` with such an
   annotation is packed when the declaration owns it — a literal, `[]`, or the
   fresh result of a call (reference count 1). A list that other variables still
   reference keeps its storage (packing it would split it in two): it is only
   checked, as before. Parameters and struct fields are checked, not packed (a
   later step can extend packing to them).
5. **Boundaries.** Native functions and methods of native objects that take
   lists receive an array as a list (a copy): the 200 places of the standard
   library that read lists keep working unchanged.
6. **The plain element types pack too (0.8.3).** `list<int>` and `list<float>`
   take 8 bytes per element and `list<bool>` one. Until 0.8.2 these annotations
   only documented the intent; now storing a value of another type is the type
   mismatch of the annotation, and an int stored in a `list<float>` reads back
   as a float (`[1, 2]` prints `[1.0, 2.0]`). This waited for step 7: packing
   them before would have moved their loops from compiled code to the
   interpreter.
7. **Compiled loops (0.8.3).** The JIT compiles loops over sized lists like the
   ones over ordinary lists. A list slot of a loop holds the address of the
   list's cell; bit 0 of that address (free: the cell is 8-aligned) says it is
   a sized list, so the generated code is the same and only the runtime helpers
   tell the two apart (`ListRef` in `runtime/jit.rs`). `xs[i]`, `len(xs)` and
   `for x in xs` read the compact storage in place (no `Value` is built, no
   copy of the list is made); `xs.push(v)` is a direct store when the kind
   always holds the value (an int into `list<int>` / `list<i64>`, a number into
   the float kinds, a bool into `list<bool>`) and a checked one otherwise (a
   `u8` out of range, a float into ints): the check runs before the push and,
   when it fails, the loop leaves before any effect of the iteration and the
   interpreter redoes it and reports the error — the mechanism every
   speculation of the JIT already uses. `for x in xs` over a sized list is its
   own loop kind (`LoopKind::ForArray`) with its own element helpers, so the
   helpers of ordinary lists are untouched.

## Alternatives considered

- *Packing every list whose elements happen to be small ints* (no annotation):
  silent changes of storage and of what a store may hold — rejected; the
  annotation is the programmer's statement.
- *Changing `Value::List`'s payload to a polymorphic storage*: every one of the
  ~230 places that borrow a `Vec<Value>` would change — rejected for the risk.
- *Extending `buffer` with an element type*: a different type with different
  methods than a list — rejected; the annotation should not change the API.

## Measured

- Memory, a million elements pushed (peak RSS above an empty program, 7.4 MB): plain list 52 MB,
  `list<u8>` 5 MB, `list<i32>` 10 MB, `list<f64>` 18 MB. A million characters: plain list 54 MB,
  `list<char>` 11 MB.
- Speed of the existing programs: every `bench/` program, against the code before this change
  built the same way, interleaved — no slower. The first version put the JSON writer's array case
  in the main match and slowed `stringify` by 6% and `parse` by 8% (codegen of the hot function);
  moving it to a `#[cold]` helper removed that.
- Loops, 0.8.3 (lite build, interleaved with the build of the commit before, minimum of 15 runs,
  a million elements): 21 passes of `s = s + xs[i]` — plain list 174 ms (285 ms before: the
  helper no longer clones the value), `list<int>` 255 ms (seconds before: interpreted); 40 passes
  of `for x in xs` — plain list unchanged, `list<int>` 0.49 s against 1.10 s for the plain list
  (no 24 MB snapshot per pass). Peak memory of that program: 78 MB plain, 42 MB `list<int>`,
  20 MB `list<u8>`.
- Still interpreted: `xs[i] = v` in a loop (for ordinary lists too), loops over `list<char>`.

## How it is checked

- `tests/spec/sized_loops.fg`: long loops (compiled) over every plain and sized kind — pushes,
  index reads, `for … in`, a function taking the list, and a push the kind refuses in the middle
  of a compiled loop (the same error, the same elements pushed before it).
- Spec tests on the three engines: every element type, `list<char>`, range errors, float/int
  rules, list methods, iteration, equality, JSON, natives receiving arrays,
  shared lists not packed.
- Memory: a million `u8` / `i32` / `f64` elements, peak RSS before and after.
- Speed: every `bench/` program against the previous release (no regression);
  element reads/writes and iteration over arrays against lists.
