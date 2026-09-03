use std::time::Duration;

use axum::{Router, routing::get};
use toolbox_cluster::{Adapter, Deployment, InProcessEventBus};
use toolbox_server::startup::{StartupConfig, StartupError};
use toolbox_web::server::serve;

/// `serve` binds the listener and runs the axum serve loop until it is
/// cancelled - it must not return on its own.
#[tokio::test]
async fn serve_binds_and_stays_up() {
    // Take a free port, then hand it to `serve`.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let deployment = Deployment::Single;
    let cfg = StartupConfig::new(addr, &deployment);
    let app = Router::new().route("/ping", get(|| async { "pong" }));

    tokio::select! {
        result = serve(cfg, app) => panic!("serve exited early: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
}

/// The deployment guard runs before the listener opens, so a single-replica
/// adapter under `DEPLOYMENT=clustered` is refused rather than served.
#[tokio::test]
async fn serve_refuses_a_single_replica_adapter_when_clustered() {
    let bus = InProcessEventBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus];
    let deployment = Deployment::Clustered {
        instance_id: "a".to_owned(),
    };
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap(), &deployment).adapters(&adapters);

    let err = serve(cfg, Router::new()).await.unwrap_err();
    assert!(matches!(err, StartupError::Deployment(_)), "{err:?}");
}
