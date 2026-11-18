# Tuples: `return a, b` is not a list

Status: **implemented** (0.8.1). A design note, as
[engineering-principles.md](engineering-principles.md) asks for a change to a
fundamental: the value model (`Value::Tuple`) and the semantics of `[v, err]`
and `?`.

## The bug

Until 0.8.0, `return a, b` built a **list**, and both idioms of errors as values
looked at lists:

- `const [v, err] = f()` split any list element by element;
- `expr?` treated any list of two elements whose second one was `nil` or an
  `error` as a value/error pair.

So a function that *succeeded* and returned a list was read as a pair. With
`const [rows, err] = db.query(…)` a query that found two rows gave the first row
as `rows` and the second row as `err` — the program then took a success for a
failure, or carried on with the wrong data. `json.parse("[1, 2]")` gave `1` and
`2`; `json.parse("[5, null]")?` gave `5` and silently dropped the list; a
function returning three values, `(int, str, error)`, did not propagate its
error with `?` at all. The tester's report counted this as the critical bug of
the first round (net §1) and two bugs of the second (L03, L04). Nothing in a
library could fix it: the information "these are several return values" was
lost when `return a, b` produced a list.

## The change

`return a, b` builds a **tuple**, a value of its own (`Value::Tuple(Rc<[Value]>)`,
`typeof` → `"tuple"`), immutable, which the spec already described (§8.4: "Multiple
values returned by a function are represented internally as tuples").

| | 0.8.0 | 0.8.1 |
| --- | --- | --- |
| `return a, b` | a list | a tuple |
| `const [v, err] = <tuple>` | — | its values |
| `const [v, err] = <error>` | `nil`, the error | `nil`, the error |
| `const [a, b] = [1, 2]` (list written there) | `1`, `2` | `1`, `2` |
| `const [v, err] = <any other list>` | split | **the list, `nil`** |
| `const [v, err] = <any other value>` | the value, `nil` | the value, `nil` |
| `x?` on an `error` | returns the error | returns the error |
| `x?` on a tuple ending in an `error` | (2 values only) | returns the error, any length |
| `x?` on a tuple ending in `nil` | (2 values only) | its other values (one, or a tuple) |
| `x?` on a list | split if it looked like a pair | the list, always |

What `?` returns from the function: a tuple of `nil`s and the error shaped like
the declared return type (`(T1, T2, error)` → three values), the error alone when
the declared type is not a tuple (`fn main(): error`), and `(nil, error)` when
there is no declaration.

A tuple prints as a list (`[3, nil]`, so existing output does not change), is
converted to a JSON array, supports `len`, indexing (also negative), `for x in`,
`for i, x in` and `==`; it cannot be changed (`t[0] = 1` is an error) and has no
methods. The annotation `tuple` matches a tuple, `list` a list, and `(T1, T2)` a
tuple of that length. A tuple returned by a `sync.Worker` function crosses the
thread as a tuple (the JSON array is marked); through the C API a tuple ending in
an error is the call's error and one ending in `nil` gives the host its value.

`fogu check` warns (W0005) where the new rule can surprise: destructuring a value
known to be a list that is not written in the declaration.

## What breaks

`const [a, b] = xs`, where `xs` is a list from anywhere but the declaration itself,
now gives `a = xs`, `b = nil`. A count over the code in the repository, its
libraries and the tester's 3 400 files: 1 184 destructurings, all but a handful the
value/error idiom on a call; the others split a list literal, which still works.
Code that split a list returned by a function must index it (`xs[0]`) or return
the values as a tuple (`return a, b`). Fogu is before 1.0 and its compatibility
policy allows the change; W0005 points at the cases `fogu check` can see.

## The five questions

1. *Performance?* `return a, b` allocates one `Rc<[Value]>` instead of an
   `Rc<RefCell<Vec<Value>>>` (one allocation fewer); destructuring copies the
   values as before. No benchmark of `bench/` returns tuples in a hot loop.
2. *Memory?* Smaller: no `RefCell`, no spare capacity.
3. *Isolation and concurrency?* Unchanged: a tuple is `Rc` and stays on its
   thread like every value; workers send it as marked JSON.
4. *Simplicity?* One variant more in `Value`; the rules of `[v, err]` and `?` are
   now stated by the type, in two functions shared by the interpreter and the VM
   (`interpreter::destructure`, `propagated_return`), instead of a guess on the
   shape of a list.
5. *Identity of the language?* It is what §8.4 of the spec said; errors as
   values keep their syntax.

Tests: `tests/spec/tuples.fg`, `tuples_immutable.fg` (three engines), the
resolver unit test for W0005.
