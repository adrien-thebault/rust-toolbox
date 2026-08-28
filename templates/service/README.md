# {{project-name}}

A gRPC service{% if gateway %} and an HTTP gateway{% endif %}, generated from
the rust-toolbox template.

## What you get without writing it

- graceful shutdown on `SIGTERM`, with the drain delay that stops a rolling
  deploy dropping requests
- `/health` and `/ready`, wired into the compose healthcheck
- a request timeout and a body size limit
- W3C trace context, so every log line and every error body carries a request id
- RFC 9457 error responses, with 5xx detail redacted
- a deployment guard that refuses to start a single-replica adapter under
  `DEPLOYMENT=clustered`
- gRPC health and reflection, so `grpcurl` works with no protos to hand
- locked migrations, so replicas starting together do not race
{% if gateway %}- login, refresh, logout, `/auth/me` and `/auth/providers`, with
  the login rate limit already attached
{% endif %}

## Running it

```sh
cp .env.example .env      # then set {% if gateway %}SESSION_SECRET and ADMIN_PASSWORD_HASH{% else %}DATABASE_URL{% endif %}
cargo fmt --all           # imports sort by crate name, and yours is new
cargo build               # writes Cargo.lock - commit it, see below
docker compose up --build
```

`cargo fmt` first because a crate's own name sorts into its import blocks, and
the template cannot know it in advance. One run and `cargo fmt --check` in the
generated CI passes from then on.

**Commit `Cargo.lock`.** This is an application, not a library, so the lockfile
is what makes a build reproducible - and the `Dockerfile` does `COPY Cargo.lock`
with `--locked`, so without it the image cannot build at all. `cargo generate`
cannot ship one, because the resolved graph differs per database backend and
per gateway choice.
{% if gateway %}
One account is seeded, from `ADMIN_USERNAME` and `ADMIN_PASSWORD_HASH`. Produce
the hash with:

```sh
cargo run -p toolbox-auth --features password --example hash-password
```

Then log in and use the token:

```sh
TOKEN=$(curl -s localhost:8080/auth/login \
  -H 'content-type: application/json' \
  -d '{"username":"admin","password":"..."}' | jq -r .access_token)
curl localhost:8080/api/todos -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"title":"write it down"}'
```
{% endif %}

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
{% if gateway %}
web/
  src/
    main.rs           the gateway process
    lib.rs
    state.rs          AppState, and the AuthState impl that mounts /auth/*
    auth.rs           who may log in, and the roles this project has
    routes.rs         the router and the Status -> ApiError seam
    routes/
      todo.rs         the DTOs and the todo routes
{% endif %}```

Three units, and they are deliberately different sizes:

- A **domain** is a crate. It owns a schema, a migration set and a pool, and
  everything inside it can share them.
- A **gRPC service** is a file under that crate's `service/`. Two services in
  one domain read the same tables; giving each its own crate would buy no
  isolation the module boundary did not already give you, at the cost of a
  second build graph and a second deployment unit.
- An **entity** is a file under `model/`. A `TodoList` regrouping several todos
  is `model/todo_list.rs`, not a longer `model.rs`.

Adding a second domain is a new directory under `grpc/`. `members = ["grpc/*"]`
picks it up with no edit to the workspace manifest.

`crate::Backend` and `crate::Timestamp` in `grpc/todo/src/lib.rs` are the
**only** places the database backend and the timestamp type are named. Swapping
either is a one-line change.

## Adding an entity

1. A migration in `grpc/todo/migrations/`.
2. A `table!` in `grpc/todo/src/schema.rs`.
3. A file in `grpc/todo/src/model/` with `#[derive(toolbox_db::Entity)]` and
   `#[entity(backend = crate::Backend, ...)]`, and a line in `model.rs`.

The derive generates `find_by_id`, `find_by_ids`, `exists`, `count`, `page`,
`save`, `save_all`, `delete_by_id`, `delete_by_ids`, `truncate` and `query()`
as inherent methods. `query()` is the escape hatch, and pagination composes
onto whatever you build with it - `Todo::search` is the worked example.
{% if gateway %}
## Replacing the seeded account

`SeededAdmin` in `web/src/auth.rs` is a `UserStore` over one hard-coded user. A
real one is a `UserStore` over a `users` table and one line in `providers()`.
The login route does not change, and neither does anything that reads a
`Principal`.
{% endif %}
