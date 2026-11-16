# Map keys

Status: **implemented** (unreleased, after 0.8.0). A design note, as
[engineering-principles.md](engineering-principles.md) asks for a change to a
fundamental — here the representation of a value (`Value::Map`).

## What changed

A Fogu map was a hash map keyed by `String`:

```text
Value::Map(Rc<RefCell<HashMap<String, Value>>>)
```

so every key of every map owned a heap allocation. A literal such as
`{ kind: "num", value: v, text: "" }` allocated and copied three key texts each
time it was evaluated, on top of the table itself. A profile of
`bench/parser.fg` on the bytecode VM attributed 36% of the time to
`String::clone`.

The key is now `Str`, the type `Value::Str` already uses (`runtime/fstr.rs`):

```text
Value::Map(Rc<RefCell<HashMap<Str, Value>>>)
```

A key of up to 15 bytes lives inside the key (no allocation to create, copy or
drop it); a longer one is a reference-counted text, so copying it is a counter
increment. Three places hold keys ready to be copied into a new map:

- the AST: `Expr::Map(Vec<(Str, Expr)>)`, read by `Interpreter::eval_map`;
- the bytecode: `Proto.keys: Vec<Vec<Str>>`, read by `Op::NewMap`;
- the JIT: interned `&'static str` keys, turned into `Str` by `h_v_map_set`.

`m[k] = v` with a string `k` moves the `Str` of the value into the table instead
of copying its text, and `map.has/get/set/remove` take the key by reference
count.

In the standard library every function that receives or builds a Fogu map
(`opts(...)` helpers, parsers, rows of a query, headers given to a handler) uses
`HashMap<Str, Value>`. Code that looks a key up by text did not change:
`Str: Borrow<str>` and `Str` hashes exactly like `str`
(`fstr::tests::hashes_like_str`), so `m.get("name")` finds the entry.

## What did not change

- **Semantics.** Lookup, insertion, removal, `len`, equality, copies and the
  order of iteration: `for k in m`, `for k, v in m`, `keys()`, `values()`,
  `entries()`, `print` and `json.stringify` sort the keys, and `Str` orders as
  `str` does (by bytes). `tests/spec/map_keys.fg` fixes this for keys of 0, 15,
  16 and more bytes and non-ASCII keys, on the three engines; its expected
  output is what 0.8.0 prints.
- **The value size.** `Value` is still 24 bytes and `Value::Map` is still one
  `Rc`. An entry of the table shrinks from 48 to 40 bytes (key 24 → 16).
- **Isolation.** A long `Str` is an `Rc`: like every value it stays on its
  thread. Native modules that move data between threads keep doing so as
  `String`, `Vec<u8>` or `serde_json::Value` and build the map on the
  interpreter thread; the compiler enforces it (`Str` is not `Send`).
- **What is not a Fogu map.** `ModuleValue.exports`, the globals of
  `NativeContext`, `StructLayout.index` / `methods`, and the internal tables of
  native modules (HTTP headers as `(String, String)`, ORM models, the shards and
  procedures of `fogu:web`) are still keyed by `String`.

## The five questions

1. *Does it respect the execution model?* Yes: nothing about tasks, the
   scheduler or the engines changes; the three engines build the same maps.
2. *Isolation and efficiency?* Isolation is unchanged (above). A map entry is 8
   bytes smaller and a key of up to 15 bytes no longer has a heap block.
3. *Performance?* Kept or improved on every program measured (below).
4. *Is the complexity justified?* No new type and no new code path: one type
   parameter changed, and the conversions at the borders of native code
   (`String` → `Str` when a map is built from outside data, `to_string()` where
   a native API needs a `String`).
5. *Identity of the language?* Nothing a program can observe changes.

## Measurements

Wall time in ms, minimum of 7 interleaved runs, same machine and load. The
baseline is 0.8.0 built with the same flags (it has no VM).

| program | 0.8.0 | default engine | change | `--engine=vm` |
| --- | ---: | ---: | ---: | ---: |
| `bench/parser.fg` | 4221 | 4062 | −3.8% | 1915 |
| `bench/orders.fg` | 4578 | 4516 | −1.4% | 1361 |
| `bench/wordfreq.fg` | 1546 | 1521 | −1.6% | 783 |
| `bench/maps.fg` | 140 | 111 | −20.7% | 113 |
| `bench/records.fg` | 76 | 68 | −10.5% | 68 |
| `bench/strings.fg` | 31 | 31 | 0% | 31 |
| `bench/sort.fg` | 23 | 21 | within noise | 23 |
| `bench/fib.fg` | 14 | 13 | within noise | 14 |

Against the same branch before this change (default / VM): `parser`
4248 / 2057 → 4062 / 1915, `orders` 4564 / 1395 → 4516 / 1361, `wordfreq`
1582 / 827 → 1521 / 783. Those "before" numbers come from an earlier session,
not from interleaved runs with the new binary, so read the differences of a few
percent with care. The VM gains about 7% on `parser`, far less than the 36% the
profile attributed to `String::clone`: the copies of map keys were only part of
it.
