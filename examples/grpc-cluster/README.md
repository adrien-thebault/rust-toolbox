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
./scripts/example.sh                  # the same proof on the host
```

On the host, `cargo run -p example-todo` and `cargo run -p example-web` take the
same variables as flags - `--listen-addr`, `--database-url`, `--todo-backend`.

## Layout

```
grpc/                 a grouping directory, not a crate
  todo/               one domain, one crate
    proto/todo/v1/
    migrations/
    src/
      main.rs         the process that serves this domain
      lib.rs          Backend, Connection, Timestamp, MIGRATIONS, proto
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
    state.rs          AppState, and the AuthState impl that mounts /auth/*
    auth.rs           who may log in, and the one role this example has
    routes.rs         the router, the OpenAPI doc, the Status -> ApiError seam
    routes/
      todo.rs         the DTOs and the five todo routes
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
