//! The HTTP gateway.

use clap::Parser;
use {{crate_name}}_web::{auth, auth::AuthConfig, routes::router};
use toolbox_cluster::Adapter;
use toolbox_grpc::{ClientConfig, client};
use toolbox_server::{
    args::{DeploymentArgs, ServerArgs},
    stack::{StackConfig, http_stack},
    startup::StartupConfig,
    telemetry::TelemetryArgs,
};
use toolbox_web::{
    TrustedHops,
    auth::LoginLimit,
    health::{HealthState, health_router},
    rate_limit::RateLimitAdapter,
    serve_http,
};

/// Command-line arguments.
#[derive(Parser)]
#[command(name = "gateway")]
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

    /// Where the {{project-name}} backend is.
    #[arg(
        long,
        env = "SERVICE_BACKEND",
        default_value = "http://127.0.0.1:50051"
    )]
    service_backend: String,

    /// How many proxies append to `X-Forwarded-For` before a request arrives.
    ///
    /// Set too low behind a proxy, the login limiter keys on the proxy's own
    /// address and the first attacker locks out every other caller.
    #[arg(long, env = "TRUSTED_HOPS", default_value_t = 1)]
    trusted_hops: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let _telemetry = args.telemetry.init()?;

    let todos = client("todo", &ClientConfig::new(&args.service_backend)?);

    // Everything identity needs, read once at startup so a missing variable is
    // a refusal to start rather than a 500 on the first login.
    let config = AuthConfig::from_env()?;
    let state = auth::state(todos, &config)?;

    let login = LoginLimit {
        hops: TrustedHops(args.trusted_hops),
        ..LoginLimit::default()
    };

    let deployment = args.deployment.resolve()?;
    let limiter = RateLimitAdapter;
    // Every stateful adapter this process built, so the guard can flag a
    // single-replica one under DEPLOYMENT=clustered.
    let adapters: Vec<&dyn Adapter> = vec![&limiter];

    let cfg = StartupConfig::new(args.server.listen_addr, &deployment).adapters(&adapters);
    let health = HealthState::new(cfg.shutdown_handle.readiness());

    // The stack is applied here rather than by serve_http, because a router
    // with realtime routes needs realtime_stack on those and http_stack on
    // the rest. StackConfig's defaults give a 30s timeout and a 2 MiB body
    // limit; an upload route would be layered separately.
    let app = router(state, &login)
        .layer(http_stack(StackConfig::default()))
        // Outside the stack on purpose: inside it, the 503 that /ready returns
        // while draining is classified as a failure and logged at ERROR on
        // every rolling deploy.
        .merge(health_router().with_state(health));

    serve_http(cfg, app).await?;
    Ok(())
}
