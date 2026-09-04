#[test]
fn the_prelude_carries_the_names_a_handler_actually_uses() {
    use toolbox::prelude::*;

    let _ = ErrorKind::NotFound;
    let _: PageRequest = PageRequest::unpaged(Sort::unsorted());
    let _ = Problem::new(404, "Not Found");
    let _ = DbError::NotFound;
    let _ = ApiError::not_found("x");
    let _ = Principal::new("u", "local");
}
