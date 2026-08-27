use toolbox_cluster::{Adapter, Deployment, InProcessBus, Scope};
use toolbox_server::{
    args::DeploymentArgs,
    serve::{ServeConfig, ServeError, bind},
};

fn deployment_args(value: &str) -> DeploymentArgs {
    DeploymentArgs {
        deployment: value.to_owned(),
        instance_id: None,
    }
}

#[test]
fn deployment_resolves_from_its_argument() {
    assert_eq!(
        deployment_args("single").resolve().unwrap(),
        Deployment::Single
    );
    assert!(
        deployment_args("CLUSTERED")
            .resolve()
            .unwrap()
            .is_clustered()
    );
}

#[test]
fn a_clustered_deployment_gets_an_instance_id_even_when_none_was_given() {
    let resolved = deployment_args("clustered").resolve().unwrap();
    assert!(resolved.instance_id().is_some_and(|id| !id.is_empty()));
}

#[test]
fn an_explicit_instance_id_is_kept() {
    let args = DeploymentArgs {
        deployment: "clustered".to_owned(),
        instance_id: Some("pod-7".to_owned()),
    };
    assert_eq!(args.resolve().unwrap().instance_id(), Some("pod-7"));
}

/// Guessing here would defeat the guard, so an unrecognised value is an error.
#[test]
fn an_unknown_deployment_is_rejected_rather_than_defaulted() {
    let err = deployment_args("multi").resolve().unwrap_err();
    assert!(
        err.to_string().contains("expected `single` or `clustered`"),
        "{err}"
    );
}

#[tokio::test]
async fn binding_checks_the_deployment_before_it_binds() {
    let bus = InProcessBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus];
    let deployment = Deployment::Clustered {
        instance_id: "a".to_owned(),
    };
    let cfg = ServeConfig::new("127.0.0.1:0".parse().unwrap(), &deployment).adapters(&adapters);

    let err = bind(&cfg).await.unwrap_err();
    assert!(matches!(err, ServeError::Deployment(_)), "{err:?}");
}

#[tokio::test]
async fn binding_succeeds_with_an_acceptable_deployment() {
    let deployment = Deployment::Single;
    let cfg = ServeConfig::new("127.0.0.1:0".parse().unwrap(), &deployment);
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
    let cfg = ServeConfig::new("127.0.0.1:0".parse().unwrap(), &deployment).adapters(&adapters);
    assert!(bind(&cfg).await.is_ok());
}
