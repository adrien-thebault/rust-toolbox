mod identity;
mod shared_secret;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use toolbox_grpc::{RoutesBuilder, ServerConfig, serve};
use toolbox_server::{lifecycle::ReadinessCheck, startup::StartupConfig};

/// `serve` binds the listener and runs the tonic serve loop until it is
/// cancelled - it must not return on its own.
#[tokio::test]
async fn serve_binds_and_stays_up() {
    // Take a free port, then hand it to `serve`.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let cfg = StartupConfig::new(addr);

    tokio::select! {
        result = serve(cfg, ServerConfig::default(), RoutesBuilder::default()) => {
            panic!("serve exited early: {result:?}")
        }
        () = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
}

/// A readiness check flipped by the test.
struct Toggle(Arc<AtomicBool>);

impl ReadinessCheck for Toggle {
    fn name(&self) -> &'static str {
        "toggle"
    }
    fn is_ready(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The gRPC health probe for the empty service name tracks the readiness
/// checks, so a backend whose dependency is down is pulled from rotation the
/// same way the axum `/ready` route does it.
#[tokio::test]
async fn the_health_probe_follows_the_readiness_checks() {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let flag = Arc::new(AtomicBool::new(false));
    let cfg = StartupConfig::new(addr);
    let server =
        ServerConfig::default().readiness_checks(vec![Box::new(Toggle(Arc::clone(&flag)))]);

    tokio::select! {
        result = serve(cfg, server, RoutesBuilder::default()) => panic!("serve exited: {result:?}"),
        outcome = probe_health_transitions(addr, &flag) => outcome,
    }
}

/// Connect a health client, watch the status while the flag is flipped.
async fn probe_health_transitions(addr: std::net::SocketAddr, flag: &AtomicBool) {
    use tonic_health::pb::{
        HealthCheckRequest, health_check_response::ServingStatus, health_client::HealthClient,
    };

    let channel = loop {
        let endpoint = tonic::transport::Channel::from_shared(format!("http://{addr}")).unwrap();
        match endpoint.connect().await {
            Ok(channel) => break channel,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    let client = HealthClient::new(channel);
    let check = || async {
        client
            .clone()
            .check(HealthCheckRequest {
                service: String::new(),
            })
            .await
            .unwrap()
            .into_inner()
            .status()
    };

    assert_eq!(
        check().await,
        ServingStatus::NotServing,
        "the check starts failing, so the probe must not serve"
    );

    flag.store(true, Ordering::SeqCst);
    // One poll interval plus slack; the interval is a private constant.
    tokio::time::sleep(Duration::from_millis(2_500)).await;

    assert_eq!(
        check().await,
        ServingStatus::Serving,
        "the check recovered, so the probe serves again"
    );
}
