//! The todo service process.

use std::{sync::Arc, time::Duration};

use clap::Parser;
use {{crate_name}}_todo::{Connection, MIGRATIONS, TodoService, proto};
{% if gateway %}use toolbox_auth::{AssertedPrincipalProvider, ProviderRegistry};
{% endif %}use toolbox_cluster::{EventBus, InMemoryEventBus, InMemoryLockManager};
{% if database == "postgres" %}use toolbox_db::{Db, args::DatabaseArgs};
{% else %}use toolbox_db::{Db, SqlitePragmas, args::DatabaseArgs};
{% endif %}{% if gateway %}use toolbox_grpc::{
    GrpcServerConfig, Routes, serve,
    server::{identity, shared_secret::shared_secret_layer},
};
{% else %}use toolbox_grpc::{GrpcServerConfig, Routes, serve};
{% endif %}use toolbox_schedule::{Scheduler, Trigger};
use toolbox_server::{ServerBuilder, args::ServerArgs, poll_check, telemetry::TelemetryArgs};
{% if gateway %}use tower::Layer;
{% endif %}
/// Command-line arguments.
#[derive(Parser)]
#[command(name = "{{project-name}}")]
struct Args {
    /// Log format and level.
    #[command(flatten)]
    telemetry: TelemetryArgs,
    /// Listen address.
    #[command(flatten)]
    server: ServerArgs,
    /// The database URL and pool settings.
    #[command(flatten)]
    database: DatabaseArgs,
{% if gateway %}
    /// The secret the gateway presents on every call. Anything that cannot
    /// present it is refused before `TodoService` sees the request at all.
    #[arg(long, env = "SERVICE_SECRET")]
    service_secret: String,
{% endif %}}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    args.telemetry.init()?;

{% if database == "postgres" %}    let db = args.database.builder::<Connection>().build()?;
{% else %}    let db = args
        .database
        .builder::<Connection>()
        .sqlite_pragmas(SqlitePragmas::default())
        .build()?;
{% endif %}    // Locked, so three replicas starting together do not race.
    db.migrate(MIGRATIONS).await?;

    // A background probe of the pool, surfaced as `/ready` degradation rather
    // than a liveness failure that would restart the replica.
    let db_probe = poll_check("database", Duration::from_secs(5), db.clone(), Db::is_live);

    // Published to by every mutation and by the purge below; `WatchTodos`
    // streams it to the gateway. In-process only - a shared adapter would go
    // here for a second replica.
    let events: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::default());
    let todos = TodoService::new(db, events);

    // Sweeps completed todos nobody has touched in a month, emitting a
    // `todo.deleted` per row. Exclusive, so three replicas do not all
    // soft-delete the same rows - the point of `toolbox-schedule` existing at
    // all. Every minute so a freshly generated service shows the scheduler
    // working; set your real cadence (e.g. `0 3 * * *`) before deploying.
    // Handed to the server as a task, so it is aborted cleanly on `SIGTERM`.
    let scheduler = Scheduler::builder(Arc::new(InMemoryLockManager::new()))
        .job(
            "purge-completed-todos",
            Trigger::cron("* * * * *")?,
            Duration::from_secs(30),
            todos.clone(),
            |todos| async move {
                let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::days(30);
                todos.purge_completed(cutoff).await?;
                Ok(())
            },
        )?
        .build()?;
{% if gateway %}
    // Only the gateway may call this service: `shared_secret_layer` refuses
    // anything that does not present the secret, and `identity_layer` reads
    // who the gateway is asking on behalf of. Without both, anyone who can
    // reach this port could call `DeleteTodo` directly.
    let registry = Arc::new(ProviderRegistry::new().with(AssertedPrincipalProvider::new()));
    let routes = Routes::new(
        shared_secret_layer(args.service_secret.clone()).layer(
            identity::identity_layer(registry)
                .extracting(identity::asserted_principal)
                .layer(todos.into_server()),
        ),
    );
{% else %}
    // No gateway in front of this service - a caller reaches it directly, so
    // there is no shared secret to check and nothing to assert an identity
    // from. Add `toolbox_grpc::server::shared_secret::shared_secret_layer` and
    // your own identity extraction here if that changes.
    let routes = Routes::new(todos.into_server());
{% endif %}
    // Graceful shutdown, health, reflection and the standard stack all come
    // from serve; none of it is written here. A second domain is one more
    // `.add_service` on `routes` if it shares this process, or its own binary
    // if not.
    let server = ServerBuilder::listening_on(args.server.listen_addr)
        .check(db_probe)
        .task(scheduler.into_task(Duration::from_secs(60)))
        .build()
        .await?;
    serve(
        server,
{% if gateway %}        GrpcServerConfig::default()
            .reflection(proto::DESCRIPTOR)
            // So the gateway's own `poll_health` proves the secret is right,
            // not just that this process is up.
            .health_secret(args.service_secret),
{% else %}        GrpcServerConfig::default().reflection(proto::DESCRIPTOR),
{% endif %}        routes,
    )
    .await?;
    Ok(())
}
