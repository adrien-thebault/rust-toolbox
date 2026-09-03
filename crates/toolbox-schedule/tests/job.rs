use toolbox_schedule::{JobOutcome, Overlap, RunMode};

#[test]
fn every_outcome_has_a_stable_metric_label() {
    for (outcome, label) in [
        (JobOutcome::Succeeded, "succeeded"),
        (JobOutcome::Failed, "failed"),
        (JobOutcome::TimedOut, "timed_out"),
        (JobOutcome::Skipped, "skipped"),
        (JobOutcome::Overlapped, "overlapped"),
    ] {
        assert_eq!(outcome.as_label(), label);
    }
}

/// `ran` is what decides whether a duration histogram is recorded, so a run
/// that never started must report `false`.
#[test]
fn only_outcomes_that_executed_the_body_count_as_having_run() {
    assert!(JobOutcome::Succeeded.ran());
    assert!(JobOutcome::Failed.ran());
    assert!(JobOutcome::TimedOut.ran());
    assert!(!JobOutcome::Skipped.ran());
    assert!(!JobOutcome::Overlapped.ran());
}

/// The defaults are the safe-for-anything-that-writes ones: exactly-once, and
/// do not start a second run over the first.
#[test]
fn the_defaults_are_the_ones_that_do_not_corrupt_data() {
    assert_eq!(RunMode::default(), RunMode::Exclusive);
    assert_eq!(Overlap::default(), Overlap::Skip);
}
