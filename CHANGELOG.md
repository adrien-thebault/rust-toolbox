# Changelog

All notable changes to this project are documented here, generated from
Conventional Commit history by [git-cliff](https://git-cliff.org/).

## [0.2.1] - 2026-09-21

### Bug Fixes

- *(cluster,web)* Make idempotency claims ownership-safe (04811de)
- *(db)* Implement backend-specific migration locking (7d4424f)
- *(auth)* Refresh JWKS independently of requests (c8b6512)
- *(server,grpc,web)* Enforce bounded serving limits (aaba3e6)
- *(web)* [**breaking**] Replace trusted hop counts with proxy networks (81cbae6)
- *(ci)* Allow the transitive toml_datetime version split (c4a07f2)
- *(template)* Keep generated imports formatted (730ae13)

### Documentation

- Update crate READMEs (d0556c8)

### Features

- *(schedule)* Bound concurrent job execution (37d4c7e)
- *(template)* Add MariaDB backend support (1a83241)

### Miscellaneous

- *(workspace)* [**breaking**] Raise MSRV to Rust 1.98 (38dd1b2)
- *(ci)* Pin actions and group dependency updates (384597d)

### Refactor

- *(web)* [**breaking**] Simplify realtime hub configuration (dcc2833)
- *(pagination)* [**breaking**] Make page requests valid by construction (ff50fc5)
- *(test)* Specialize temporary databases for SQLite (66896f2)
- *(toolbox,examples,template)* Adopt the facade crate (7e062be)
- *(workspace)* Simplify imports and remove dead dependencies (c523e49)

## [0.2.0] - 2026-09-14

### Bug Fixes

- *(examples,templates)* React to the named SSE events on the tester page (772e50a)
- *(workspace)* Exclude templates from workspace member discovery (c5237ba)
- *(grpc)* Report a misconfigured retry policy instead of skipping or panicking (6930184)
- *(schedule)* Isolate job panics and anchor FixedDelay on actual completion (5b3caaa)

### CI

- Add workspace lints, dependency bans, release-plz and a CI matrix (27b7cda)
- Fail the build when the committed OpenAPI spec drifts (5ffd7a6)
- *(template)* Pin clippy and build to stable in the generate job (b81929d)

### Documentation

- Add generated CHANGELOG.md (936bf9a)
- *(examples)* Add a gRPC cluster example as two independently deployable crates (72e7278)
- Rewrite the README and CLAUDE.md, and add a README to every crate (fa15a0f)
- *(contributing)* Add the three questions and the per-crate commit scopes (a9cfd60)
- Soften the capabilities rule, log recent deletions (470b405)
- Log the capability-machinery deletion (5e46922)
- Log the Ticket-to-auth-token replacement (ccf3948)
- *(server)* Drop the stale BackendArgs reference (6326596)
- *(schedule)* Drop the stale deployment-guard reference (2d1e79e)
- Fix intra-doc links that fail cargo doc with -D warnings (eeaa9db)
- Pare the READMEs down to what each crate contains (548b07b)

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
- *(auth)* Forwarded-principal provider + shared-secret trust for ForwardedIdentityProvider (88b6f97)
- *(grpc)* Gate the health probe on readiness checks (7dca06c)
- *(template)* Generate an OpenAPI spec with a CI drift check (3a27a98)
- *(auth)* Mint an access token with a caller-supplied TTL (48c5fd8)
- *(examples)* Close the backend auth bypass and demonstrate more of the toolbox (60ff2aa)
- *(templates)* Mirror the example's auth, health, schedule and realtime work (550cc86)
- *(server)* Poll_check dependency probe; principal and latency_ms on request spans (4694593)
- *(db)* Db::ping and Db::is_live; drop the health feature (570617a)
- *(grpc)* Propagate the caller's trace context to the backend (3364d41)
- *(grpc)* Poll_health client probe, a secret-gated health service, and origin-side error logging (a8ed138)
- *(schedule)* Log job start/finish with next_run_at; job() takes a state arg (1864475)
- *(web)* Open the SSE stream with a comment so a buffering proxy doesn't stall the connect (e842d75)
- *(examples,templates)* Stream todo events from the backend; wire the new health/schedule/logging (3b61e85)
- *(examples,templates)* Serve the OpenAPI spec live, move search onto the service (1881b44)
- *(macros,core)* Add #[derive(ServiceError)] (60487c8)
- *(db,test)* Add sqlite_backend!/postgres_backend!/mysql_backend! and migrated_db/migrated_conn (d585497)
- *(server,grpc,web,schedule)* Rebuild startup around ServerBuilder and Server (e91b932)
- *(web)* Add Idempotent::json to replay the claim/run/record dance (215c62b)

### Miscellaneous

- Drop the incubator; toolbox-files lives outside this repo now (13d04bc)
- Move openapi.sh into examples/grpc-cluster, drop example.sh (f84e535)
- *(examples)* Update Cargo.lock for the new dependencies (e4ca574)
- *(examples)* Commit a working .env with a real admin password (5eaaac9)
- Drop gitignore entries for deleted rewrite planning docs (efcc8dc)

### Refactor

- [**breaking**] Replace the single legacy crate with a cargo workspace (ed89e0e)
- *(auth)* Unify token verification in JwtIdentityProvider; doc private items workspace-wide (ff430ab)
- *(cluster)* Merge bus into event, rename kv/lock families, move clock to toolbox-schedule (ca7e22a)
- *(core)* Sort as a sibling module, UncheckedPageRequest rename, ABOUT_BLANK re-export (171456b)
- *(db)* Merge migration lock into migrate, fold sort into pagination, DbPool rename (6f57689)
- *(grpc)* Client/server split, drop Discovery, forwarded-identity layers (a7f7117)
- *(macros)* Fold dialect into autoincrement, rework the entity derive doc (7588217)
- *(schedule)* Drop Spring analogies, ScheduledJob->Job, split builder, name-keyed run_now (4a62afe)
- *(server)* Flatten deadline/trace_context layers, serve->startup, MakeTracedSpan (a8b87b5)
- *(web)* Split auth/openapi, rename modules, mirror tests to src, drop doc archaeology (2b873ec)
- *(web)* ClientIpTrust for TrustedHops, RateLimit for LoginLimit, drop leftover Defaults (c726275)
- *(test)* Fold BackendAddrs into TestCluster, TestApp -> TestGateway (2fcf333)
- *(grpc)* Collapse serve into one async fn, apply grpc_stack, fail health on drain (848668e)
- *(server)* Lift ReadinessCheck out of toolbox-web (53eb85e)
- *(db)* Cut Now, hardcode chrono in the derive's timestamp autofill (273bad3)
- *(cluster)* Drop toolbox-cluster-postgres and the deployment guard (40d5ed3)
- *(cluster)* Trim capability negotiation into hard contract requirements (80c6060)
- *(server)* Fold readiness and drain into one LifecycleHandle (12ec4a5)
- *(grpc)* Rename Backoff to BackoffConfig (b52745d)
- *(web)* Cut CaptchaProvider, apply the Config/Policy convention, split sse.rs (c37cee8)
- *(web)* Replace the KV-backed Ticket with a short-lived auth token (224bf24)
- *(server)* Move bind to the crate root, fold StartupConfig into lifecycle (81cc4c7)
- *(server)* Rename readiness to health, ready.rs to health.rs (9325a2d)
- *(auth)* Rename ForwardedPrincipal to AssertedPrincipal (7697215)
- *(cluster)* Rename InProcess adapters to InMemory (f63bb5f)
- *(web)* Rename openapi dump to serialize (462215c)
- Drop unnecessary paths and tracing:: prefixes (8dadb3b)
- *(examples,templates)* Move search and the sweep back to the entity; sweep returns ids (6156af0)
- *(examples,templates)* Adopt ServiceError, sqlite_backend!, and ServerBuilder (0449934)
- [**breaking**] Split toolbox-core into toolbox-error and toolbox-pagination (e0122af)
- *(examples,template)* Rename grpc-cluster/service to todo/template (139c418)

### Testing

- *(macros)* Add trybuild cases for every #[derive(Entity)] misuse (06ad9bd)
- Mirror every crate's test module tree to src (except toolbox-cluster-postgres) (0f23eb7)
- *(db)* Mirror sqlite pragma tests, drop dead is_sqlite (a82902f)
- *(server)* Mirror the Health enum test into lifecycle/health.rs (4aa61bb)
- *(grpc)* Cover ClientInterceptor, which had no tests at all (4a48197)
- *(web)* Cover AuthState's default refresh methods (5ff3931)
- Add behaviour-focused tests to lift workspace coverage (4c86296)

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

