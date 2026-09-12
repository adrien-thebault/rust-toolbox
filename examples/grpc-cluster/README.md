# The gRPC cluster example

Two crates that deploy separately: `grpc` owns the database and speaks only
gRPC, `web` owns authentication and speaks only HTTP. It is the same tree
`templates/service` generates, with one less placeholder; the template adds a
Dockerfile and the compose file.

## Running it

```sh
cp .env.example .env
docker compose up                     # both services, port 8080
docker compose run --rm test          # the end-to-end test, no toolchain needed
cargo test -p example-todo -p example-web   # the same test on the host
./openapi.sh                          # regenerate the committed openapi.json
```

On the host, `cargo run -p example-todo` and `cargo run -p example-web` take
the same variables as flags - `--listen-addr`, `--database-url`,
`--todo-backend`.

Once both are up, open `web/static/index.html` from a plain local static server
(`python3 -m http.server`, say) - not `file://` - and point it at
`http://localhost:8080`. The gateway also serves its spec at `/openapi.json`
and a Scalar page at `/docs`.

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
        todo.rs       todo.v1.TodoService, and what it can fail with

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

Three units:

- A **domain** is a crate. It owns a schema, a migration set and a pool. A
  second domain is a sibling of `grpc/todo/`, picked up by
  `members = ["examples/*/grpc/*"]`.
- A **gRPC service** is a file under `service/`, named after the proto service
  it implements, carrying its own error type: `todo.v1.TodoAdminService` would
  be `service/todo_admin_service.rs` with a `TodoAdminServiceError` beside it.
- An **entity** is a file under `model/`. A `TodoList` regrouping several todos
  is `model/todo_list.rs`.

`From<Todo> for proto::Todo` lives with the entity, not with a service. The
end-to-end test in `web/tests/` drives both processes together; it is what CI
runs on every commit.
