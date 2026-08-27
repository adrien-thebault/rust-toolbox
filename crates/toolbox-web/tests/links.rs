use toolbox_core::{Page, PageRequest, Sort};
use toolbox_web::links::page_links;

fn page(offset: i64, limit: i64, total: i64) -> Page<u8> {
    Page::new(
        vec![],
        PageRequest::paged(offset, limit, Sort::unsorted()).unwrap(),
        total,
    )
}

fn links(offset: i64, limit: i64, total: i64) -> String {
    page_links(&page(offset, limit, total), "/api/todos")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_first_page_offers_next_and_last_only() {
    let value = links(0, 10, 47);
    assert!(value.contains("rel=\"next\""), "{value}");
    assert!(value.contains("rel=\"last\""), "{value}");
    assert!(!value.contains("rel=\"prev\""), "{value}");
    assert!(!value.contains("rel=\"first\""), "{value}");
}

#[test]
fn a_middle_page_offers_all_four() {
    let value = links(20, 10, 47);
    for rel in ["first", "prev", "next", "last"] {
        assert!(
            value.contains(&format!("rel=\"{rel}\"")),
            "missing {rel}: {value}"
        );
    }
    assert!(
        value.contains("offset=10"),
        "prev is one page back: {value}"
    );
    assert!(value.contains("offset=30"), "next is one page on: {value}");
}

#[test]
fn the_last_page_offers_no_next() {
    let value = links(40, 10, 47);
    assert!(!value.contains("rel=\"next\""), "{value}");
    assert!(value.contains("rel=\"prev\""), "{value}");
}

#[test]
fn the_last_link_points_at_the_final_page_not_past_the_end() {
    // 47 rows at 10 per page: the last page starts at 40, not at 47 or 50.
    let ragged = links(0, 10, 47);
    assert!(
        ragged.contains("offset=40&limit=10>; rel=\"last\""),
        "{ragged}"
    );

    // An exact multiple must not point one page past the end.
    let exact = links(0, 10, 40);
    assert!(
        exact.contains("offset=30&limit=10>; rel=\"last\""),
        "{exact}"
    );
}

#[test]
fn a_sort_is_preserved_in_every_link() {
    let request = PageRequest::paged(0, 10, Sort::parse("-created_at").unwrap()).unwrap();
    let value = page_links(&Page::<u8>::new(vec![], request, 47), "/api/todos")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(value.contains("sort=-created_at"), "{value}");
}

#[test]
fn an_unpaged_result_has_no_navigation() {
    let page = Page::<u8>::unpaged(vec![], Sort::unsorted());
    assert!(page_links(&page, "/api/todos").is_none());
}

#[test]
fn a_single_page_of_results_still_offers_last() {
    let value = links(0, 10, 3);
    assert!(value.contains("rel=\"last\""), "{value}");
    assert!(!value.contains("rel=\"next\""), "{value}");
}

#[test]
fn an_empty_result_has_no_links() {
    assert!(page_links(&page(0, 10, 0), "/api/todos").is_none());
}

#[test]
fn the_header_is_rfc_8288_shaped() {
    let value = links(20, 10, 47);
    for part in value.split(", ") {
        assert!(
            part.starts_with('<'),
            "each link is angle-bracketed: {part}"
        );
        assert!(part.contains(">; rel=\""), "with a rel parameter: {part}");
    }
}
