use toolbox_core::{ErrorKind, ServiceError};
use toolbox_schedule::ScheduleError;

#[test]
fn each_variant_names_the_job_or_expression_in_its_message() {
    assert!(
        ScheduleError::NotFound("nightly".to_owned())
            .to_string()
            .contains("nightly")
    );
    assert!(
        ScheduleError::DuplicateName("nightly".to_owned())
            .to_string()
            .contains("already registered")
    );
    assert!(
        ScheduleError::Cron {
            expr: "not a cron".to_owned(),
            reason: "bad".to_owned(),
        }
        .to_string()
        .contains("not a cron")
    );
}

/// The kind is what a transport maps to a status: a missing job is a 404, a
/// lock failure is a 503, a bad expression is a 400.
#[test]
fn the_kind_matches_what_the_failure_actually_is() {
    assert_eq!(
        ScheduleError::NotFound("x".to_owned()).kind(),
        ErrorKind::NotFound
    );
    assert_eq!(
        ScheduleError::Lock("down".to_owned()).kind(),
        ErrorKind::Unavailable
    );
    assert_eq!(
        ScheduleError::DuplicateName("x".to_owned()).kind(),
        ErrorKind::InvalidArgument
    );
}

#[test]
fn the_job_name_is_carried_as_metadata_where_there_is_one() {
    let meta = ScheduleError::NotFound("nightly".to_owned()).metadata();
    assert_eq!(meta.get("job").map(String::as_str), Some("nightly"));
    assert!(
        ScheduleError::Lock("down".to_owned()).metadata().is_empty(),
        "a lock failure has no job to name"
    );
}
