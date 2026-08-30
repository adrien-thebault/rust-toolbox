# toolbox-db

Diesel, with the blocking path out of reach.

Every diesel call reachable from `async` code goes through a closure this crate
runs on a blocking thread. The one escape hatch is called `blocking_conn` so it
is visible in review.

| Module | What it holds |
|---|---|
| `db` | `Db<C>` and `DbBuilder<C>`: `run`, `query`, `transaction`, and their `_named` spans |
| `entity` | the `Entity` and `Now` traits the derive implements and relies on |
| `pagination` | `Paginate`, which composes onto **any** diesel query, and the sort-field allowlist check that guards `ORDER BY` |
| `migrate` | migrations, serialised across replicas by the backend's own session lock |
| `sqlite` | connection pragmas |
| `args` | the clap arguments, next to the type they configure |

## No backend features

This crate declares no `sqlite`, `postgres` or `mysql` feature. It is generic
over `C: R2D2Connection`, and the entity names the backend as a **type**. Three
things follow: `--all-features` works, one process can hold two pools of
different backends, and a feature enabled elsewhere in a workspace cannot
change which backend another crate compiles against.

Pagination uses `COUNT(*) OVER ()` in the same statement as the rows, so the
total cannot disagree with the page - which a separate `COUNT` query can, and
does, under concurrent writes.
