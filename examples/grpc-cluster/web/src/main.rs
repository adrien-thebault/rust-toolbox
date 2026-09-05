//! The gateway process.
//!
//! It owns authentication, rate limiting and the RFC 9457 error shape; the
//! backend owns the data.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use clap::Parser;
use example_todo::proto::{ListTodosRequest, todo_service_client::TodoServiceClient};
use example_web::{
    auth::{self, AuthConfig},
    routes::{realtime_router, router},
};
use toolbox_grpc::{BackoffConfig, ClientConfig, RetryPolicy, client};
use toolbox_server::{
    HealthCheck, StartupConfig,
    args::ServerArgs,
    stack::{StackConfig, http_stack, realtime_stack},
    telemetry::TelemetryArgs,
};
use toolbox_web::{
    ClientIpTrustPolicy, cors_localhost,
    health::{HealthState, health_router},
    rate_limit::RateLimitConfig,
    serve,
};

/// Command-line arguments.
#[derive(Parser)]
#[command(name = "example-web")]
struct Args {
    /// Log format and level.
    #[command(flatten)]
    telemetry: TelemetryArgs,
    /// Listen address.
    #[command(flatten)]
    server: ServerArgs,

    /// Where the todo backend is.
    #[arg(long, env = "TODO_BACKEND", default_value = "http://127.0.0.1:50051")]
    todo_backend: String,

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

/// Reports the backend as healthy once it has answered a real call - not just
/// a TCP connect - and unhealthy again the moment one fails.
struct BackendCheck(Arc<AtomicBool>);

impl HealthCheck for BackendCheck {
    fn name(&self) -> &'static str {
        "todo-backend"
    }
    fn is_healthy(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Ping the backend every `interval` with a cheap, real call - the same path
/// exercises the shared secret and the standard stack, not just the socket.
async fn ping_backend(
    channel: toolbox_grpc::ClientChannel,
    healthy: Arc<AtomicBool>,
    interval: Duration,
) {
    let mut client = TodoServiceClient::new(channel.channel());
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let ok = client
            .list_todos(ListTodosRequest {
                page: None,
                title_contains: String::new(),
            })
            .await
            .is_ok();
        healthy.store(ok, Ordering::Relaxed);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let _telemetry = args.telemetry.init()?;

    let todos = client(
        "todo",
        &ClientConfig::new(&args.todo_backend)?
            .service_secret(&args.service_secret)
            // Reads only: retrying a create or a delete would risk a caller
            // seeing it happen twice.
            .retry(RetryPolicy::Idempotent {
                max_attempts: 3,
                backoff: BackoffConfig::default(),
                methods: &["GetTodo", "ListTodos"],
            }),
    );

    let backend_healthy = Arc::new(AtomicBool::new(false));
    tokio::spawn(ping_backend(
        todos.clone(),
        Arc::clone(&backend_healthy),
        Duration::from_secs(5),
    ));

    // Everything identity needs, read once at startup so a missing variable is
    // a refusal to start rather than a 500 on the first login.
    let config = AuthConfig::from_env()?;
    let state = auth::state(todos, &config)?;
    tokio::spawn(auth::forward_events(
        state.events.clone(),
        state.hub.clone(),
    ));

    // A handful of attempts, then one back every few seconds: a typo goes
    // unnoticed, credential stuffing from one address does not.
    let login = RateLimitConfig::new(
        5,
        Duration::from_secs(5),
        ClientIpTrustPolicy::hops(args.trusted_hops),
    );

    let cfg = StartupConfig::new(args.server.listen_addr);
    let health = HealthState::new(cfg.shutdown_handle.clone())
        .with_checks(vec![Box::new(BackendCheck(backend_healthy))]);

    // The stack is applied here, not by serve: a router with realtime
    // routes needs realtime_stack on those and http_stack on the rest.
    let app = router(state.clone(), &login)
        .layer(http_stack(StackConfig::default()))
        .merge(realtime_router(state).layer(realtime_stack()))
        // Outside the stack on purpose: inside it, the 503 that /ready returns
        // while draining is classified as a failure and logged at ERROR on
        // every rolling deploy.
        .merge(health_router().with_state(health))
        // So `web/static/index.html`, served from a plain local static server,
        // can call this gateway across origins. Loopback only - see
        // `cors_localhost`'s own doc for why that never belongs in production.
        .layer(cors_localhost(&[]));

    serve(cfg, app).await?;
    Ok(())
}
