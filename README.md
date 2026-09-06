# rust-toolbox

A collection of small, domain-agnostic Rust crates, kept in one workspace so
they are not copy-pasted between projects. Each is a building block a service
backend tends to need - a database pool, error types, middleware, auth,
scheduled tasks - with nothing specific to any one project in it.

## Install

**Not published to crates.io and will not be.**

```toml
[dependencies]
toolbox = { git = "https://github.com/adrien-thebault/rust-toolbox.git", tag = "…", features = ["db", "web"] }
```

## The crates

Each has its own README describing its modules.

| Crate                                                   | What it is                                                                                           |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| [`toolbox`](crates/toolbox/README.md)                   | the facade: one dependency line, a feature per crate                                                 |
| [`toolbox-core`](crates/toolbox-core/README.md)         | the shared error, problem and pagination vocabulary                                                  |
| [`toolbox-macros`](crates/toolbox-macros/README.md)     | the `#[derive(Entity)]` proc macro                                                                   |
| [`toolbox-db`](crates/toolbox-db/README.md)             | a diesel pool that keeps blocking calls off the async runtime, with pagination and locked migrations |
| [`toolbox-cluster`](crates/toolbox-cluster/README.md)   | the event-bus, key-value and lock traits a multi-replica deployment coordinates through              |
| [`toolbox-schedule`](crates/toolbox-schedule/README.md) | scheduled tasks that run once per cluster, and the clock port                                        |
| [`toolbox-server`](crates/toolbox-server/README.md)     | the transport-agnostic runtime: trace context, middleware stacks, deadlines, graceful drain          |
| [`toolbox-auth`](crates/toolbox-auth/README.md)         | principals, roles, identity providers and JWT sessions, with no transport dependency                 |
| [`toolbox-web`](crates/toolbox-web/README.md)           | axum building blocks: one error shape, extractors, health, rate limiting, OpenAPI, SSE               |
| [`toolbox-grpc`](crates/toolbox-grpc/README.md)         | tonic building blocks: status conversion, service clients, identity layers, serving                  |
| [`toolbox-test`](crates/toolbox-test/README.md)         | throwaway databases, an in-process gateway, a problem-document assertion; dev-only                   |

## Template

```sh
cargo generate --git https://github.com/adrien-thebault/rust-toolbox.git templates/service
```

Two prompts: whether to include an HTTP gateway, and which database backend.

## Development

Commands and conventions are in [CLAUDE.md](CLAUDE.md) and
[CONTRIBUTING.md](CONTRIBUTING.md). `toolbox-grpc` needs `protoc`. Each crate
has one integration harness at `tests/integration.rs` whose module tree
mirrors `src/`.

## License

[MIT](LICENSE)
