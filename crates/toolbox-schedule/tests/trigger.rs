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

/// A project migrating from Spring writes six fields; one writing Unix cron
/// writes five. Both should work without a flag.
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
    let next = trigger.next_after(at("2026-01-15T00:00:00Z")).unwrap();
    assert_eq!(next, at("2026-01-15T03:00:00Z"));

    // The same expression, six months later, at the same instant of the day.
    let summer = trigger.next_after(at("2026-07-15T00:00:00Z")).unwrap();
    assert_eq!(summer, at("2026-07-15T03:00:00Z"));
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
    let next = trigger.next_after(at("2026-01-01T00:00:00Z")).unwrap();
    assert_eq!(next, at("2026-01-01T00:05:00Z"));
}

#[test]
fn a_fixed_delay_trigger_advances_by_its_delay() {
    let trigger = Trigger::fixed_delay(Duration::from_secs(60));
    let next = trigger.next_after(at("2026-01-01T00:00:00Z")).unwrap();
    assert_eq!(next, at("2026-01-01T00:01:00Z"));
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
