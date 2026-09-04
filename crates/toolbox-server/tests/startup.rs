use toolbox_server::startup::{StartupConfig, bind};

#[tokio::test]
async fn binding_hands_out_a_listener_on_the_requested_address() {
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap());
    let listener = bind(&cfg).await.unwrap();
    assert!(listener.local_addr().unwrap().port() > 0);
}
