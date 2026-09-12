use toolbox_core::ServiceError;

#[derive(Debug, thiserror::Error, ServiceError)]
#[service_error(domain = "x")]
enum E {
    #[error("boom")]
    Boom,
}

fn main() {}
