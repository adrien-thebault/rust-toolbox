use toolbox_cluster::{Adapter, Deployment, InProcessEventBus, Scope};
use toolbox_server::startup::{StartupConfig, StartupError, bind};

#[tokio::test]
async fn binding_checks_the_deployment_before_it_binds() {
    let bus = InProcessEventBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus];
    let deployment = Deployment::Clustered {
        instance_id: "a".to_owned(),
    };
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap(), &deployment).adapters(&adapters);

    let err = bind(&cfg).await.unwrap_err();
    assert!(matches!(err, StartupError::Deployment(_)), "{err:?}");
}

#[tokio::test]
async fn binding_succeeds_with_an_acceptable_deployment() {
    let deployment = Deployment::Single;
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap(), &deployment);
    let listener = bind(&cfg).await.unwrap();
    assert!(listener.local_addr().unwrap().port() > 0);
}

#[tokio::test]
async fn a_shared_adapter_binds_under_clustering() {
    struct Shared;
    impl Adapter for Shared {
        fn name(&self) -> &'static str {
            "Shared"
        }
        fn scope(&self) -> Scope {
            Scope::Shared
        }
    }

    let shared = Shared;
    let adapters: Vec<&dyn Adapter> = vec![&shared];
    let deployment = Deployment::Clustered {
        instance_id: "a".to_owned(),
    };
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap(), &deployment).adapters(&adapters);
    assert!(bind(&cfg).await.is_ok());
}
