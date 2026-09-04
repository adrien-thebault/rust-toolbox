use toolbox_test::TestCluster;

#[test]
fn an_empty_cluster_has_no_backends() {
    let cluster = TestCluster::new();
    assert_eq!(cluster.backend("todo"), None);
}

/// A started backend gets an ephemeral port, so two test binaries running at
/// once do not collide on a fixed one.
#[tokio::test]
async fn a_started_backend_is_reachable_by_name_on_an_ephemeral_port() {
    let cluster = TestCluster::new()
        .service("todo", |_routes| {
            // No services mounted: the port still binds and accepts, which is
            // all this asserts.
        })
        .await
        .unwrap();

    let addr = cluster.backend("todo").expect("an address");
    assert!(addr.port() > 0);
    assert!(cluster.backend_uri("todo").starts_with("http://"));
}
