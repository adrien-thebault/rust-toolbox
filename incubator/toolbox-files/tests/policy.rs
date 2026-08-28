use toolbox_files::{MimePolicy, UploadPolicy};

#[test]
fn any_permits_everything() {
    assert!(MimePolicy::Any.permits("application/octet-stream"));
    assert!(MimePolicy::Any.permits("text/html"));
}

#[test]
fn an_allowlist_permits_exactly_what_it_lists() {
    let policy = MimePolicy::Allowlist(&["application/pdf"]);
    assert!(policy.permits("application/pdf"));
    assert!(!policy.permits("text/html"));
    assert_eq!(policy.allowed(), ["application/pdf"]);
}

#[test]
fn images_only_permits_what_a_browser_renders() {
    let policy = MimePolicy::ImagesOnly;
    assert!(policy.permits("image/png"));
    assert!(policy.permits("image/webp"));
    assert!(
        !policy.permits("image/svg+xml"),
        "SVG is script, not an image"
    );
    assert!(!policy.permits("application/pdf"));
}

#[test]
fn the_default_cap_is_present_rather_than_unlimited() {
    assert_eq!(UploadPolicy::default().max_bytes, 10 * 1024 * 1024);
}
