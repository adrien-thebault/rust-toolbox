use std::time::Duration;

use toolbox_grpc::{ClientConfig, RoutesBuilder, ServerConfig, client, client::poll_health, serve};
use toolbox_server::{StartupConfig, lifecycle::HealthCheck};

/// Real bound server, real client, no shared secret: `poll_health` should
/// need nothing more than a serving backend to report healthy.
#[tokio::test]
async fn poll_health_reports_a_serving_backend_as_healthy() {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    tokio::spawn(serve(
        StartupConfig::new(addr),
        ServerConfig::default(),
        RoutesBuilder::default(),
    ));

    let cfg = ClientConfig::new(&format!("http://{addr}")).unwrap();
    let channel = client("backend", &cfg);
    let (check, task) = poll_health(channel, "backend", Duration::from_millis(20));
    tokio::spawn(task);

    for _ in 0..200 {
        if check.is_healthy() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("poll_health never reported the backend healthy");
}
