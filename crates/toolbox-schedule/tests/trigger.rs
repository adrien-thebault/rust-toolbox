use std::time::Duration;

use chrono::{DateTime, Utc};
use toolbox_schedule::{ScheduleError, Trigger};

fn at(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

#[test]
fn a_five_field_unix_expression_parses() {
    assert!(Trigger::cron("0 3 * * *").is_ok());
}

/// Six fields carry seconds, five do not; both parse without a flag.
#[test]
fn a_six_field_quartz_expression_parses_too() {
    assert!(Trigger::cron("30 0 3 * * *").is_ok());
}

/// The expression is read in UTC. Named zones are not supported, so a job
/// pinned to a local hour drifts with that zone's offset - which is the price
/// of not carrying a DST policy.
#[test]
fn a_daily_cron_fires_at_its_utc_hour() {
    let trigger = Trigger::cron("0 3 * * *").unwrap();
    let started = at("2026-01-15T00:00:00Z");
    let next = trigger.next_after(started, started).unwrap();
    assert_eq!(next, at("2026-01-15T03:00:00Z"));

    // The same expression, six months later, at the same instant of the day.
    let summer = at("2026-07-15T00:00:00Z");
    assert_eq!(
        trigger.next_after(summer, summer).unwrap(),
        at("2026-07-15T03:00:00Z")
    );
}

/// A typo must be a startup failure, not a job that silently never runs.
#[test]
fn a_malformed_expression_is_refused_at_registration() {
    let err = Trigger::cron("not a cron").unwrap_err();
    assert!(matches!(err, ScheduleError::Cron { .. }), "{err:?}");
}

#[test]
fn a_fixed_rate_trigger_advances_by_its_period() {
    let trigger = Trigger::fixed_rate(Duration::from_secs(300));
    let started = at("2026-01-01T00:00:00Z");
    let next = trigger.next_after(started, started).unwrap();
    assert_eq!(next, at("2026-01-01T00:05:00Z"));
}

/// A run that overran its nominal due time must not push a fixed-rate
/// schedule back: it counts from the start, not the end, of a run.
#[test]
fn a_fixed_rate_trigger_ignores_how_long_the_run_took() {
    let trigger = Trigger::fixed_rate(Duration::from_secs(300));
    let started = at("2026-01-01T00:00:00Z");
    let completed = at("2026-01-01T00:04:00Z"); // this run took 4 minutes
    assert_eq!(
        trigger.next_after(started, completed).unwrap(),
        at("2026-01-01T00:05:00Z")
    );
}

#[test]
fn a_fixed_delay_trigger_advances_by_its_delay_from_completion() {
    let trigger = Trigger::fixed_delay(Duration::from_secs(60));
    let started = at("2026-01-01T00:00:00Z");
    let completed = at("2026-01-01T00:04:00Z"); // this run took 4 minutes
    assert_eq!(
        trigger.next_after(started, completed).unwrap(),
        at("2026-01-01T00:05:00Z")
    );
}

/// A delay too large for a `chrono::TimeDelta` is a misconfiguration to
/// report, not one to silently reinterpret as some made-up default gap.
#[test]
fn a_duration_too_large_for_a_time_delta_is_reported_not_guessed() {
    let trigger = Trigger::fixed_delay(Duration::MAX);
    let started = at("2026-01-01T00:00:00Z");
    let err = trigger.next_after(started, started).unwrap_err();
    assert!(
        matches!(err, ScheduleError::DurationOutOfRange(_)),
        "{err:?}"
    );
}

#[test]
fn every_trigger_describes_itself_for_the_startup_log() {
    assert!(
        Trigger::cron("0 3 * * *")
            .unwrap()
            .describe()
            .contains("UTC")
    );
    assert!(
        Trigger::fixed_rate(Duration::from_secs(60))
            .describe()
            .contains("every")
    );
    assert!(
        Trigger::fixed_delay(Duration::from_secs(60))
            .describe()
            .contains("previous"),
        "the description says which of the two it is"
    );
}
