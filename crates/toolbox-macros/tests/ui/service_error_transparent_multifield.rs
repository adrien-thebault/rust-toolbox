use toolbox_error::ServiceError;

#[derive(Debug, thiserror::Error, ServiceError)]
#[service_error(domain = "x")]
enum E {
    #[error("two {0} {1}")]
    #[service_error(transparent)]
    Two(i32, i32),
}

fn main() {}
