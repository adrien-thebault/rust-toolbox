# {{project-name}}

A gRPC service{% if gateway %} and an HTTP gateway{% endif %}, generated from
the rust-toolbox template.

## Running it

```sh
cp .env.example .env      # then set {% if gateway %}SESSION_SECRET, SERVICE_SECRET and ADMIN_PASSWORD_HASH{% else %}DATABASE_URL{% endif %}
cargo fmt --all           # your crate name is new to the import sort order
cargo build               # writes Cargo.lock - commit it
{% if gateway %}./openapi.sh              # writes web/openapi.json - commit it too
{% endif %}docker compose up --build
```

{% if gateway %}
Once it is up, open `web/static/index.html` from a plain local static server
(`python3 -m http.server`, say) - not `file://` - and point it at
`http://localhost:8080`.
{% endif %}
**Commit `Cargo.lock`.** The `Dockerfile` copies it with `--locked`, and
`cargo generate` cannot ship one because the resolved graph differs per
backend and per gateway choice.
{% if gateway %}
One account is seeded from `ADMIN_USERNAME` and `ADMIN_PASSWORD_HASH`. Produce
the hash with `cargo run -p toolbox-auth --features password --example
hash-password`, then:

```sh
TOKEN=$(curl -s localhost:8080/auth/login \
  -H 'content-type: application/json' \
  -d '{"username":"admin","password":"..."}' | jq -r .access_token)
curl localhost:8080/api/todos -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"title":"write it down"}'
```

{% endif %}

## Layout

````
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

Three units:

- A **domain** is a crate. It owns a schema, a migration set and a pool. A
  second domain is a new directory under `grpc/`, picked up by
  `members = ["grpc/*"]`.
- A **gRPC service** is a file under that crate's `service/`. Two services in
  one domain read the same tables.
- An **entity** is a file under `model/`. A `TodoList` regrouping several todos
  is `model/todo_list.rs`.

`crate::Backend` and `crate::Timestamp` in `grpc/todo/src/lib.rs` are the only
places the database backend and the timestamp type are named.

## Adding an entity

1. A migration in `grpc/todo/migrations/`.
2. A `table!` in `grpc/todo/src/schema.rs`.
3. A file in `grpc/todo/src/model/` with `#[derive(toolbox_db::Entity)]` and
   `#[entity(backend = crate::Backend, ...)]`, and a line in `model.rs`.

The derive generates `find_by_id`, `find_by_ids`, `exists`, `count`, `page`,
`save`, `save_all`, `delete_by_id`, `delete_by_ids`, `truncate` and `query()`
as inherent methods. The `title_contains` filter in `service/todo_service.rs`
is the worked example of `query()` composed with pagination.
{% if gateway %}
## Replacing the seeded account

`SeededAdmin` in `web/src/auth.rs` is a `UserStore` over one hard-coded user. A
real one is a `UserStore` over a `users` table and one line in `providers()`.
{% endif %}
````
