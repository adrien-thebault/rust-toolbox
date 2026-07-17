//! single integration-test harness whose module tree mirrors `src/`:
//! `tests/diesel_tools/repository/find.rs` tests
//! `src/diesel_tools/repository/find.rs`, and so on. Unit tests that need
//! access to private items (jwt claims handling, `ApiError` internals,
//! pagination arithmetic, the macro compile-checks) stay next to the code
//! they test, in `#[cfg(test)]` modules under `src/`.
//!
//! The diesel tests are backend-generic: under `sqlite` they run against a
//! per-test in-memory database, under `postgresql`/`mysql` they run against
//! the server named by `TOOLBOX_TEST_POSTGRES_URL`/`TOOLBOX_TEST_MYSQL_URL`
//! (and soft-skip with a note when the variable isn't set) - see
//! `common/mod.rs`.

#[cfg(feature = "diesel")]
mod common;

#[cfg(feature = "diesel")]
mod diesel_tools {
    mod repository {
        mod delete;
        mod find;
        mod save;
    }
    mod service {
        mod entity_service;
    }
}

#[cfg(feature = "axum")]
mod axum_tools {
    mod auth;
}

#[cfg(feature = "tower")]
mod tower_tools {
    mod layers {
        mod request_id;
    }
}

#[cfg(feature = "mail")]
mod mail_tools;
