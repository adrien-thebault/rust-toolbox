# Changelog

All notable changes to this project are documented here, generated from
Conventional Commit history by [git-cliff](https://git-cliff.org/).

## [0.2.0] - 2026-08-28

### CI

- Add workspace lints, dependency bans, release-plz and a CI matrix (27b7cda)
- Fail the build when the committed OpenAPI spec drifts (5ffd7a6)

### Documentation

- Add generated CHANGELOG.md (936bf9a)
- *(examples)* Add a gRPC cluster example as two independently deployable crates (72e7278)
- Rewrite the README and CLAUDE.md, and add a README to every crate (fa15a0f)
- *(contributing)* Add the three questions and the per-crate commit scopes (a9cfd60)

### Features

- *(core)* Add ErrorKind, ServiceError, RFC 9457 Problem and Page (9011aa2)
- *(macros)* Add #[derive(Entity)] generating inherent CRUD methods (ba61a53)
- *(db)* Add Db<C> over r2d2, window-function pagination and locked migrations (c0312ef)
- *(cluster)* Add the event bus, key-value, lock and clock traits with local adapters (910df99)
- *(server)* Add W3C trace context, layer stacks, deadlines and graceful drain (fc73041)
- *(grpc)* Add status conversion, backend clients, discovery and pagination.proto (c4a60d7)
- *(auth)* Add principals, providers, sessions, refresh tokens, OIDC and argon2 passwords (562321b)
- *(web)* Add the axum layer: errors, extractors, health, rate limiting and realtime (2bbdf10)
- *(test)* Add temp databases, TestApp, TestCluster and the assert_problem macro (5030a17)
- *(files)* Add a file service in the incubator, outside the toolbox (2d6198c)
- *(cluster)* Add the PostgreSQL outbox, key-value store and leased locks (5977df5)
- *(schedule)* Add a cluster-safe scheduler with cron triggers in UTC (1a706ba)
- *(toolbox)* Add the facade crate with feature re-exports and toolbox::deps (8d95831)
- *(template)* Consolidate both service templates behind one gateway prompt (d39ade8)

### Refactor

- [**breaking**] Replace the single legacy crate with a cargo workspace (ed89e0e)

### Testing

- *(macros)* Add trybuild cases for every #[derive(Entity)] misuse (06ad9bd)

## [0.1.0] - 2026-07-20

### Documentation

- Clean-up docs (18d8363)

### Features

- Initial commit (8d3ebb3)
- Tonic/tower tools to easily propagate the request id between hops (3e4e10c)
- *(axum_tools)* Add rate-limit module for per-IP request throttling (b34b899)
- *(diesel_tools)* Add SqlitePragmas connection customizer (c7b360b)

### Miscellaneous

- Add .vscode default feature set (28d49de)

### Refactor

- Remove unnecessary clones (a044424)

### Testing

- Quote rank in the mysql DDL to fix CI (384f5fc)

