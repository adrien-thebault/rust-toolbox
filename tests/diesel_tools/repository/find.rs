//! tests `src/diesel_tools/repository/find.rs`: the `impl_find!`-generated
//! `count`/`exists_by_id*`/`find_by_id*`/`find_all` against a real database.

use crate::common::{self, Widget, WidgetRepository, widget};
use rust_toolbox::diesel_tools::{Find, Page, PageRequest, Save, Sort, SortDirection};

fn ids(page: &Page<Widget>) -> Vec<i32> {
    page.data.iter().map(|w| w.id).collect()
}

#[test]
fn count_exists_and_find_by_id() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(&mut db.conn, &[widget(1, "a", 1), widget(2, "b", 2)]).unwrap();

    assert_eq!(WidgetRepository::count(&mut db.conn).unwrap(), 2);

    assert!(WidgetRepository::exists_by_id(&mut db.conn, &1).unwrap());
    assert!(!WidgetRepository::exists_by_id(&mut db.conn, &42).unwrap());
    // one entry per requested id, in the order they were asked for
    assert_eq!(
        WidgetRepository::exists_by_id_in(&mut db.conn, &[42, 1]).unwrap(),
        vec![(42, false), (1, true)]
    );

    assert_eq!(
        WidgetRepository::find_by_id(&mut db.conn, &1).unwrap(),
        Some(widget(1, "a", 1))
    );
    assert_eq!(
        WidgetRepository::find_by_id(&mut db.conn, &42).unwrap(),
        None
    );

    let found = WidgetRepository::find_by_id_in(&mut db.conn, &[1, 2, 42]).unwrap();
    assert_eq!(found.len(), 2);
}

#[test]
fn find_all_applies_multi_field_sort_and_pagination() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(
        &mut db.conn,
        &[widget(1, "c", 2), widget(2, "a", 1), widget(3, "b", 2)],
    )
    .unwrap();

    // rank asc first, then name desc within equal ranks: a(2), c(1), b(3)
    let sort = Sort::Sorted {
        items: vec![
            ("rank".to_string(), SortDirection::Asc),
            ("name".to_string(), SortDirection::Desc),
        ],
    };

    let all = WidgetRepository::find_all(&mut db.conn, PageRequest::Unpaged { sort: sort.clone() })
        .unwrap();
    assert_eq!(ids(&all), vec![2, 1, 3]);
    assert_eq!(all.total_elements, 3);
    assert!(all.next_page().is_none());

    // first page of two: same order, truncated, and next_page picks up
    // exactly where it left off
    let first =
        WidgetRepository::find_all(&mut db.conn, PageRequest::paged(0, 2, sort).unwrap()).unwrap();
    assert_eq!(ids(&first), vec![2, 1]);
    assert_eq!(first.total_elements, 3);

    let second =
        WidgetRepository::find_all(&mut db.conn, first.next_page().expect("a second page"))
            .unwrap();
    assert_eq!(ids(&second), vec![3]);
    assert!(second.next_page().is_none());
    assert!(second.previous_page().is_some());
}

#[test]
fn negative_page_size_is_clamped_to_an_empty_page_not_the_whole_table() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(&mut db.conn, &[widget(1, "a", 1), widget(2, "b", 2)]).unwrap();

    // SQLite treats a negative LIMIT as "no limit" - the clamp in
    // apply_page_request must keep a hostile size from dumping the table
    let page = WidgetRepository::find_all(
        &mut db.conn,
        PageRequest::Paged {
            offset: 0,
            size: -1,
            sort: Sort::Unsorted,
        },
    )
    .unwrap();
    assert!(page.is_empty());
    assert_eq!(page.total_elements, 2);
}

#[test]
fn unknown_sort_fields_are_ignored_not_fatal() {
    let Some(mut db) = common::setup() else {
        return;
    };
    WidgetRepository::save_all(&mut db.conn, &[widget(1, "a", 1), widget(2, "b", 2)]).unwrap();

    let page = WidgetRepository::find_all(
        &mut db.conn,
        PageRequest::Unpaged {
            sort: Sort::Sorted {
                items: vec![("no_such_column".to_string(), SortDirection::Asc)],
            },
        },
    )
    .unwrap();
    assert_eq!(page.len(), 2);
}
