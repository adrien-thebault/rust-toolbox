use std::time::SystemTime;

use toolbox_schedule::{Clock, SystemClock};

#[test]
fn the_system_clock_reports_the_real_time() {
    let before = SystemTime::now();
    let now = SystemClock.now();
    assert!(now >= before);
}
