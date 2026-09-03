use toolbox_cluster::Deployment;
use toolbox_server::args::DeploymentArgs;

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
