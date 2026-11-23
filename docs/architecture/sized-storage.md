# Design note: storage sized by the annotation

Status: implemented (0.8.2) · changes a fundamental (the value representation),
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
   library that read lists keep working unchanged. Compiled code (the JIT) does
   not compile loops over arrays yet; it leaves them to the interpreter.

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
- Not yet fast: loops over sized lists run in the interpreter (the JIT bails on them), so pushing
  a million elements takes 0.22 s against 0.08 s for a plain list the JIT compiles. Next step:
  typed element access in the JIT (and the VM's fast paths).

## How it is checked

- Spec tests on the three engines: every element type, `list<char>`, range errors, float/int
  rules, list methods, iteration, equality, JSON, natives receiving arrays,
  shared lists not packed.
- Memory: a million `u8` / `i32` / `f64` elements, peak RSS before and after.
- Speed: every `bench/` program against the previous release (no regression);
  element reads/writes and iteration over arrays against lists.
