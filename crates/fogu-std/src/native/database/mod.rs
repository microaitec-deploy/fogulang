//! SQL, ORM, key-value stores, SurrealDB, Scylla, Redis, etcd (feature `database`).

#[cfg(not(target_family = "wasm"))]
pub mod sql;
#[cfg(not(target_family = "wasm"))]
pub mod orm;
#[cfg(not(target_family = "wasm"))]
pub mod kv;
#[cfg(not(target_family = "wasm"))]
pub mod surrealkv;
#[cfg(not(target_family = "wasm"))]
pub mod surrealdb;
#[cfg(not(target_family = "wasm"))]
pub mod scylla;
#[cfg(not(target_family = "wasm"))]
pub mod redis;
#[cfg(not(target_family = "wasm"))]
pub mod etcd;
