use toolbox_core::ServiceError;

#[derive(Debug, thiserror::Error, ServiceError)]
#[service_error(status)]
enum E {
    #[error("boom")]
    #[service_error(code = "BOOM", kind = Internal)]
    Boom,
}

fn main() {}
