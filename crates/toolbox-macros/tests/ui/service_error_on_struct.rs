use toolbox_core::ServiceError;

#[derive(Debug, thiserror::Error, ServiceError)]
#[error("nope")]
#[service_error(domain = "x")]
struct NotAnEnum;

fn main() {}
