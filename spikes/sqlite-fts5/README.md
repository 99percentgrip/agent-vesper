# Disposable SQLite FTS5 spike

This package validates the derived legacy-equivalent index with either
rusqlite's bundled SQLite or an available system SQLite. It is not a session
store.

```text
cargo test --locked
cargo test --locked --no-default-features --features system-sqlite
```
