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
- gRPC health and reflection, so `grpcurl` works with no protos to hand
- locked migrations, so replicas starting together do not race
- a `HealthCheck` polling the database in the background, so `/ready` reflects
  a real dependency rather than "the process is up"
- an exclusive, cluster-safe scheduled job (`toolbox-schedule`) sweeping
  completed todos nobody has touched in a month
{% if gateway %}- login, refresh, logout and `/auth/me`, with the login rate
  limit already attached
- the gateway's calls to the service carry an asserted, verified principal
  (`shared_secret_layer` + `identity_layer`), so bypassing the gateway and
  calling the service directly does not bypass authorization too
- a repeated `Idempotency-Key` on the create route replays the first response
  instead of creating a second row
- an SSE route and a plain HTML page (`web/static/index.html`) that uses it
- an OpenAPI spec generated from the routes, served live at `/openapi.json`
  with a Scalar page at `/docs`, plus a CI check that the committed
  `web/openapi.json` stays in sync
{% endif %}

## Running it

```sh
cp .env.example .env      # then set {% if gateway %}SESSION_SECRET, SERVICE_SECRET and ADMIN_PASSWORD_HASH{% else %}DATABASE_URL{% endif %}
cargo fmt --all           # imports sort by crate name, and yours is new
cargo build               # writes Cargo.lock - commit it, see below
{% if gateway %}./openapi.sh              # writes web/openapi.json - commit it too
{% endif %}docker compose up --build
```
{% if gateway %}
Once it is up, open `web/static/index.html` from a plain local static server
(`python3 -m http.server`, say) - not `file://`, which sends no `Origin` header
`cors_localhost` can match - and point it at `http://localhost:8080`.
{% endif %}

`cargo fmt` first because a crate's own name sorts into its import blocks, and
the template cannot know it in advance. One run and `cargo fmt --check` in the
generated CI passes from then on.{% if gateway %} The same goes for `./openapi.sh`
and the `openapi` CI job: run it once, commit `web/openapi.json`, and a diff
afterwards means a route changed shape without the spec being regenerated.{% endif %}

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
      auth.rs         the Admin role this domain checks on its own caller
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
    state.rs          AppState: the service channel, idempotency, the event bus
    auth.rs           who may log in, the roles this project has, and the
                       event bus -> hub wiring behind the SSE route
    routes.rs         the router, the OpenAPI doc, the Status -> ApiError seam
    routes/
      todo.rs         the DTOs, the todo routes and the SSE route
  static/
    index.html        a plain HTML client against the running gateway
  examples/
    dump_openapi.rs   prints the spec; ./openapi.sh redirects it into openapi.json
  openapi.json        the committed spec, checked for drift in CI
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
onto whatever you build with it - the `title_contains` filter in
`service/todo_service.rs` is the worked example.
{% if gateway %}
## Replacing the seeded account

`SeededAdmin` in `web/src/auth.rs` is a `UserStore` over one hard-coded user. A
real one is a `UserStore` over a `users` table and one line in `providers()`.
The login route does not change, and neither does anything that reads a
`Principal`.
{% endif %}
