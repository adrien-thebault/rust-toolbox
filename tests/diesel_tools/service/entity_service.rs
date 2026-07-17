//! tests `src/diesel_tools/service/entity_service.rs`: the `EntityService`
//! default methods really delegate to the underlying repository (they add
//! logging on top, so this is mostly a wiring check - a service only has to
//! pick the `Error` type).

use crate::common::{self, WidgetRepository, widget};
use rust_toolbox::diesel_tools::{DatabaseError, EntityService, PageRequest};

struct WidgetService;

impl EntityService<WidgetRepository> for WidgetService {
    type Error = DatabaseError;
}

#[test]
fn entity_service_delegates_to_the_repository() {
    let Some(mut db) = common::setup() else {
        return;
    };
    let service = WidgetService;

    let saved = service.save(&mut db.conn, &widget(1, "alpha", 1)).unwrap();
    assert_eq!(saved, widget(1, "alpha", 1));
    service
        .save_all(&mut db.conn, &[widget(2, "beta", 2), widget(3, "gamma", 3)])
        .unwrap();

    assert_eq!(service.count(&mut db.conn).unwrap(), 3);
    assert!(service.exists_by_id(&mut db.conn, &1).unwrap());
    assert_eq!(
        service.exists_by_id_in(&mut db.conn, &[1, 42]).unwrap(),
        vec![(1, true), (42, false)]
    );
    assert_eq!(
        service.find_by_id(&mut db.conn, &1).unwrap(),
        Some(widget(1, "alpha", 1))
    );
    assert_eq!(
        service.find_by_id_in(&mut db.conn, &[1, 2]).unwrap().len(),
        2
    );
    assert_eq!(
        service
            .find_all(&mut db.conn, &PageRequest::default())
            .unwrap()
            .len(),
        3
    );

    assert_eq!(service.delete_by_id(&mut db.conn, &1).unwrap(), 1);
    assert_eq!(service.delete_by_id_in(&mut db.conn, &[2, 42]).unwrap(), 1);
    assert_eq!(service.clear(&mut db.conn).unwrap(), 1);
    assert_eq!(service.count(&mut db.conn).unwrap(), 0);
}
