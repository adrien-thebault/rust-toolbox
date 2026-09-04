use std::time::Duration;

use axum::{Router, routing::get};
use toolbox_server::StartupConfig;
use toolbox_web::server::serve;

/// `serve` binds the listener and runs the axum serve loop until it is
/// cancelled - it must not return on its own.
#[tokio::test]
async fn serve_binds_and_stays_up() {
    // Take a free port, then hand it to `serve`.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let cfg = StartupConfig::new(addr);
    let app = Router::new().route("/ping", get(|| async { "pong" }));

    tokio::select! {
        result = serve(cfg, app) => panic!("serve exited early: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
}
