use toolbox_core::{Page, PageRequest, Sort};
use toolbox_grpc::{PROTO_INCLUDE, PageInfo, PageRequestProto, split};

#[test]
fn a_page_request_round_trips_through_the_wire_type() {
    let request = PageRequest::paged(20, 10, Sort::parse("-created_at,title").unwrap()).unwrap();
    let wire = PageRequestProto::from(&request);

    assert_eq!(wire.offset, 20);
    assert_eq!(wire.limit, 10);
    assert_eq!(wire.sort, "-created_at,title");
    assert_eq!(wire.to_domain().unwrap(), request);
}

/// Proto3's default for an unset int is zero, so "unpaged" and "the field was
/// not sent" have to agree or every client that omits it gets one row.
#[test]
fn a_zero_limit_on_the_wire_means_unpaged_not_an_error() {
    let wire = PageRequestProto {
        offset: 0,
        limit: 0,
        sort: String::new(),
    };
    assert!(matches!(
        wire.to_domain().unwrap(),
        PageRequest::Unpaged { .. }
    ));
}

#[test]
fn an_unpaged_request_serializes_as_a_zero_limit() {
    let wire = PageRequestProto::from(&PageRequest::unpaged(Sort::parse("id").unwrap()));
    assert_eq!(wire.limit, 0);
    assert_eq!(wire.sort, "id");
}

#[test]
fn an_invalid_wire_request_is_rejected_rather_than_clamped() {
    let wire = PageRequestProto {
        offset: -5,
        limit: 10,
        sort: String::new(),
    };
    assert!(wire.to_domain().is_err());

    let over = PageRequestProto {
        offset: 0,
        limit: 1_000_000,
        sort: String::new(),
    };
    assert!(over.to_domain().is_err());
}

#[test]
fn a_malformed_sort_on_the_wire_is_rejected() {
    let wire = PageRequestProto {
        offset: 0,
        limit: 10,
        sort: "title,".to_owned(),
    };
    assert!(wire.to_domain().is_err());
}

#[test]
fn page_info_describes_the_page_it_came_from() {
    let request = PageRequest::paged(20, 10, Sort::parse("-id").unwrap()).unwrap();
    let page = Page::new(vec![1, 2, 3], request.clone(), 47);
    let info = PageInfo::from(&page);

    assert_eq!(info.offset, 20);
    assert_eq!(info.limit, 10);
    assert_eq!(info.total, 47);
    assert_eq!(info.sort, "-id");
    assert_eq!(info.to_request().unwrap(), request);
}

/// This plus `Page::try_map` is the whole ~90-line conversion block that was
/// byte-identical in every consumer.
#[test]
fn split_gives_the_two_halves_a_list_response_carries() {
    let request = PageRequest::paged(0, 2, Sort::unsorted()).unwrap();
    let (items, info) = split(Page::new(vec!["a", "b"], request, 9));

    assert_eq!(items, ["a", "b"]);
    assert_eq!(info.total, 9);
    assert_eq!(info.limit, 2);
}

#[test]
fn the_proto_include_path_points_at_a_real_directory() {
    let path = std::path::Path::new(PROTO_INCLUDE);
    assert!(
        path.is_dir(),
        "{PROTO_INCLUDE} should exist for a consumer's build script"
    );
    assert!(path.join("toolbox/v1/pagination.proto").is_file());
}
