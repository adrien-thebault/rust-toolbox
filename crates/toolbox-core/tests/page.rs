use toolbox_core::{MAX_LIMIT, Page, PageError, PageRequest, Sort, SortDirection, SortItem};

#[test]
fn paged_rejects_a_negative_offset() {
    assert_eq!(
        PageRequest::paged(-1, 10, Sort::unsorted()),
        Err(PageError::NegativeOffset(-1))
    );
}

#[test]
fn paged_rejects_a_non_positive_limit() {
    assert_eq!(
        PageRequest::paged(0, 0, Sort::unsorted()),
        Err(PageError::NonPositiveLimit(0))
    );
    assert_eq!(
        PageRequest::paged(0, -5, Sort::unsorted()),
        Err(PageError::NonPositiveLimit(-5))
    );
}

#[test]
fn paged_rejects_a_limit_over_the_cap() {
    assert_eq!(
        PageRequest::paged(0, MAX_LIMIT + 1, Sort::unsorted()),
        Err(PageError::LimitTooLarge {
            requested: MAX_LIMIT + 1,
            max: MAX_LIMIT
        })
    );
    assert!(PageRequest::paged(0, MAX_LIMIT, Sort::unsorted()).is_ok());
}

#[test]
fn a_call_site_can_lower_the_cap() {
    assert!(PageRequest::paged_with_max(0, 51, Sort::unsorted(), 50).is_err());
    assert!(PageRequest::paged_with_max(0, 50, Sort::unsorted(), 50).is_ok());
}

#[test]
fn next_page_saturates_instead_of_wrapping() {
    let r = PageRequest::paged(i64::MAX - 1, 10, Sort::unsorted()).unwrap();
    assert_eq!(r.next_page().offset(), Some(i64::MAX));
}

#[test]
fn previous_page_saturates_at_zero() {
    let r = PageRequest::paged(5, 10, Sort::unsorted()).unwrap();
    assert_eq!(r.previous_page().offset(), Some(0));
}

#[test]
fn paging_an_unpaged_request_is_a_no_op() {
    let r = PageRequest::unpaged(Sort::unsorted());
    assert_eq!(r.next_page(), r);
    assert_eq!(r.previous_page(), r);
}

#[test]
fn sort_round_trips_through_the_compact_form() {
    for s in ["", "title", "-created_at", "-created_at,title", "a,-b,c"] {
        assert_eq!(
            Sort::parse(s).unwrap().to_string(),
            s,
            "round trip of `{s}`"
        );
    }
}

#[test]
fn sort_parses_directions_and_optional_plus() {
    let sort = Sort::parse("-created_at, +title ,id").unwrap();
    assert_eq!(sort.len(), 3);
    assert_eq!(sort.items()[0], SortItem::desc("created_at"));
    assert_eq!(sort.items()[1], SortItem::asc("title"));
    assert_eq!(sort.items()[2], SortItem::asc("id"));
}

#[test]
fn an_empty_sort_string_is_unsorted() {
    assert!(Sort::parse("").unwrap().is_empty());
    assert!(Sort::parse("   ").unwrap().is_empty());
}

#[test]
fn a_blank_sort_term_is_rejected() {
    assert_eq!(
        Sort::parse("title,").unwrap_err(),
        PageError::EmptySortField
    );
    assert_eq!(Sort::parse("-").unwrap_err(), PageError::EmptySortField);
}

#[test]
fn sort_direction_renders_as_sql() {
    assert_eq!(SortDirection::Asc.as_sql(), "ASC");
    assert_eq!(SortDirection::Desc.as_sql(), "DESC");
}

#[test]
fn page_number_and_total_pages_are_derived_from_the_request() {
    let req = PageRequest::paged(20, 10, Sort::unsorted()).unwrap();
    let page = Page::new(vec![1, 2, 3], req, 47);
    assert_eq!(page.page_number(), Some(2));
    assert_eq!(page.total_pages(), Some(5));
    assert_eq!(page.len(), 3);
    assert_eq!(page.total(), 47);
    assert!(page.has_next());
    assert!(page.has_previous());
}

#[test]
fn total_pages_rounds_up_and_does_not_overflow() {
    let req = PageRequest::paged(0, 10, Sort::unsorted()).unwrap();
    assert_eq!(
        Page::<u8>::new(vec![], req.clone(), 0).total_pages(),
        Some(0)
    );
    assert_eq!(
        Page::<u8>::new(vec![], req.clone(), 1).total_pages(),
        Some(1)
    );
    assert_eq!(
        Page::<u8>::new(vec![], req.clone(), 10).total_pages(),
        Some(1)
    );
    assert_eq!(
        Page::<u8>::new(vec![], req.clone(), 11).total_pages(),
        Some(2)
    );
    assert_eq!(
        Page::<u8>::new(vec![], req, i64::MAX).total_pages(),
        Some(i64::MAX / 10)
    );
}

#[test]
fn the_last_page_has_no_next() {
    let req = PageRequest::paged(40, 10, Sort::unsorted()).unwrap();
    let page = Page::new(vec![1], req, 47);
    assert!(!page.has_next());
    assert!(page.has_previous());
}

#[test]
fn an_unpaged_page_has_no_page_numbers() {
    let page = Page::unpaged(vec![1, 2, 3], Sort::unsorted());
    assert_eq!(page.page_number(), None);
    assert_eq!(page.total_pages(), None);
    assert_eq!(page.total(), 3);
    assert!(!page.has_next());
    assert!(!page.has_previous());
}

#[test]
fn map_keeps_the_metadata() {
    let req = PageRequest::paged(10, 5, Sort::parse("-id").unwrap()).unwrap();
    let page = Page::new(vec![1, 2], req.clone(), 99).map(|n| n.to_string());
    assert_eq!(page.items(), ["1", "2"]);
    assert_eq!(page.total(), 99);
    assert_eq!(page.request(), &req);
}

#[test]
fn try_map_keeps_the_metadata_and_short_circuits() {
    let req = PageRequest::paged(0, 5, Sort::unsorted()).unwrap();
    let ok: Page<i64> = Page::new(vec![1_i32, 2], req.clone(), 2)
        .try_map(|n| Ok::<_, ()>(i64::from(n)))
        .unwrap();
    assert_eq!(ok.items(), [1, 2]);
    assert_eq!(ok.total(), 2);

    let err: Result<Page<i64>, &str> = Page::new(vec![1_i32, 2], req, 2).try_map(|n| {
        if n == 2 {
            Err("nope")
        } else {
            Ok(i64::from(n))
        }
    });
    assert_eq!(err.unwrap_err(), "nope");
}

#[test]
fn an_empty_page_is_empty() {
    let page = Page::<u8>::empty(PageRequest::default());
    assert!(page.is_empty());
    assert_eq!(page.total(), 0);
}
