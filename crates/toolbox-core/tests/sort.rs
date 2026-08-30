use toolbox_core::{PageError, Sort, SortDirection, SortItem};

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
