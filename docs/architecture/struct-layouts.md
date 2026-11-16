# Struct layouts

Status: **implemented** (0.8.1). A design note, as
[engineering-principles.md](engineering-principles.md) asks for a change to a
fundamental — here the representation of a value (`Value::Struct`).

## What changed

Until 0.8.0 an instance of a struct owned two hash maps keyed by `String`:

```text
StructInstance { type_name, module, embeds,
                 fields:  HashMap<String, Value>,
                 methods: HashMap<String, Rc<FnValue>> }
```

Every `new T(...)` built both: one `String` per field name, and for each
method a new `FnValue` (its name formatted, its parameter list copied). Every
`p.x` hashed the text `"x"` twice (is it a method? then the field) and every
`p.m()` hashed `"m"`.

From 0.8.1 the type owns what its instances share, and an instance is one
vector:

```text
StructLayout   { id, type_name, module, embeds,
                 names: Vec<String>,            // field names, in slot order
                 index: HashMap<String, u32>,   // field name → slot
                 methods: HashMap<String, Rc<FnValue>>,
                 ctor: Option<Rc<FnValue>> }
StructInstance { layout: Rc<StructLayout>, values: Vec<Value> }
```

The layout is built by the first `new` of a type and kept in its
`StructTypeValue`. `StructInstance` dereferences to its layout, so code that
reads `type_name`, `module`, `embeds` or `methods` did not change; fields are
read with `field(name)`, `field_mut(name)` and `fields()`.

The bytecode VM keeps one inline cache per instruction (`vm::Cache`):
`GetField` / `SetField` remember (layout id, slot), `CallMethod` remembers
(layout id, method, proto), `Call` remembers (function body, proto). A hit
costs one integer comparison instead of a hash of the name. Layout ids are
unique in the process and never reused, so a cache cannot match a different
type that happens to live at the same address.

## What did not change

- **Semantics.** Field order of an embed: the fields of each embedded type,
  then the embed itself (a field named after the type), then the struct's own
  fields; a field two embeds have keeps the value of the first; own methods
  replace embedded ones; a method wins over a field of the same name; a field
  the struct does not have cannot be assigned (`field 'y' does not exist on
  'P'`); `_private` names are checked before a cache is filled, and a cache
  belongs to one function, so to one module. `tests/spec/struct_layout.fg`
  and `struct_layout_error.fg` fix these, on the three engines.
- **The value size.** `Value` is still 24 bytes; `Value::Struct` is still
  `Rc<RefCell<StructInstance>>`.
- **Isolation.** A layout is `Rc`: it stays on its thread like every value.
  Workers build their own (they run their own copy of the program).
- **The REPL.** Redeclaring an embedded type builds a new layout for the
  types that embed it (the layout records the ids of the layouts it was built
  from and is rebuilt when they differ).

## The five questions

1. *Does it keep performance?* It improves it: see the measurements.
2. *Memory?* An instance drops two hash tables (at least 2 × 48 bytes of
   header plus a `String` and a `Value` per field, plus an `Rc<FnValue>` per
   method) for one `Rc` and a vector of 24 bytes per field.
3. *Isolation and the concurrency model?* Unchanged (above).
4. *Simplicity of the runtime?* One more type (`StructLayout`); `instantiate_struct`
   lost the per-instance method construction. The caches live in the VM only.
5. *Identity of the language?* Nothing a program can observe changes.

## Measurements

`bench/orders.fg` (a limit order book: two binary heaps of structs, 200k
orders), minimum of 5 interleaved runs, same machine and load, in ms:

| | tree-walker | `--engine=vm` |
| --- | ---: | ---: |
| before this change | 4821 | 1955 |
| after | 4564 | 1395 |

Built with the same flags, the default engine is not slower than 0.8.0 on any
program of `bench/` (minimum of 7 interleaved runs; `orders` 4833 → 4630,
`records` 79.9 → 72.6, the others within 2%).

The numbers of the release are in `bench/RESULTS_PROGRAMS.md`, written by
`fogu bench/run.fg programs`.
