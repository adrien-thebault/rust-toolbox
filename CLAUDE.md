# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## What this is

A cargo workspace of generic Rust building blocks - a diesel entity derive, a
pool that keeps blocking calls off the async runtime, tower layer stacks, RFC
9457 errors, axum extractors, tonic status conversion, the traits a cluster
is wired from, and scheduled tasks. It was extracted out of a service backend because none of it is
specific to that project's domain; it is consumed as a git dependency by that
repo, and meant to be reusable by others.

Nothing here may depend on, or encode assumptions about, any specific
downstream project. If a change only makes sense in terms of one consumer's
domain model, it belongs in that consumer's own crate, not here.

## Architecture

Dependency order, which is the thing to get right first:

```
core -> db -> cluster -> server -> {web, grpc}
```

| Crate | Owns |
|---|---|
| `toolbox-core` | `ErrorKind`, `ServiceError`, `ErrorInfo`, RFC 9457 `Problem`, `Page`/`PageRequest`/`Sort`. serde and thiserror only |
| `toolbox-macros` | `#[derive(Entity)]`. Proc-macro crate, so necessarily separate |
| `toolbox-db` | `Db<C>`, `DbError`, `Entity`/`Now`, `Paginate`, locked `migrate()`, `SqlitePragmas`, `DatabaseArgs` |
| `toolbox-cluster` | `CloudEvent`; the `EventBus`/`KvStore`/`LockManager` traits, their local adapters, and the deployment guard |
| `toolbox-cluster-postgres` | the shared adapters: outbox, key-value, leased locks |
| `toolbox-schedule` | scheduled tasks that run once per cluster, plus the `Clock` port (`system`/`manual`) |
| `toolbox-server` | trace context, `http_stack`/`grpc_stack`/`realtime_stack`, deadlines, shutdown, telemetry, `ServerArgs`/`DeploymentArgs`, `bind` |
| `toolbox-auth` | `Principal`, `Role`, `IdentityProvider`/`ProviderRegistry`, `PrincipalMapping`, `JwtIdentityProvider` (mints HS256 sessions + stateless refresh, verifies a bearer - its own, JWKS or a public key). Depends only on `toolbox-core`, so a backend validates a token without compiling axum or the cluster traits. No OIDC redirect flow, no identity federation |
| `toolbox-web` | `ApiError`, `Authenticated<R>`, `ValidJson`, `PageQuery`, `Idempotent`, health, CORS, rate limiting, `client_ip`, OpenAPI, SSE, `serve_http` |
| `toolbox-grpc` | `to_status`/`from_status`, `client()`, `pagination.proto`, `shared_secret_layer`/`identity_layer`, health, reflection, `serve` |
| `toolbox-test` | `temp_db`, `TestApp`, `TestCluster`, `assert_problem!`. Dev-only |
| `toolbox` | facade features, prelude, `toolbox::deps` |

Two boundaries that are easy to break by accident:

- **`toolbox-web` must not depend on `toolbox-grpc`.** The seam is
  `ErrorInfo`: grpc owns `Status -> ErrorInfo`, web owns
  `ErrorInfo -> ApiError`. There is no exception and no feature that adds one:
  code needing both transports belongs in the consumer, not here.
- **`toolbox-db` declares no backend feature.** It is generic over
  `C: R2D2Connection`, and the entity names the backend as a *type*.

Anything with a domain of its own goes in `incubator/`, which is outside the
workspace and has its own README. That is where `toolbox-files` went.

