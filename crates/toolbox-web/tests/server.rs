use std::time::Duration;

use axum::{Router, routing::get};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use toolbox_server::ServerBuilder;
use toolbox_web::{WebServerConfig, server::serve};

/// Bind a free port, hand it back released so `build` can take it.
async fn free_addr() -> std::net::SocketAddr {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    addr
}

/// One raw HTTP/1.1 GET, returning the status line.
async fn get_status(addr: std::net::SocketAddr, path: &str) -> String {
    let mut stream = loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(s) => break s,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    text.lines().next().unwrap_or_default().to_owned()
}

/// `serve` runs the axum serve loop until it is cancelled - it must not
/// return on its own.
#[tokio::test]
async fn serve_stays_up() {
    let addr = free_addr().await;
    let server = ServerBuilder::listening_on(addr).build().await.unwrap();
    let app = Router::new().route("/ping", get(|| async { "pong" }));

    tokio::select! {
        result = serve(server, WebServerConfig::default(), app) => {
            panic!("serve exited early: {result:?}")
        }
        () = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
}

/// `serve` merges `/health` and `/ready` at the root, so a caller never wires
/// the health router itself.
#[tokio::test]
async fn serve_merges_health_at_the_root() {
    let addr = free_addr().await;
    let server = ServerBuilder::listening_on(addr).build().await.unwrap();
    let app = Router::new().route("/ping", get(|| async { "pong" }));

    tokio::select! {
        _ = serve(server, WebServerConfig::default(), app) => panic!("serve exited early"),
        line = get_status(addr, "/health") => {
            assert!(line.contains("200"), "GET /health -> {line:?}");
        }
    }
}
