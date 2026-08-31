//! The todo service process.
//!
//! A whole gRPC service in under fifty lines, with graceful shutdown, health,
//! reflection, a deployment guard and locked migrations - none of which any
//! hand-written binary had.

use clap::Parser;
use example_todo::{Connection, MIGRATIONS, TodoService, proto};
use toolbox_db::args::DatabaseArgs;
use toolbox_grpc::{ServerConfig, serve};
use toolbox_server::{
    args::{DeploymentArgs, ServerArgs},
    startup::StartupConfig,
    telemetry::TelemetryArgs,
};

/// Command-line arguments.
#[derive(Parser)]
#[command(name = "example-todo")]
struct Args {
    /// Log format and level.
    #[command(flatten)]
    telemetry: TelemetryArgs,
    /// Listen address.
    #[command(flatten)]
    server: ServerArgs,
    /// `single` or `clustered`.
    #[command(flatten)]
    deployment: DeploymentArgs,
    /// The database URL and pool settings.
    #[command(flatten)]
    database: DatabaseArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let _telemetry = args.telemetry.init()?;

    let db = args.database.builder::<Connection>().build()?;
    // Locked, so three replicas starting together do not race.
    db.migrate(MIGRATIONS).await?;

    let deployment = args.deployment.resolve()?;
    let cfg = StartupConfig::new(args.server.listen_addr, &deployment);

    // Graceful shutdown, health, reflection and the deployment check all come
    // from serve; none of it is written here. A second domain is one more
    // `.add_service(...)` if it shares this process, or its own binary if not.
    serve(cfg, ServerConfig::default().reflection(proto::DESCRIPTOR))
        .add_service(TodoService::new(db).into_server())
        .run()
        .await?;
    Ok(())
}
