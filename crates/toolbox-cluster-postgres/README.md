# toolbox-cluster-postgres

The shared adapters.

PostgreSQL implementations of the cluster traits in `toolbox-cluster`. A trait
with only a local adapter has never been tested against the thing it
abstracts.

| Module | What it holds |
|---|---|
| `outbox` | `OutboxBus`, the transactional outbox |
| `key_value` | `PostgresKeyValue`, with `DELETE .. RETURNING` for an atomic take |
| `lock` | `PostgresLocks`, held as leases |

PostgreSQL and nothing else, deliberately: if you run more than one replica you
already have it, so this is the zero-new-infrastructure option. Redis, NATS and
Kafka are a day each afterwards, because by then an adapter implements a trait
whose contract is already pinned by tests.

## Why a separate crate

These adapters name `diesel::pg::Pg` concretely, so a feature on
`toolbox-cluster` would enable `diesel/postgres` - and cargo unifies features
across a workspace. Every sibling crate would then compile PostgreSQL support
and need `libpq` present, including a gateway that only ever talks to SQLite.

## Leases, not advisory locks

`pg_advisory_lock` belongs to a **session**, and a pooled connection is handed
back between statements - so the next caller can get the same session, where
advisory locks are re-entrant, and take a lock somebody else holds. A lease row
is owned by whoever wrote it, and expires on its own when a holder hangs.
