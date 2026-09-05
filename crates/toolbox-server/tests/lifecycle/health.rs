use toolbox_server::lifecycle::Health;

#[test]
fn is_healthy_is_true_for_healthy_alone() {
    assert!(Health::Healthy.is_healthy());
    assert!(!Health::Starting.is_healthy());
    assert!(!Health::Degraded.is_healthy());
    assert!(!Health::ShuttingDown.is_healthy());
}
