# SQLite FTS5 Spike

## Purpose

Validate legacy-equivalent FTS5 behavior and bundled/system SQLite packaging.

## Ownership

- The package owns disposable schema/index/search/rebuild tests only.

## Local Contracts

- Session JSON remains authoritative; the database is derived and rebuildable.
- Exclude system messages, bound text, redact canaries, and fail soft.
- Test both bundled and system feature selections locally where available.

## Verification

- `cargo test --locked`
- `cargo test --locked --no-default-features --features system-sqlite`

## Child DOX Index

No children.
