mod health;
mod lifecycle;
mod shutdown;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use toolbox_server::server::{ServerBuilder, Shutdown, poll_check};

#[tokio::test]
async fn build_binds_the_listener_and_folds_probe_checks_into_the_lifecycle() {
    let shutdown = Shutdown::new();
    let server = ServerBuilder::listening_on("127.0.0.1:0".parse().unwrap())
        .shutdown_handle(shutdown.clone())
        .drain_after(Duration::from_secs(1))
        .check(poll_check("dep", Duration::from_secs(5), (), |()| async {
            true
        }))
        .task(std::future::pending())
        .build()
        .await
        .expect("bind");

    assert!(server.listener.local_addr().unwrap().port() > 0);
    assert_eq!(server.drain.drain_delay, Duration::from_secs(1));
    assert_eq!(
        server.lifecycle.checks().len(),
        1,
        "the probe check is registered"
    );
    // The shared handle is only flipped by a drain, which has not run.
    assert!(!shutdown.is_shutting_down());
}

#[tokio::test]
async fn build_spawns_the_tasks_and_dropping_the_server_aborts_them() {
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    let server = ServerBuilder::listening_on("127.0.0.1:0".parse().unwrap())
        .check(poll_check(
            "dep",
            Duration::from_millis(10),
            (),
            |()| async { true },
        ))
        .task(async move {
            flag.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
        })
        .build()
        .await
        .expect("bind");

    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(ran.load(Ordering::SeqCst), "the task was spawned");

    // Dropping the `Server` drops its `TaskGuard`, which aborts the probe loop
    // and the pending task. Nothing to assert beyond "does not panic".
    drop(server);
}
