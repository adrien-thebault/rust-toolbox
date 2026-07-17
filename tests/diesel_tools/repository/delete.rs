//! tests `src/diesel_tools/repository/delete.rs`: the `impl_delete!`-generated
//! `delete_by_id`/`delete_by_id_in`/`clear`.

use crate::common::{self, WidgetRepository, widget};
use rust_toolbox::diesel_tools::{Delete, Find, Save};

#[test]
fn delete_by_id_removes_exactly_that_row() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(&mut db.conn, &[widget(1, "a", 1), widget(2, "b", 2)]).unwrap();

    assert_eq!(WidgetRepository::delete_by_id(&mut db.conn, &1).unwrap(), 1);
    assert_eq!(
        WidgetRepository::delete_by_id(&mut db.conn, &42).unwrap(),
        0
    );
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 1);
    assert_eq!(
        WidgetRepository::find_by_id(&mut db.conn, &1).unwrap(),
        None
    );
}

#[test]
fn delete_by_id_in_and_clear_remove_rows() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(
        &mut db.conn,
        &[widget(1, "a", 1), widget(2, "b", 2), widget(3, "c", 3)],
    )
    .unwrap();

    // unknown ids are skipped, not an error - the count says how many hit
    assert_eq!(
        WidgetRepository::delete_by_id_in(&mut db.conn, &[1, 3, 42]).unwrap(),
        2
    );
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 1);

    assert_eq!(WidgetRepository::clear(&mut db.conn).unwrap(), 1);
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 0);
}
