//! The HTTP gateway.

use std::{error::Error, time::Duration};

use clap::Parser;
use toolbox::{
    grpc::{BackoffConfig, ClientConfig, RetryPolicy, client, client::poll_health},
    server::{ServerBuilder, args::ServerArgs, stack::StackConfig, telemetry::TelemetryArgs},
    web::{
        ClientIpTrustPolicy, PRIVATE_RANGES, WebServerConfig, apply_http_stack,
        apply_realtime_stack, rate_limit::RateLimitConfig, serve,
    },
};
use {{crate_name}}_web::{
    auth::AuthConfig,
    routes::{openapi, realtime_router, router, todo::forward_events},
    state::state,
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
    /// Authentication and session settings.
    #[command(flatten)]
    auth: AuthConfig,

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    args.telemetry.init()?;

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
    let backend_probe = poll_health(todos.clone(), "backend", Duration::from_secs(5));

    let state = state(todos, &args.auth)?;

    // A handful of attempts, then one back every few seconds: a typo goes
    // unnoticed, credential stuffing from one address does not.
    let login = RateLimitConfig::new(
        5,
        Duration::from_secs(5),
        ClientIpTrustPolicy::BehindProxies(PRIVATE_RANGES.to_vec()),
    );

    // The stack is applied here rather than by serve, because a router
    // with realtime routes needs the realtime stack on those and the HTTP
    // stack on the rest. StackConfig's defaults give a 30s timeout and a 2 MiB body
    // limit; an upload route would be layered separately. `/health`, `/ready`,
    // `/openapi.json`, `/docs` and CORS are added at the root by serve.
    let app = apply_http_stack(router(state.clone(), &login), StackConfig::default())
        .merge(apply_realtime_stack(realtime_router(state.clone())));

    let server = ServerBuilder::listening_on(args.server.listen_addr)
        .check(backend_probe)
        // Hold one WatchTodos stream open against the backend and fan its
        // events into the local SSE hub, for the life of the process; aborted
        // on `SIGTERM`.
        .task(forward_events(state.todos.clone(), state.hub.clone()))
        .build()
        .await?;

    serve(
        server,
        WebServerConfig::default()
            // So `web/static/index.html`, served from a plain local static
            // server, can call this gateway across origins. Loopback only -
            // see `cors_localhost`'s own doc for why that never ships.
            .cors_localhost(&[])
            // The spec at /openapi.json and a Scalar page at /docs. The
            // committed web/openapi.json is the same document, produced
            // offline by ./openapi.sh for the CI drift check.
            .openapi(openapi()),
        app,
    )
    .await?;
    Ok(())
}
