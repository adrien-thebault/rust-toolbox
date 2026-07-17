# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A standalone library of generic Rust building blocks - diesel repository/service
macros, tower logging/request-id layers, axum auth/RBAC/error types, a tonic
`ServiceError` trait, and an SMTP mail sender. It was extracted out of a
service backend because none of it is specific to that project's domain; it's
consumed as a git dependency by that repo (and meant to be reusable by
others).

**This is the one hard rule for this repo**: nothing here may depend on, or
encode assumptions about, any specific downstream project. If a change only
makes sense in terms of one consumer's domain model, it belongs in that
consumer's own crate, not here.

## Architecture

Every module lives behind its own Cargo feature flag and is independently
optional - a consumer enables only what it needs:

- `diesel_tools` (feature `diesel`, plus `sqlite`/`mysql`/`postgresql` for the
  backend) - `impl_repository!` + `impl_save!` generate a diesel CRUD
  repository from a schema module, entity type, and id column (`impl_save!`
  has a plain-upsert and an `autoincrement` flavor, picked by the id column's
  shape). `service/database_service.rs` (pool → tonic server) and
  `service/entity_service.rs` (find/save/delete request handling) are the
  two traits a `*Service` type implements. `pagination.rs` has `Page`,
  `PageRequest`, `Sort`.
- `tower_tools` (feature `tower`) - request-id assignment/propagation and
  request-tracing layers.
- `tonic_tools` (feature `tonic`) - the `ServiceError` trait
  (`code`/`domain`/`status_code`/`metadata`) and `to_status`/`from_status`
  conversions to/from `tonic::Status` via `google.rpc.ErrorInfo`
  (`tonic-types`). Deliberately *not* a blanket `impl From<E: ServiceError>
  for tonic::Status` - that would violate Rust's orphan rules for any
  downstream crate defining its own `E`. Each consumer keeps its own
  one-line `impl From<XError> for Status { to_status(err) }` instead.
- `axum_tools` (feature `axum`, implies `tonic` - a hard compile-time
  dependency, `api_error.rs` references `tonic_tools` unconditionally; does
  *not* imply `tower`, since axum_tools' own code never touches it) -
  `auth.rs` (`User`, `Credential`, `AuthBackend`, `SessionSecretProvider`),
  `auth/jwt.rs` (session issue/verify), `auth/in_memory.rs` (a basic-auth
  backend), `api_error.rs` (`ApiError`/`Problem`, an HTTP error type that
  checks `tonic_tools::from_status` first before falling back to a flat
  code→variant match). Role/permission types are deliberately *not* defined
  here - a consumer defines its own `Role` enum with `impl AsRef<str>` and
  calls `user.require_role(...)` with it.
- `mail_tools` (feature `mail`) - a reusable SMTP sender, configured
  explicitly (no reading env vars itself - that's the consumer's job).

Module names are `diesel_tools`/`tower_tools`/`axum_tools`/`tonic_tools`
rather than plain `diesel`/`tower`/`axum`/`tonic` on purpose: naming a module
identically to the extern crate it wraps makes the crate name ambiguous
wherever both are in scope, including inside the module's own macro-generated
code.

## Commands

```sh
cargo build --features sqlite,axum,mail
cargo fmt --all
cargo clippy --all-targets --features sqlite,axum,mail
cargo test --features sqlite,axum,mail <substring>   # single test, substring match
./scripts/test-all.sh   # cargo test once per feature combination (see below)
```

There's no workspace here - just this one crate. `sqlite`/`mysql`/`postgresql`
are mutually exclusive diesel backends (all three enabled together, e.g. via
`--all-features`, fails to compile with re-export/impl conflicts), so
there's no single "test everything" command - `scripts/test-all.sh` runs
`cargo test --no-default-features --features <set>` once for each of
`axum,mail`; `sqlite`; `postgresql`; `mysql`.

## Conventions to preserve

- **Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)**
  and commits should be signed - see [CONTRIBUTING.md](CONTRIBUTING.md).
  `CHANGELOG.md` is generated from commit history with
  [git-cliff](https://git-cliff.org/) (`scripts/changelog.sh`), so a
  malformed type/scope means a commit silently drops out of, or gets
  miscategorized in, the changelog.
- **No project-specific naming or assumptions.** Error domains, table names,
  role strings, etc. are all supplied by the consumer at the call site, not
  hardcoded here.
- **`#![warn(missing_docs)]` is on** (`src/lib.rs`) - every public item needs
  a doc comment.
- Diesel `AsChangeset` skips `Option<T>` fields on `None` by default (doesn't
  null them out) - this is diesel's behavior, not this crate's, but it's a
  common gotcha for anyone extending `diesel_tools`: a nullable column that
  needs to be clearable needs `#[diesel(treat_none_as_null = true)]` on that
  field in the *consumer's* model, not here.
