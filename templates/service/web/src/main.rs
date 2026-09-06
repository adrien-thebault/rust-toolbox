//! The HTTP gateway.

use std::time::Duration;

use clap::Parser;
use {{crate_name}}_web::{
    auth,
    auth::AuthConfig,
    routes::{openapi, realtime_router, router},
};
use toolbox_grpc::{BackoffConfig, ClientConfig, RetryPolicy, client, client::poll_health};
use toolbox_server::{
    StartupConfig,
    args::ServerArgs,
    stack::{StackConfig, http_stack, realtime_stack},
    telemetry::TelemetryArgs,
};
use toolbox_web::{
    ClientIpTrustPolicy, OpenApiConfig, cors_localhost,
    health::{HealthState, health_router},
    openapi_router,
    rate_limit::RateLimitConfig,
    serve,
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

    /// Where the {{project-name}} backend is.
    #[arg(
        long,
        env = "SERVICE_BACKEND",
        default_value = "http://127.0.0.1:50051"
    )]
    service_backend: String,

    /// The secret this gateway presents to the backend on every call, so the
    /// backend can refuse anyone who reaches it some other way.
    #[arg(long, env = "SERVICE_SECRET")]
    service_secret: String,

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

    let todos = client(
        "todo",
        &ClientConfig::new(&args.service_backend)?
            .service_secret(&args.service_secret)
            // Reads only: retrying a create or a delete would risk a caller
            // seeing it happen twice.
            .retry(RetryPolicy::Idempotent {
                max_attempts: 3,
                backoff: BackoffConfig::default(),
                methods: &["GetTodo", "ListTodos"],
            }),
    );

    // The standard `grpc.health.v1.Health/Check` RPC, not a business call -
    // cheap, and it still proves the shared secret is right, since the
    // backend gates its health service on it too.
    let (backend_check, backend_poll) =
        poll_health(todos.clone(), "backend", Duration::from_secs(5));
    tokio::spawn(backend_poll);

    // Everything identity needs, read once at startup so a missing variable is
    // a refusal to start rather than a 500 on the first login.
    let config = AuthConfig::from_env()?;
    let state = auth::state(todos, &config)?;
    tokio::spawn(auth::forward_events(state.todos.clone(), state.hub.clone()));

    // A handful of attempts, then one back every few seconds: a typo goes
    // unnoticed, credential stuffing from one address does not.
    let login = RateLimitConfig::new(
        5,
        Duration::from_secs(5),
        ClientIpTrustPolicy::hops(args.trusted_hops),
    );

    let cfg = StartupConfig::new(args.server.listen_addr);
    let health =
        HealthState::new(cfg.shutdown_handle.clone()).with_checks(vec![Box::new(backend_check)]);

    // The stack is applied here rather than by serve, because a router
    // with realtime routes needs realtime_stack on those and http_stack on
    // the rest. StackConfig's defaults give a 30s timeout and a 2 MiB body
    // limit; an upload route would be layered separately.
    let app = router(state.clone(), &login)
        .layer(http_stack(StackConfig::default()))
        .merge(realtime_router(state).layer(realtime_stack()))
        // Outside the stack on purpose: inside it, the 503 that /ready returns
        // while draining is classified as a failure and logged at ERROR on
        // every rolling deploy.
        .merge(health_router().with_state(health))
        // The spec at /openapi.json and a Scalar page at /docs. Public and
        // outside the stack, like health: a spec fetch is not a business call
        // and wants neither the deadline nor the body limit. The committed
        // web/openapi.json is the same document, produced offline by
        // ./openapi.sh for the CI drift check.
        .merge(openapi_router(openapi(), &OpenApiConfig::default()))
        // So `web/static/index.html`, served from a plain local static server,
        // can call this gateway across origins. Loopback only - see
        // `cors_localhost`'s own doc for why that never belongs in production.
        .layer(cors_localhost(&[]));

    serve(cfg, app).await?;
    Ok(())
}
