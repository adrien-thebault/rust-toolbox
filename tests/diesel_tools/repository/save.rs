//! tests `src/diesel_tools/repository/save.rs`: the `impl_save!`-generated
//! upsert (both flavors) and the transactional `save_all` default.

use crate::common::{self, Gadget, GadgetRepository, WidgetRepository, widget};
use rust_toolbox::diesel_tools::{Find, Save};

#[test]
fn save_inserts_then_updates_by_id() {
    let Some(mut db) = common::setup() else {
        return;
    };

    let saved = WidgetRepository::save(&mut db.conn, &widget(1, "alpha", 3)).unwrap();
    assert_eq!(saved, widget(1, "alpha", 3));
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 1);

    // saving again with the same id is an update, not a duplicate
    let updated = WidgetRepository::save(&mut db.conn, &widget(1, "alpha-renamed", 5)).unwrap();
    assert_eq!(updated, widget(1, "alpha-renamed", 5));
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 1);
    assert_eq!(
        WidgetRepository::find_by_id(&mut db.conn, &1).unwrap(),
        Some(widget(1, "alpha-renamed", 5))
    );
}

#[test]
fn save_all_rolls_back_the_whole_batch_on_failure() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save(&mut db.conn, &widget(1, "solo", 1)).unwrap();

    // the second entity violates CHECK (rank >= 0) - the first, valid one
    // must be rolled back with it
    let result =
        WidgetRepository::save_all(&mut db.conn, &[widget(2, "fresh", 1), widget(3, "bad", -1)]);
    assert!(result.is_err());
    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 1);
    assert!(!WidgetRepository::exists_by_id(&mut db.conn, &2).unwrap());
}

#[test]
fn autoincrement_save_assigns_ids_and_upserts_by_them() {
    let Some(mut db) = common::setup() else {
        return;
    };

    let first = GadgetRepository::save(
        &mut db.conn,
        &Gadget {
            id: None,
            name: "one".to_string(),
        },
    )
    .unwrap();
    let second = GadgetRepository::save(
        &mut db.conn,
        &Gadget {
            id: None,
            name: "two".to_string(),
        },
    )
    .unwrap();

    let first_id = first.id.expect("db-assigned id");
    let second_id = second.id.expect("db-assigned id");
    assert_ne!(first_id, second_id);
    assert_eq!(GadgetRepository::count(&mut db.conn).unwrap(), 2);

    // a known id upserts instead of inserting a third row
    let renamed = GadgetRepository::save(
        &mut db.conn,
        &Gadget {
            id: Some(first_id),
            name: "one-renamed".to_string(),
        },
    )
    .unwrap();
    assert_eq!(renamed.id, Some(first_id));
    assert_eq!(GadgetRepository::count(&mut db.conn).unwrap(), 2);
    assert_eq!(
        GadgetRepository::find_by_id(&mut db.conn, &first_id)
            .unwrap()
            .expect("still there")
            .name,
        "one-renamed"
    );
}
