# Design note: `fogu:kv`

Source: [`src/native/kv.rs`](../../src/native/kv.rs).

An embedded key-value store backed by [`redb`](https://github.com/cberner/redb).

## API

```fogu
import kv from "fogu:kv"

const db = kv.open("data.redb")?      // opens or creates the file
db.set("name", "Fogu")
db.set("list", [1, "two", 3])
print(db.get("name"), db.has("name"), db.keys(), db.len())
db.delete("name")
db.clear()
db.close()
```

## Notes

1. **Value format**: every value is stored as a string prefixed with `\u{0}`
   and its type (`\u{0}int:42`, `\u{0}str:…`), which avoids collisions with
   strings that happen to start with `int:`. Lists are serialized as simple JSON.
2. **Supported types**: `nil`, `bool`, `int`, `float`, `str`, `list`. Maps are not
   supported directly — serialize them with `json.stringify`.
3. **Synchronous**: `redb` operations block the current thread; for scripts and
   automation this is fast enough. `redb` supports concurrent reads while a
   write is in progress (MVCC).
4. **Single file**: the whole database is one `.redb` file; back it up by copying it.
5. **`close`** releases the file lock; if it is not called, the database is
   closed when the object is dropped.
6. **Errors**: opening or I/O failures are operational errors, returned as
   `error` values (`kv.open(path)?`).
