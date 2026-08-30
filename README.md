# rust-toolbox

Generic Rust building blocks, extracted from a service backend so they are not
copy-pasted between projects.

## Install

**This is not published to crates.io and will not be.** Depend on it by git,
and **pin a tag** - `branch = "master"` means the next `cargo update` can bring
a breaking change nobody chose.

```toml
[dependencies]
toolbox = { git = "https://github.com/adrien-thebault/rust-toolbox.git", tag = "v0.4.1", features = ["db", "web"] }
```

Take the facade. Each feature pulls in the crate of the same name, and the
dependency order means enabling `web` also enables `server`, `cluster`, `auth`
and `core`. Depending on the individual crates instead is possible and compiles
a little less, but it is a version to bump per crate for a saving you will not
measure.

## The crates

Each has its own README explaining what its modules do.

| Crate | What it owns |
|---|---|
| [`toolbox`](crates/toolbox/README.md) | the facade: features, prelude, `toolbox::deps` |
| [`toolbox-core`](crates/toolbox-core/README.md) | `ErrorKind`, `ServiceError`, RFC 9457 `Problem`, `Page`/`Sort` |
| [`toolbox-macros`](crates/toolbox-macros/README.md) | `#[derive(Entity)]` |
| [`toolbox-db`](crates/toolbox-db/README.md) | `Db<C>`, pagination, locks, migrations, pragmas |
| [`toolbox-cluster`](crates/toolbox-cluster/README.md) | the cluster traits, their local adapters, the deployment guard |
| [`toolbox-cluster-postgres`](crates/toolbox-cluster-postgres/README.md) | the shared adapters: outbox, key-value, leases |
| [`toolbox-schedule`](crates/toolbox-schedule/README.md) | scheduled tasks that run once per cluster, plus the `Clock` port |
| [`toolbox-server`](crates/toolbox-server/README.md) | trace context, layer stacks, deadlines, graceful drain |
| [`toolbox-auth`](crates/toolbox-auth/README.md) | principals, roles, identity providers, `PrincipalMapping`, JWT sessions with stateless refresh |
| [`toolbox-web`](crates/toolbox-web/README.md) | errors, extractors, health, rate limiting, OpenAPI, SSE |
| [`toolbox-grpc`](crates/toolbox-grpc/README.md) | status conversion, backend clients, discovery, serving |
| [`toolbox-test`](crates/toolbox-test/README.md) | throwaway databases, an in-process gateway, `assert_problem!` |

Dependency order is `core -> db -> cluster -> server -> {web, grpc}`.

### What you have to declare yourself

`diesel`, `serde`, `prost` and `diesel_migrations` cannot be re-exported: their
derives emit absolute paths that only resolve when the crate is a direct
dependency under that exact name.

```toml
diesel = { version = "2.3", features = ["r2d2", "chrono", "sqlite"] }
diesel_migrations = "2.3"
serde = { version = "1.0", features = ["derive"] }
prost = "0.14"
tonic-prost = "0.14"
```

Everything whose *types* cross the boundary - `axum`, `tonic`, `http`, `tower`,
`tower-http` - comes through `toolbox::deps`, so you link the same version the
toolbox's own types come from.

## Starting a new service

```sh
cargo generate --git https://github.com/adrien-thebault/rust-toolbox.git templates/service
```

Two prompts: whether to include an HTTP gateway, and which database backend.
All four combinations are generated and built in CI on every commit, so a fresh
project passes its own checks.

You get, without writing any of it: graceful shutdown with the drain delay that
stops a rolling deploy dropping requests, `/health` and `/ready`, a request
timeout and body limit, W3C trace context on every log line and error body, RFC
9457 errors with 5xx detail redacted, a startup guard that refuses a
single-replica adapter under `DEPLOYMENT=clustered`, gRPC health and reflection,
and locked migrations.

`examples/grpc-cluster` is the same thing as two crates you can deploy
separately - `grpc/todo` owns the database, `web` owns authentication. It is
what the template generates, so the two cannot drift: `docker compose up` in
that directory runs both services, `./scripts/example.sh` runs the end-to-end
test, and CI runs that test on every commit.

## Choosing a database backend

`#[entity(backend = ...)]` names a **type**, not a cargo feature. Point it at
one alias:

```rust,ignore
pub type Backend = diesel::sqlite::Sqlite;
```

Swapping to PostgreSQL is that line plus the connection URL. `toolbox-db`
declares no backend feature, which is why `--all-features` works and why one
process can hold a PostgreSQL pool and a SQLite pool at once.

## Deployment modes

Set `DEPLOYMENT=single` or `DEPLOYMENT=clustered`. Every stateful adapter
declares whether its state is shared, and `serve_http`/`serve_grpc` check at
startup: an adapter that would be **incorrect** on several replicas refuses to
start and names the variable to change; one that would merely be **degraded**
warns.

## Semver policy

One version across the workspace, cut from one tag by `release-plz`.
`cargo-semver-checks` runs on every release PR. Re-exported upstream types are
part of the public API, so an upstream major bump is a breaking change here.
Every breaking release ships a migration guide. Tags are the unit of
consumption.

## Development

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo hack --feature-powerset --depth 2 check -p toolbox-db -p toolbox-web -p toolbox-auth -p toolbox
cargo deny check
```

`toolbox-db`'s tests compile all three diesel backends at once, so building
them needs `libpq` and `libmysqlclient`. `protoc` is needed for `toolbox-grpc`.
Cross-backend tests soft-skip unless `TOOLBOX_TEST_POSTGRES_URL` or
`TOOLBOX_TEST_MYSQL_URL` is set.

Each crate has one integration harness at `tests/integration.rs` whose module
tree mirrors `src/`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions and how the
changelog is generated.

## License

[MIT](LICENSE)
