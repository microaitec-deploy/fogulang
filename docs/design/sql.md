# Design note: `fogu:sql`

Source: [`src/native/sql.rs`](../../src/native/sql.rs).

Multi-database SQL with [SQLx](https://github.com/launchbadge/sqlx) and its
`Any` driver, which picks the backend **at runtime** from the URL scheme, so
the same program works with SQLite, PostgreSQL and MySQL/MariaDB.

## API

```fogu
import sql from "fogu:sql"

// "sqlite://file.db" (created if missing) · "sqlite::memory:"
// "postgres://user:pass@host/db" · "mysql://user:pass@host/db"
const db = sql.open("sqlite://shop.db")?
print(db.driver)
db.exec("CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, name TEXT)")
db.run("INSERT INTO t (name) VALUES (?)", ["Fogu"])      // rows affected
for row in db.query("SELECT * FROM t WHERE id > ?", [0]) { print(row.name) }
const one = db.one("SELECT * FROM t WHERE name = ?", ["Fogu"])   // map or nil
db.begin()
db.commit()        // or db.rollback()
db.close()
```

## Notes

1. **Async under the hood**: SQLx is async and Fogu is synchronous, so each
   `Sql` object embeds a small Tokio runtime and uses `block_on`. The runtime
   **must live as long as the pool** (dropping it would orphan the sockets),
   so both are kept together and released by `close()`.
2. **Placeholders**: always write `?`; SQLx translates it to the backend syntax
   (`$1, $2…` on PostgreSQL), which keeps programs portable.
3. **SQLite files**: `?mode=rwc` is appended to `sqlite://` URLs so the file is
   created when it does not exist.
4. **Types**: the `Any` driver maps to a common subset — integers, floats,
   booleans and text. Other types (UUID, JSONB, dates, decimals) fall back to text.
5. **SQL Server** is not supported (removed from SQLx 0.7+); it would need a
   separate module built on `tiberius`.
6. **Errors**: connection and query failures are operational errors, returned
   as `error` values. Since `db.query` returns a list, propagate errors with `?`
   rather than destructuring.
