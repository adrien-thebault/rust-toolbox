//! The todo service process.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use clap::Parser;
use diesel::RunQueryDsl as _;
use {{crate_name}}_todo::{Connection, MIGRATIONS, TodoService, model::Todo, proto};
{% if gateway %}use toolbox_auth::{AssertedPrincipalProvider, ProviderRegistry};
{% endif %}use toolbox_cluster::InMemoryLockManager;
use toolbox_db::args::DatabaseArgs;
{% if gateway %}use toolbox_grpc::{
    RoutesBuilder, ServerConfig, serve,
    server::{identity, shared_secret::shared_secret_layer},
};
{% else %}use toolbox_grpc::{RoutesBuilder, ServerConfig, serve};
{% endif %}use toolbox_schedule::{Scheduler, Trigger};
use toolbox_server::{HealthCheck, StartupConfig, args::ServerArgs, telemetry::TelemetryArgs};
{% if gateway %}use tower::Layer;
{% endif %}use tracing::error;

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

/// Reports the database as healthy once it has answered a trivial query, and
/// unhealthy again if a later ping fails.
///
/// Polled in the background rather than on the request path: `/ready` and the
/// gRPC health probe read `HealthCheck::is_healthy` synchronously and often,
/// so it must never itself make a round trip to the database.
struct DbCheck(Arc<AtomicBool>);

impl HealthCheck for DbCheck {
    fn name(&self) -> &'static str {
        "database"
    }
    fn is_healthy(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Ping the database every `interval`, publishing the result to `healthy`.
async fn ping_database(
    db: toolbox_db::Db<Connection>,
    healthy: Arc<AtomicBool>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let ok = db
            .query(|c: &mut Connection| diesel::sql_query("SELECT 1").execute(c))
            .await
            .is_ok();
        healthy.store(ok, Ordering::Relaxed);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let _telemetry = args.telemetry.init()?;

    let db = args.database.builder::<Connection>().build()?;
    // Locked, so three replicas starting together do not race.
    db.migrate(MIGRATIONS).await?;

    let db_healthy = Arc::new(AtomicBool::new(false));
    tokio::spawn(ping_database(
        db.clone(),
        Arc::clone(&db_healthy),
        Duration::from_secs(5),
    ));

    // Sweeps completed todos nobody has touched in a month. Exclusive, so
    // three replicas of this process do not all soft-delete the same rows -
    // the point of `toolbox-schedule` existing at all.
    let purge_db = db.clone();
    let scheduler = Scheduler::builder(Arc::new(InMemoryLockManager::new()))
        .job(
            "purge-completed-todos",
            Trigger::cron("0 3 * * *")?,
            Duration::from_secs(30),
            move || {
                let db = purge_db.clone();
                Box::pin(async move {
                    let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::days(30);
                    db.run(move |c: &mut Connection| Todo::purge_completed_before(c, cutoff))
                        .await?;
                    Ok(())
                })
            },
        )?
        .build()?;
    tokio::spawn(async move {
        let mut scheduler = scheduler;
        if let Err(e) = scheduler.run(Duration::from_secs(60)).await {
            error!(error = %e, "the purge scheduler stopped");
        }
    });

    let cfg = StartupConfig::new(args.server.listen_addr);
{% if gateway %}
    // Only the gateway may call this service: `shared_secret_layer` refuses
    // anything that does not present the secret, and `identity_layer` reads
    // who the gateway is asking on behalf of. Without both, anyone who can
    // reach this port could call `DeleteTodo` directly.
    let registry = Arc::new(ProviderRegistry::new().with(AssertedPrincipalProvider::new()));
    let service = shared_secret_layer(args.service_secret).layer(
        identity::identity_layer(registry)
            .extracting(identity::asserted_principal)
            .layer(TodoService::new(db).into_server()),
    );
{% else %}
    // No gateway in front of this service - a caller reaches it directly, so
    // there is no shared secret to check and nothing to assert an identity
    // from. Add `toolbox_grpc::server::shared_secret::shared_secret_layer` and
    // your own identity extraction here if that changes.
    let service = TodoService::new(db).into_server();
{% endif %}
    // Graceful shutdown, health, reflection and the standard stack all come
    // from serve; none of it is written here. A second domain is one more
    // `add_service` if it shares this process, or its own binary if not.
    let mut routes = RoutesBuilder::default();
    routes.add_service(service);
    serve(
        cfg,
        ServerConfig::default()
            .reflection(proto::DESCRIPTOR)
            .readiness_checks(vec![Box::new(DbCheck(db_healthy))]),
        routes,
    )
    .await?;
    Ok(())
}
