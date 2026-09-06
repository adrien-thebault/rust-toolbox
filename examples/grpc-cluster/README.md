# The gRPC cluster example

Two crates that deploy separately: `grpc` owns the database and speaks only
gRPC, `web` owns authentication and speaks only HTTP. Neither knows how the
other is deployed, which is the point.

It is what `templates/service` generates: same layout, same two `main.rs`, one
less placeholder. What the template adds is a Dockerfile and the compose file
that deploys it.

## Running it

```sh
cp .env.example .env
docker compose up                     # both services, port 8080
docker compose run --rm test          # the end-to-end proof, no toolchain needed
cargo test -p example-todo -p example-web   # the same proof on the host
./openapi.sh                          # regenerate the committed openapi.json
```

On the host, `cargo run -p example-todo` and `cargo run -p example-web` take the
same variables as flags - `--listen-addr`, `--database-url`, `--todo-backend`.

Once both are up, open `web/static/index.html` from a plain local static
server (`python3 -m http.server`, say) - not `file://`, which sends no `Origin`
header `cors_localhost` can match - and point it at `http://localhost:8080`.
It logs in, lists, creates, completes and deletes todos, and updates live over
the SSE stream: a real client against the real API, no build step.

The gateway also serves its own spec at `http://localhost:8080/openapi.json`
and a Scalar page at `/docs`. `./openapi.sh` writes the same document to the
committed `web/openapi.json`, which CI checks for drift.

## Layout

```
grpc/                 a grouping directory, not a crate
  todo/               one domain, one crate
    proto/todo/v1/
    migrations/
    src/
      main.rs         the process that serves this domain
      lib.rs          Backend, Connection, Timestamp, MIGRATIONS, proto
      auth.rs         the Admin role this domain checks on its own caller
      schema.rs
      model.rs
      model/
        todo.rs       the entity, and how it goes on the wire
      service.rs
      service/
        todo_service.rs   todo.v1.TodoService, and what it can fail with

web/
  src/
    main.rs           the gateway process
    lib.rs
    state.rs          AppState: the backend channel, idempotency, the event bus
    auth.rs           who may log in, the one role this example has, and the
                       event bus -> hub wiring behind the SSE route
    routes.rs         the router, the OpenAPI doc, the Status -> ApiError seam
    routes/
      todo.rs         the DTOs, the five todo routes and the SSE route
  static/
    index.html        a plain HTML client against the running gateway
  tests/
  examples/dump_openapi.rs
```

Three units, deliberately different sizes:

- A **domain** is a crate. It owns a schema, a migration set and a pool, and
  everything inside it shares them. A second domain is a sibling of
  `grpc/todo/`, which `members = ["examples/*/grpc/*"]` picks up with no edit.
- A **gRPC service** is a file under `service/`, named after the proto service
  it implements, carrying its own error type. `todo.v1.TodoAdminService` would
  be `service/todo_admin_service.rs` with a `TodoAdminServiceError` beside it -
  same tables, same pool, no second crate.
- An **entity** is a file under `model/`. A `TodoList` regrouping several todos
  is `model/todo_list.rs`, not a longer `model.rs`.

`From<Todo> for proto::Todo` lives with the entity rather than with a service,
because every service in the domain sends the same shape and the entity is what
they have in common.

## What the test covers that a smoke test would not

- a backend `TODO_NOT_FOUND` arriving as the gateway's own RFC 9457 document
  with the originating code intact
- validation rejecting at the gateway before a hop is made
- optimistic locking losing across two processes as a 409
- a real login: the token comes out of `/auth/login` and is checked by
  `Authenticated<Admin>`, so the codec and the extractor are both exercised
- an unknown username failing identically to a wrong password
- a stateless refresh token redeeming for a usable session
- a caller deadline reaching the backend as `grpc-timeout`
- **calling the backend directly, bypassing the gateway entirely, is
  refused** - the regression test for the actual hole this shape used to
  leave open: `shared_secret_layer` refuses a caller with no secret, and
  `TodoService::delete_todo` refuses an asserted principal without the admin
  role, even if the secret is genuine
- a repeated `Idempotency-Key` on `POST /api/todos` replays the first
  response rather than creating a second todo
- a mutation reaching every `Hub` subscriber through `forward_events` - the
  glue behind the SSE route

## Auth, health and scheduling this example also demonstrates

Beyond the write-up above, the backend and gateway put a few more toolbox
pieces to real work, not just the bare minimum to compile:

- **The gateway asserts who it is calling on behalf of.** Every backend call
  carries the caller's `AssertedPrincipal` (`toolbox_grpc::client::asserting`)
  behind a mandatory `shared_secret_layer`, so the backend's own
  `identity_layer` can check it - see `grpc/todo/src/auth.rs` and the security
  test above.
- **`Health`/`HealthCheck` report something real.** Both processes poll a
  real dependency in the background (the database; the backend, over a real
  RPC) rather than reporting healthy unconditionally from process start.
- **A scheduled job.** `toolbox-schedule` runs an exclusive, cluster-safe
  sweep that soft-deletes completed todos nobody has touched in a month,
  leased through `toolbox_cluster::LockManager` - the reason
  `toolbox-schedule` is a dependency at all.
- **A forwarded-proxy identity provider is actually registered**
  (`ForwardedIdentityProvider::trusting_peers`), illustrating the pattern for
  a deployment that sits behind an authenticating reverse proxy, alongside the
  gateway's own login.