## Commands

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo +nightly fmt --all           # group_imports is nightly-only
cargo hack --feature-powerset --depth 2 check -p toolbox-db -p toolbox-web -p toolbox-auth -p toolbox
cargo deny check
./scripts/changelog.sh
./scripts/example.sh                 # the gRPC cluster example, end to end
./scripts/openapi.sh                 # regenerate the example's committed spec
./scripts/hash-password.sh           # an argon2 hash in PHC format
```

Needs `protoc`, and `libpq` + `libmysqlclient` for `toolbox-db`'s
three-backend test.

`examples/grpc-cluster` and `templates/service` are the **same tree**: same
crate layout, same modules, same two `main.rs`. The template adds the
placeholders, the Dockerfile and the compose file; the example adds the
end-to-end test. A change to one belongs in both, and the example's test is
what proves the shape still works. Neither ships a second binary - a tool that
prints a file is an `[[example]]`, not a `[[bin]]`.

Each crate carries its own `README.md` describing its modules; the top-level
README links them.

## Conventions to preserve

**Three questions before adding anything**, in order:

1. **Does a crate already do this?** Read the top two results' docs, not their
   names. Then say what the module adds: it unifies an error type across a
   boundary; it encodes a decision you would otherwise re-make wrong; it
   removes a trap the underlying crate makes easy; it bridges two crates that
   do not know about each other; or it is invoked identically in every project
   *and* the underlying API needs more than ten lines of setup. If the answer
   is "it is nicer than calling X directly", **stop**.
2. **Does a standard already define this?** For any format, header, envelope
   or wire contract. If one exists and fits, implement it even when bespoke
   would be 20% less code. If it does not fit, write why in the doc comment.
3. **Does it hold state across requests?** Then it is a **trait with
   adapters**, not a struct: a local adapter, at least one shared adapter,
   capabilities declared rather than assumed, and unsupported operations
   failing at wiring time rather than at runtime.

**Write the answer into the module's doc comment.** One sentence, straight
after the summary line, saying why the module is there. No "Why this exists:"
preamble - just the reason. It is what lets the decision be re-opened in two
years when the ecosystem moves.

**One datetime library, named once per project.** `chrono`, through the
consumer's own `crate::Timestamp` alias. Use it where it is the clearest thing
to use - a scheduler's next fire time is a `DateTime`, not an integer - and
keep it out of a signature where a plain `Duration` or an IANA name string says
the same thing. The point is that swapping it later is one alias, not that it
is banned.

**A trait needs two implementations or `dyn`.** Otherwise write a function.
This is the rule that deleted `Controller`, `EntityService`, `DatabaseService`,
`Find`, `Save`, `Delete` and `Repository`.

**Never block the async runtime.** Any diesel call reachable from an `async fn`
goes through `Db::run`, `Db::query` or `Db::transaction`. The one escape hatch
is named `blocking_conn()` so it shows up in review.

**Backends and datetimes are named exactly once per project** -
`crate::Backend` and `crate::Timestamp`. That is what makes swapping either a
one-line change.

**Keep comments short.** One line saying what the item is, plus at most a
sentence of *why* when the why is not obvious. No restating the signature in
prose. Reasoning that needs a paragraph goes in the crate's README, not in
a `//`. The
two exceptions are the one-line "why this exists" and a note where getting it
wrong is a bug.

**Hyphens only.** Use `-`. Never an em dash (U+2014), an en dash (U+2013), a
figure dash (U+2012) or a horizontal bar (U+2015) - in code, comments, error
strings, commit messages, markdown or generated output. Box-drawing characters
in tree diagrams are not dashes and are fine.

**Conventional Commits, scoped by crate** (`feat(db):`, `feat(web):`). The
changelog is generated from them, so a malformed type or scope silently drops
a commit.

**`missing_docs` is warned workspace-wide** - every public item needs a doc
comment. The exception is a `diesel::table!` schema module, which generates
undocumented items; put `#![allow(missing_docs)]` there.

**Rust and tooling only.** No npm package, no Svelte, nothing published to
crates.io.

### The diesel gotcha worth keeping

`AsChangeset` skips `Option<T>` fields on `None` by default rather than
nulling them. That is diesel's behaviour, not this crate's, but it bites: a
nullable column that needs to be clearable needs
`#[diesel(treat_none_as_null = true)]` on that field in the **consumer's**
model.

## What was deleted, and why

Do not helpfully reintroduce these:

- `Repository`, `Find`, `Save`, `Delete`, `impl_repository!` and friends -
  replaced by `#[derive(Entity)]` generating inherent methods.
- `EntityService` - its logging moved into `Db::run_named`, one span at DEBUG
  instead of a dozen hand-written `info!` lines that logged reads at INFO.
- `DatabaseService` - a service is a struct with `new(db)` and
  `into_server()`.
- `Controller<S>` - a router is `pub fn router() -> Router<S>`. A trait with
  one method and one implementation is a function.
- `request_id_layer()` / `propagate_request_id_layer()` - one-line renames of
  `tower-http` calls, replaced by W3C trace context.
- A hand-rolled `Cidr`, an RFC 3339 formatter and a `CloudEvent` struct -
  `ipnet` and `cloudevents-sdk` already exist.
- The three mutually exclusive backend features and their `compile_error!`
  blocks.
