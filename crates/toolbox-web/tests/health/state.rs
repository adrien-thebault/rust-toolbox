use toolbox_server::lifecycle::Shutdown;
use toolbox_web::health::{HealthState, ReadinessCheck};

struct Always(&'static str);
impl ReadinessCheck for Always {
    fn name(&self) -> &'static str {
        self.0
    }
    fn is_ready(&self) -> bool {
        true
    }
}

/// A `Debug` of the state ends up in a startup log, so it must show the check
/// count and nothing that would be noise or a leak.
#[test]
fn debug_shows_the_check_count_and_stays_non_exhaustive() {
    let state = HealthState::new(Shutdown::new())
        .with_checks(vec![Box::new(Always("db")), Box::new(Always("cache"))]);
    let rendered = format!("{state:?}");
    assert!(rendered.contains("checks: 2"), "{rendered}");
    assert!(rendered.contains(".."), "finish_non_exhaustive");
    assert!(
        !rendered.contains("watch"),
        "the raw drain channel is not printed"
    );
}

#[test]
fn a_fresh_state_has_no_extra_checks() {
    let state = HealthState::new(Shutdown::new());
    assert!(format!("{state:?}").contains("checks: 0"));
}
