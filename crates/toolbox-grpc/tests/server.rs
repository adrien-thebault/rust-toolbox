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
use toolbox_server::{StartupConfig, lifecycle::HealthCheck};

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

/// A health check flipped by the test.
struct Toggle(Arc<AtomicBool>);

impl HealthCheck for Toggle {
    fn name(&self) -> &'static str {
        "toggle"
    }
    fn is_healthy(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A descriptor set that does not decode disables reflection with a warning
/// rather than failing startup: a broken reflection blob must not be able to
/// take the whole server down.
#[tokio::test]
async fn a_bad_reflection_descriptor_does_not_stop_the_server() {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let server = ServerConfig::default().reflection(b"not a descriptor set");

    tokio::select! {
        result = serve(StartupConfig::new(addr), server, RoutesBuilder::default()) => {
            panic!("serve exited early: {result:?}")
        }
        () = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
}

/// The gRPC health probe for the empty service name tracks the health
/// checks, so a backend whose dependency is down is pulled from rotation the
/// same way the axum `/ready` route does it.
#[tokio::test]
async fn the_health_probe_follows_the_health_checks() {
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

/// `ServerConfig::health_secret` gates the health service exactly like
/// `shared_secret_layer` gates any other service - proven here rather than
/// assumed, since the wiring inside `serve` is new.
#[tokio::test]
async fn health_secret_gates_the_health_service() {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let cfg = StartupConfig::new(addr);
    let server = ServerConfig::default().health_secret("s3cr3t");

    tokio::select! {
        result = serve(cfg, server, RoutesBuilder::default()) => panic!("serve exited: {result:?}"),
        () = assert_health_requires_the_secret(addr) => {}
    }
}

/// Connect a raw health client - no `ClientChannel`, no interceptor - so the
/// secret has to be attached by hand, proving the server checks it rather
/// than trusting whoever happens to call.
async fn assert_health_requires_the_secret(addr: std::net::SocketAddr) {
    use tonic_health::pb::{HealthCheckRequest, health_client::HealthClient};

    let channel = loop {
        let endpoint = tonic::transport::Channel::from_shared(format!("http://{addr}")).unwrap();
        match endpoint.connect().await {
            Ok(channel) => break channel,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };

    let without_secret = HealthClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await;
    assert!(without_secret.is_err(), "no secret was presented");

    let mut req = tonic::Request::new(HealthCheckRequest {
        service: String::new(),
    });
    req.metadata_mut()
        .insert(toolbox_grpc::X_SHARED_SECRET, "s3cr3t".parse().unwrap());
    HealthClient::new(channel)
        .check(req)
        .await
        .expect("the correct secret is accepted");
}
