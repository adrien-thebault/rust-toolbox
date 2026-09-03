use diesel::sqlite::SqliteConnection;
use toolbox_test::temp_db;

#[tokio::test]
async fn a_temp_db_is_private_to_the_test_that_made_it() {
    let (a, guard_a) = temp_db::<SqliteConnection>();
    let (b, guard_b) = temp_db::<SqliteConnection>();
    assert_ne!(
        guard_a.path(),
        guard_b.path(),
        "two tests must not share a file"
    );
    assert!(guard_a.path().exists());

    // Both are usable.
    a.query(|_c: &mut SqliteConnection| Ok(1_i32))
        .await
        .unwrap();
    b.query(|_c: &mut SqliteConnection| Ok(1_i32))
        .await
        .unwrap();
}

#[test]
fn a_temp_db_deletes_itself() {
    let path = {
        let (_db, guard) = temp_db::<SqliteConnection>();
        let path = guard.path();
        assert!(path.exists());
        path
    };
    assert!(!path.exists(), "the file went away with the guard");
}
