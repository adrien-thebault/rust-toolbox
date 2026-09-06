use std::{sync::Arc, time::Duration};

use cloudevents::AttributesReader as _;
use diesel::connection::SimpleConnection;
use example_todo::{Connection, MIGRATIONS, TodoService};
use example_web::{auth::AuthConfig, routes::router};
use secrecy::SecretString;
use toolbox_auth::{AssertedPrincipalProvider, ProviderRegistry};
use toolbox_cluster::InMemoryEventBus;
use toolbox_grpc::{
    ClientConfig, client,
    server::{identity, shared_secret::shared_secret_layer},
};
use toolbox_test::{TestCluster, TestGateway, assert_problem, temp_db};
use toolbox_web::{ClientIpTrustPolicy, rate_limit::RateLimitConfig};
use tower::Layer as _;

/// The seeded account's password. Hashed at test time rather than committed,
/// so the fixture cannot drift from the argon2 parameters the crate uses.
const PASSWORD: &str = "correct horse battery staple";

/// The shared secret this test's gateway and backend agree on.
const SERVICE_SECRET: &str = "test-shared-secret";

/// The gateway's configuration, with the one account the example seeds.
fn config() -> AuthConfig {
    AuthConfig {
        session_secret: SecretString::from("0123456789abcdef0123456789abcdef"),
        issuer: "example-web".to_owned(),
        admin_username: "admin".to_owned(),
        admin_password_hash: toolbox_auth::hash_password(PASSWORD).expect("argon2 accepted it"),
    }
}

/// A backend on a real socket plus a gateway in process, which is the shape a
/// deployment actually has.
async fn cluster() -> (TestGateway, TestCluster, toolbox_test::db::TempDb) {
    cluster_with(RateLimitConfig::new(
        5,
        Duration::from_secs(5),
        ClientIpTrustPolicy::hops(1),
    ))
    .await
}

