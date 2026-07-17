# rust-toolbox

Generic, domain-agnostic Rust building blocks, factored out of a service
backend so they can be reused across projects instead of copy-pasted. Every
module is behind its own feature flag and has zero knowledge of any
particular project's domain.

| Module         | Feature  | What it is |
| -------------- | -------- | ---------- |
| `diesel_tools`  | `diesel` (plus `sqlite`/`mysql`/`postgresql`) | `impl_repository!`/`impl_save!` macros that generate diesel CRUD repositories from a schema module, entity type, and id column, plus a `Service`/`EntityService` pair (pool → tonic server) and pagination types (`Page`, `PageRequest`, `Sort`). |
| `tower_tools`   | `tower`  | Tower layers for request-id assignment/propagation and request tracing. |
| `tonic_tools`   | `tonic`  | A `ServiceError` trait plus `tonic::Status` ↔ `google.rpc.ErrorInfo` conversion, so a gRPC service can carry a structured `code`/`domain`/`metadata` error across the wire. |
| `axum_tools`    | `axum` (implies `tonic`) | Auth/RBAC (`User`, session issuing/verification, an in-memory basic-auth backend) and an `ApiError`/`Problem` HTTP error type that understands `tonic_tools`' structured errors. |
| `mail_tools`    | `mail`   | A reusable SMTP sender, configured explicitly rather than from the environment. |

## Using it

Not published to crates.io - pull it in as a git dependency:

```toml
[dependencies]
rust-toolbox = { git = "https://github.com/adrien-thebault/rust-toolbox.git", branch = "master", default-features = false, features = ["sqlite", "tower", "tonic"] }
```

Enable only the features you need - each one pulls in its own dependency
set (diesel, axum, tonic, lettre, ...).

## Development

```sh
cargo build
cargo fmt
cargo clippy --all-targets --features sqlite,axum,mail
./scripts/test-all.sh   # runs the suite once per feature combination below
```

`sqlite`/`mysql`/`postgresql` are mutually exclusive diesel backends, so
there's no single "test everything" feature set (`--all-features` fails to
compile) - `scripts/test-all.sh` runs `cargo test` once per combination
(`axum,mail,tower`; `sqlite`; `postgresql`; `mysql`) instead.

The `tests/` directory is a single harness (`tests/integration.rs`) whose
module tree mirrors `src/` - `tests/diesel_tools/repository/find.rs` tests
`src/diesel_tools/repository/find.rs`, and so on. The diesel tests are
backend-generic: SQLite runs them against a per-test in-memory database;
for the other backends, point `TOOLBOX_TEST_POSTGRES_URL` /
`TOOLBOX_TEST_MYSQL_URL` at a real server (the tests soft-skip when the
variable is unset, so a plain local run still passes). CI
(`.github/workflows/ci.yml`) runs the full matrix on every push/PR, with
postgres/mysql service containers backing those two.

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions (this repo
follows [Conventional Commits](https://www.conventionalcommits.org/) and
expects signed commits) and how the changelog is generated.

## License

[MIT](LICENSE)