/// As above, with the credential routes throttled differently.
async fn cluster_with(
    login: RateLimitConfig,
) -> (TestGateway, TestCluster, toolbox_test::db::TempDb) {
    let (db, guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("migrations");

    // The same shared-secret-then-identity chain `main.rs` wires, so a test
    // that only exercised the unwrapped `TodoService` could not tell the
    // backend's own admin check from the gateway's.
    let service_db = db.clone();
    let cluster = TestCluster::new()
        .service("todo", move |routes| {
            let registry = Arc::new(ProviderRegistry::new().with(AssertedPrincipalProvider::new()));
            let events = Arc::new(InMemoryEventBus::default());
            routes.add_service(
                shared_secret_layer(SERVICE_SECRET).layer(
                    identity::identity_layer(registry)
                        .extracting(identity::asserted_principal)
                        .layer(TodoService::new(service_db, events).into_server()),
                ),
            );
        })
        .await
        .expect("the todo backend came up");

    let channel = client(
        "todo",
        &ClientConfig::new(&cluster.backend_uri("todo"))
            .expect("a valid uri")
            .service_secret(SERVICE_SECRET),
    );

    let state = example_web::auth::state(channel, &config()).expect("the gateway configured");

    (TestGateway::new(router(state, &login)), cluster, guard)
}

/// Log in as the seeded admin and return the bearer token.
///
/// No middleware injecting a `Principal`: the token comes out of the real login
/// route, so the test covers the codec and the extractor rather than faking
/// both.
async fn login(app: &TestGateway) -> String {
    let response = app
        .post_json(
            "/auth/login",
            &serde_json::json!({"username": "admin", "password": PASSWORD}),
        )
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    response.json::<serde_json::Value>()["access_token"]
        .as_str()
        .expect("a session token")
        .to_owned()
}

#[tokio::test]
async fn a_todo_can_be_created_and_read_back_through_both_hops() {
    let (app, _cluster, _guard) = cluster().await;

    let created = app
        .post_json("/api/todos", &serde_json::json!({"title": "write it down"}))
        .await;
    assert_eq!(created.status_code(), 200, "{}", created.text());

    let body: serde_json::Value = created.json();
    let id = body["id"].as_i64().expect("the database assigned an id");
    assert!(id > 0, "autoincrement produced {id}");
    assert_eq!(body["title"], "write it down");
    assert!(!body["done"].as_bool().unwrap());

    let fetched = app.get(&format!("/api/todos/{id}")).await;
    assert_eq!(fetched.status_code(), 200);
    assert_eq!(
        fetched.json::<serde_json::Value>()["title"],
        "write it down"
    );
}

/// A backend error has to arrive as the gateway's own problem document, with
/// the originating service's code intact. That is the whole `ErrorInfo` seam.
#[tokio::test]
async fn a_backend_not_found_becomes_a_problem_document_with_the_backends_code() {
    let (app, _cluster, _guard) = cluster().await;
    let problem = app.get_problem("/api/todos/9999").await;
    assert_problem!(problem, 404, "TODO_NOT_FOUND", "id");
}

#[tokio::test]
async fn validation_happens_at_the_gateway_before_a_hop_is_made() {
    let (app, _cluster, _guard) = cluster().await;
    let problem = app
        .post_problem("/api/todos", &serde_json::json!({"title": ""}))
        .await;
    assert_problem!(problem, 400, "VALIDATION_FAILED", "title");
}

#[tokio::test]
async fn pagination_and_a_backend_side_filter_agree_on_the_total() {
    let (app, _cluster, _guard) = cluster().await;
    for i in 0..7 {
        app.post_json(
            "/api/todos",
            &serde_json::json!({"title": format!("todo {i}")}),
        )
        .await;
    }

    let page = app.get("/api/todos?offset=0&limit=3").await;
    assert_eq!(page.status_code(), 200);
    let body: serde_json::Value = page.json();
    assert_eq!(body["items"].as_array().unwrap().len(), 3);
    assert_eq!(
        body["total"], 7,
        "the window is three rows, the total is all seven"
    );
}

#[tokio::test]
async fn an_over_large_page_is_refused_at_the_gateway() {
    let (app, _cluster, _guard) = cluster().await;
    let problem = app.get_problem("/api/todos?limit=100001").await;
    assert_problem!(problem, 400, "INVALID_PAGE", "max_limit");
}

/// Optimistic locking across two hops: the second writer loses, and the loss
/// is a 409 rather than a silently overwritten row.
#[tokio::test]
async fn a_stale_version_is_a_conflict_all_the_way_to_the_client() {
    let (app, _cluster, _guard) = cluster().await;
    let created: serde_json::Value = app
        .post_json("/api/todos", &serde_json::json!({"title": "race"}))
        .await
        .json();
    let id = created["id"].as_i64().unwrap();
    let version = created["version"].as_i64().unwrap();

    let first = app
        .post_json(
            &format!("/api/todos/{id}/complete"),
            &serde_json::json!({"version": version}),
        )
        .await;
    assert_eq!(first.status_code(), 200, "{}", first.text());

    let second = app
        .post_problem(
            &format!("/api/todos/{id}/complete"),
            &serde_json::json!({"version": version}),
        )
        .await;
    assert_problem!(second, 409, "TODO_CONFLICT", "id");
}

#[tokio::test]
async fn the_wrong_password_is_a_401_and_says_nothing_else() {
    let (app, _cluster, _guard) = cluster().await;
    let problem = app
        .post_problem(
            "/auth/login",
            &serde_json::json!({"username": "admin", "password": "hunter2"}),
        )
        .await;
    assert_problem!(problem, 401, "UNAUTHENTICATED");
}

/// An unknown username must fail exactly like a wrong password, or the API
/// enumerates accounts.
#[tokio::test]
async fn an_unknown_username_fails_identically_to_a_wrong_password() {
    let (app, _cluster, _guard) = cluster().await;
    let problem = app
        .post_problem(
            "/auth/login",
            &serde_json::json!({"username": "nobody", "password": PASSWORD}),
        )
        .await;
    assert_problem!(problem, 401, "UNAUTHENTICATED");
}

/// The session the login route issued is the one `/auth/me` reads back, roles
/// included.
#[tokio::test]
async fn a_session_carries_the_roles_the_user_store_declared() {
    let (app, _cluster, _guard) = cluster().await;
    let token = login(&app).await;

    let me: serde_json::Value = app
        .server()
        .get("/auth/me")
        .authorization_bearer(&token)
        .await
        .json();
    assert_eq!(me["subject"], "admin");
    assert_eq!(me["roles"], serde_json::json!(["ADMIN"]));
}

/// The role is checked by the type system, and an anonymous caller never
/// reaches the backend at all.
#[tokio::test]
async fn deleting_requires_the_admin_role() {
    let (app, _cluster, _guard) = cluster().await;
    let created: serde_json::Value = app
        .post_json("/api/todos", &serde_json::json!({"title": "delete me"}))
        .await
        .json();
    let id = created["id"].as_i64().unwrap();

    let refused = app.server().delete(&format!("/api/todos/{id}")).await;
    assert_eq!(
        refused.status_code(),
        401,
        "an anonymous caller is unauthenticated"
    );
}

#[tokio::test]
async fn an_admin_can_delete_and_the_row_soft_deletes() {
    let (app, _cluster, _guard) = cluster().await;
    let token = login(&app).await;

    let created: serde_json::Value = app
        .post_json("/api/todos", &serde_json::json!({"title": "delete me"}))
        .await
        .json();
    let id = created["id"].as_i64().unwrap();

    let deleted = app
        .server()
        .delete(&format!("/api/todos/{id}"))
        .authorization_bearer(&token)
        .await;
    assert_eq!(deleted.status_code(), 200, "{}", deleted.text());
    assert_eq!(deleted.json::<serde_json::Value>()["deleted"], 1);

    // Soft delete: it drops out of every read.
    let gone = app.get_problem(&format!("/api/todos/{id}")).await;
    assert_problem!(gone, 404, "TODO_NOT_FOUND");

    let listed: serde_json::Value = app.get("/api/todos").await.json();
    assert_eq!(listed["total"], 0);
}

/// A repeated `Idempotency-Key` replays the first response rather than
/// creating a second todo - the point of sending one at all.
#[tokio::test]
async fn a_repeated_idempotency_key_replays_the_first_response() {
    let (app, _cluster, _guard) = cluster().await;
    let body = serde_json::json!({"title": "only once"});

    let first = app
        .server()
        .post("/api/todos")
        .add_header("idempotency-key", "abc-123")
        .json(&body)
        .await;
    assert_eq!(first.status_code(), 200, "{}", first.text());

    let second = app
        .server()
        .post("/api/todos")
        .add_header("idempotency-key", "abc-123")
        .json(&body)
        .await;
    assert_eq!(second.status_code(), 200, "{}", second.text());
    assert_eq!(
        second.json::<serde_json::Value>(),
        first.json::<serde_json::Value>(),
        "replayed the first response rather than creating a second todo"
    );

    let listed: serde_json::Value = app.get("/api/todos").await.json();
    assert_eq!(listed["total"], 1, "only one todo actually exists");
}

/// This is the regression test for the actual security hole the gateway's
/// architecture used to leave open: `shared_secret_layer` and `identity_layer`
/// closed it, but only a test that calls the backend directly, bypassing the
/// gateway entirely, can prove that.
#[tokio::test]
async fn bypassing_the_gateway_and_calling_the_backend_directly_is_refused() {
    let (db, _guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("migrations");

    let cluster = TestCluster::new()
        .service("todo", move |routes| {
            let registry = Arc::new(ProviderRegistry::new().with(AssertedPrincipalProvider::new()));
            let events = Arc::new(InMemoryEventBus::default());
            routes.add_service(
                shared_secret_layer(SERVICE_SECRET).layer(
                    identity::identity_layer(registry)
                        .extracting(identity::asserted_principal)
                        .layer(TodoService::new(db, events).into_server()),
                ),
            );
        })
        .await
        .expect("the backend came up");

    // No `.service_secret(..)` here: this is exactly what reaching the
    // backend some other way than through the gateway looks like.
    let channel = client(
        "todo",
        &ClientConfig::new(&cluster.backend_uri("todo")).expect("a valid uri"),
    );
    let mut raw =
        example_todo::proto::todo_service_client::TodoServiceClient::new(channel.channel());
    let refused = raw
        .list_todos(example_todo::proto::ListTodosRequest {
            page: None,
            title_contains: String::new(),
        })
        .await;
    assert_eq!(
        refused.unwrap_err().code(),
        tonic::Code::Unauthenticated,
        "a caller with no shared secret must be refused before TodoService sees the request"
    );
}

/// Defense in depth: even a caller that presents the shared secret *and* a
/// validly asserted principal still cannot delete without the admin role -
/// the backend does not trust the gateway's own `Authenticated<Admin>` gate to
/// be the only one.
#[tokio::test]
async fn an_asserted_non_admin_principal_still_cannot_delete() {
    let (_app, cluster, _guard) = cluster().await;

    let channel = client(
        "todo",
        &ClientConfig::new(&cluster.backend_uri("todo"))
            .expect("a valid uri")
            .service_secret(SERVICE_SECRET),
    );
    let mut raw =
        example_todo::proto::todo_service_client::TodoServiceClient::new(channel.channel());

    let encoded =
        toolbox_auth::AssertedPrincipal::from(&toolbox_auth::Principal::new("mallory", "test"))
            .encode();
    let mut request = tonic::Request::new(example_todo::proto::DeleteTodoRequest { id: 1 });
    request
        .metadata_mut()
        .insert(toolbox_grpc::X_ASSERTED_PRINCIPAL, encoded.parse().unwrap());

    let refused = raw.delete_todo(request).await;
    assert_eq!(
        refused.unwrap_err().code(),
        tonic::Code::PermissionDenied,
        "a real, resolved principal without the admin role is still refused"
    );
}

/// A backend mutation publishes a `todo.*` event, `WatchTodos` streams it, and
/// `forward_events` relays it into the hub every SSE connection subscribes to.
/// The regression test for those three staying connected.
#[tokio::test]
async fn a_backend_mutation_reaches_every_hub_subscriber() {
    use example_todo::proto::{CreateTodoRequest, todo_service_client::TodoServiceClient};
    use toolbox_web::realtime::{Hub, HubConfig, SlowConsumer};

    let (db, _guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("migrations");

    let cluster = TestCluster::new()
        .service("todo", move |routes| {
            let events = Arc::new(InMemoryEventBus::default());
            routes.add_service(TodoService::new(db, events).into_server());
        })
        .await
        .expect("the todo backend came up");

    let channel = client(
        "todo",
        &ClientConfig::new(&cluster.backend_uri("todo")).expect("a valid uri"),
    );

    // `forward_events` is what `main.rs` spawns over `auth::state`; this drives
    // the same wiring against a real backend stream.
    let hub = Arc::new(Hub::new(HubConfig::new(8, SlowConsumer::DropOldest)));
    tokio::spawn(example_web::auth::forward_events(
        channel.clone(),
        hub.clone(),
    ));
    // Let the WatchTodos stream attach before anything is published.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut rx = hub.subscribe("todos");
    TodoServiceClient::new(channel.channel())
        .create_todo(CreateTodoRequest {
            title: "watch me".to_owned(),
        })
        .await
        .expect("the create succeeded");

    let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("the hub relayed the event before the timeout")
        .expect("the channel is still open");
    assert_eq!(received.ty(), "todo.created");
}

/// A refresh token is a stateless JWT: it redeems for a fresh, usable session,
/// and the rolled token is itself redeemable. Revocation is a credential
/// change, not server-side consumption.
#[tokio::test]
async fn a_refresh_token_redeems_for_a_usable_session() {
    let (app, _cluster, _guard) = cluster().await;

    let session: serde_json::Value = app
        .post_json(
            "/auth/login",
            &serde_json::json!({"username": "admin", "password": PASSWORD}),
        )
        .await
        .json();
    let refresh = session["refresh_token"]
        .as_str()
        .expect("the deployment issues refresh tokens")
        .to_owned();

    let rotated = app
        .post_json(
            "/auth/refresh",
            &serde_json::json!({"refresh_token": refresh}),
        )
        .await;
    assert_eq!(rotated.status_code(), 200, "{}", rotated.text());
    let rotated: serde_json::Value = rotated.json();

    // The new access token authorizes a real call.
    let listed = app
        .server()
        .get("/api/todos")
        .authorization_bearer(rotated["access_token"].as_str().unwrap())
        .await;
    assert_eq!(listed.status_code(), 200, "{}", listed.text());

    // The rolled refresh token is itself redeemable.
    let again = app
        .post_json(
            "/auth/refresh",
            &serde_json::json!({"refresh_token": rotated["refresh_token"]}),
        )
        .await;
    assert_eq!(again.status_code(), 200, "{}", again.text());
}

/// The credential routes are throttled and nothing else is, which is the whole
/// reason the limiter goes inside `auth_router` rather than over the gateway.
///
/// This is the regression test for a real defect: the module documented a login
/// rate limit that `auth_router` never attached.
#[tokio::test]
async fn repeated_login_attempts_are_throttled_and_the_rest_of_the_api_is_not() {
    let (app, _cluster, _guard) = cluster_with(RateLimitConfig::new(
        2,
        Duration::from_secs(60),
        ClientIpTrustPolicy::hops(1),
    ))
    .await;

    let wrong = serde_json::json!({"username": "admin", "password": "hunter2"});
    for attempt in 1..=2 {
        let refused = app.post_json("/auth/login", &wrong).await;
        assert_eq!(
            refused.status_code(),
            401,
            "attempt {attempt} is a normal rejection, not a throttle"
        );
    }

    let throttled = app.server().post("/auth/login").json(&wrong).await;
    assert_eq!(throttled.status_code(), 429);
    assert!(
        throttled.headers().contains_key("retry-after"),
        "the wait the limiter computed has to reach the client, or it can only guess"
    );
    assert_problem!(
        toolbox_test::gateway::problem_of(&throttled),
        429,
        "RATE_LIMITED"
    );

    // Reading a todo is not a credential check, so it is not throttled.
    assert_eq!(app.get("/api/todos").await.status_code(), 200);
}

/// The migrations run through `Db::migrate`, which takes the backend-native
/// lock. Calling it twice must be a no-op rather than an error.
#[tokio::test]
async fn migrations_are_idempotent() {
    let (db, _guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("first run");
    db.migrate(MIGRATIONS).await.expect("second run is a no-op");

    let count: i64 = db
        .query(|c: &mut Connection| {
            use diesel::prelude::*;
            example_todo::schema::todos::table.count().get_result(c)
        })
        .await
        .expect("the table exists");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn the_schema_the_migration_creates_matches_the_entity() {
    let (db, _guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("migrations");
    // A mismatch between the migration and the derive shows up as a query
    // error rather than at compile time, so it is worth one assertion.
    db.blocking_conn()
        .expect("a connection")
        .batch_execute(
            "SELECT id, title, done, created_at, updated_at, deleted_at, version FROM todos",
        )
        .expect("every column the entity declares exists");
}

/// The deadline has to reach the backend, or a gateway that times out leaves
/// it working on a request nobody is waiting for.
///
/// This is the regression test for a real defect: `toolbox_server::deadline`
/// was written, exported and documented, and attached to nothing.
#[tokio::test]
async fn a_caller_deadline_reaches_the_backend_as_grpc_timeout() {
    use std::sync::Mutex;

    use tonic::service::Interceptor;

    // A server-side interceptor that records what arrived.
    #[derive(Clone)]
    struct Recorder(Arc<Mutex<Option<String>>>);

    impl Interceptor for Recorder {
        fn call(
            &mut self,
            request: tonic::Request<()>,
        ) -> Result<tonic::Request<()>, tonic::Status> {
            let seen = request
                .metadata()
                .get("grpc-timeout")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            *self.0.lock().unwrap() = seen;
            Ok(request)
        }
    }

    let (db, _guard) = temp_db::<Connection>();
    db.migrate(MIGRATIONS).await.expect("migrations");

    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let recorder = Recorder(Arc::clone(&seen));

    let cluster = TestCluster::new()
        .service("todo", move |routes| {
            let events = Arc::new(InMemoryEventBus::default());
            routes.add_service(tonic::service::interceptor::InterceptedService::new(
                TodoService::new(db, events).into_server(),
                recorder,
            ));
        })
        .await
        .expect("the backend came up");

    let channel = client(
        "todo",
        &ClientConfig::new(&cluster.backend_uri("todo")).expect("a valid uri"),
    );

    // Without a caller deadline in scope, nothing is sent - inventing one
    // would cap calls made outside a request, like a scheduled job's.
    let mut client =
        example_todo::proto::todo_service_client::TodoServiceClient::new(channel.channel());
    let _ = client
        .create_todo(example_todo::proto::CreateTodoRequest { title: "x".into() })
        .await;
    assert!(
        seen.lock().unwrap().is_none(),
        "no deadline in scope means no header"
    );

    // With one, the backend is told how long it has.
    let sent = toolbox_server::deadline::DEADLINE
        .scope(
            std::time::Instant::now() + std::time::Duration::from_secs(10),
            async {
                let mut client = example_todo::proto::todo_service_client::TodoServiceClient::new(
                    channel.channel(),
                );
                let _ = client
                    .create_todo(example_todo::proto::CreateTodoRequest { title: "y".into() })
                    .await;
                seen.lock().unwrap().clone()
            },
        )
        .await;

    let sent = sent.expect("the caller's deadline reached the backend");
    let ms: u64 = sent.trim_end_matches('m').parse().expect("milliseconds");
    assert!(
        (9_400..=9_900).contains(&ms),
        "the backend gets the remaining budget less a fixed slack, not all of it: {sent}"
    );
}
