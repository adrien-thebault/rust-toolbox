use http::{StatusCode, header};
use toolbox_files::{ByteRange, Conditionals, FileError, FileMeta, parse_range, serve};

fn meta() -> FileMeta {
    FileMeta {
        key: "files/abc123".to_owned(),
        hash: "abc123".to_owned(),
        filename: Some("report.pdf".to_owned()),
        mime_type: "application/pdf".to_owned(),
        size: 1000,
    }
}

fn plain() -> Conditionals {
    Conditionals::default()
}

#[test]
fn a_whole_file_is_a_200_with_its_length() {
    let d = serve(&meta(), None, &plain()).unwrap();
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.headers[header::CONTENT_LENGTH], "1000");
    assert_eq!(d.headers[header::CONTENT_TYPE], "application/pdf");
    assert!(d.range.is_none());
    assert!(d.has_body());
}

/// The URL is the content's hash, so the bytes behind it can never change.
#[test]
fn a_content_addressed_file_is_cached_forever() {
    let d = serve(&meta(), None, &plain()).unwrap();
    let cache = d.headers[header::CACHE_CONTROL].to_str().unwrap();
    assert!(cache.contains("immutable"), "{cache}");
    assert!(cache.contains("max-age=31536000"), "{cache}");
    assert!(cache.contains("public"), "{cache}");
}

/// The ETag *is* the key, so conditional requests are correct for free.
#[test]
fn the_etag_is_the_content_hash() {
    let d = serve(&meta(), None, &plain()).unwrap();
    assert_eq!(d.headers[header::ETAG], "\"abc123\"");
}

#[test]
fn a_matching_etag_is_a_304_with_no_body() {
    let conditionals = Conditionals {
        if_none_match: Some("\"abc123\"".to_owned()),
    };
    let d = serve(&meta(), None, &conditionals).unwrap();
    assert_eq!(d.status, StatusCode::NOT_MODIFIED);
    assert!(!d.has_body());
    assert!(d.range.is_none());
}

#[test]
fn a_weak_etag_and_a_star_both_match() {
    for value in ["W/\"abc123\"", "*", "\"other\", \"abc123\""] {
        let conditionals = Conditionals {
            if_none_match: Some(value.to_owned()),
        };
        assert_eq!(
            serve(&meta(), None, &conditionals).unwrap().status,
            StatusCode::NOT_MODIFIED,
            "for {value}"
        );
    }
}

#[test]
fn a_non_matching_etag_serves_the_body() {
    let conditionals = Conditionals {
        if_none_match: Some("\"different\"".to_owned()),
    };
    assert_eq!(
        serve(&meta(), None, &conditionals).unwrap().status,
        StatusCode::OK
    );
}

/// A stored SVG or HTML file runs as your origin unless the sandbox says
/// otherwise, and a sniffing browser finds script wherever it is hidden.
#[test]
fn every_download_carries_the_headers_that_stop_a_stored_exploit() {
    let d = serve(&meta(), None, &plain()).unwrap();
    assert_eq!(d.headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(d.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
}

/// The filename came from whoever uploaded it, so it must not be able to
/// inject header syntax.
#[test]
fn a_hostile_filename_cannot_break_out_of_the_header() {
    let mut m = meta();
    m.filename = Some("a\"; attachment; x=\"b".to_owned());
    let d = serve(&m, None, &plain()).unwrap();
    let disposition = d.headers[header::CONTENT_DISPOSITION].to_str().unwrap();

    // The word "attachment" surviving inside the quoted value is harmless.
    // What must not survive is a delimiter that ends the quoted value and
    // starts a new directive.
    assert_eq!(
        disposition.matches('"').count(),
        2,
        "exactly one quoted value: {disposition}"
    );
    assert_eq!(
        disposition.matches(';').count(),
        1,
        "exactly one directive: {disposition}"
    );
    assert!(
        disposition.starts_with("inline; filename=\""),
        "{disposition}"
    );
    assert!(disposition.ends_with('"'), "{disposition}");
}

#[test]
fn a_control_character_in_a_filename_is_stripped() {
    let mut m = meta();
    m.filename = Some("a\r\nX-Evil: yes".to_owned());
    let d = serve(&m, None, &plain()).unwrap();
    let disposition = d.headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(
        !disposition.contains('\r') && !disposition.contains('\n'),
        "{disposition}"
    );
}

#[test]
fn a_filename_that_sanitizes_to_nothing_falls_back_to_inline() {
    let mut m = meta();
    m.filename = Some("\"\";;".to_owned());
    let d = serve(&m, None, &plain()).unwrap();
    assert_eq!(d.headers[header::CONTENT_DISPOSITION], "inline");
}

#[test]
fn a_range_request_is_a_206_with_a_content_range() {
    let d = serve(&meta(), Some(ByteRange { start: 0, end: 499 }), &plain()).unwrap();
    assert_eq!(d.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(d.headers[header::CONTENT_RANGE], "bytes 0-499/1000");
    assert_eq!(d.headers[header::CONTENT_LENGTH], "500");
    assert_eq!(d.range, Some(ByteRange { start: 0, end: 499 }));
}

/// Asking past the end is a normal thing a media player does; the answer is
/// the rest of the file, not an error.
#[test]
fn a_range_past_the_end_is_clamped_rather_than_refused() {
    let d = serve(
        &meta(),
        Some(ByteRange {
            start: 900,
            end: 99_999,
        }),
        &plain(),
    )
    .unwrap();
    assert_eq!(d.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(d.headers[header::CONTENT_RANGE], "bytes 900-999/1000");
    assert_eq!(d.range.unwrap().len(), 100);
}

#[test]
fn a_range_starting_past_the_end_is_not_satisfiable() {
    let err = serve(
        &meta(),
        Some(ByteRange {
            start: 2000,
            end: 3000,
        }),
        &plain(),
    )
    .unwrap_err();
    assert!(
        matches!(err, FileError::RangeNotSatisfiable { size: 1000 }),
        "{err:?}"
    );
}

#[test]
fn range_requests_are_advertised() {
    assert_eq!(
        serve(&meta(), None, &plain()).unwrap().headers[header::ACCEPT_RANGES],
        "bytes"
    );
}

#[test]
fn the_three_single_range_forms_parse() {
    assert_eq!(
        parse_range("bytes=0-499", 1000),
        Some(ByteRange { start: 0, end: 499 })
    );
    assert_eq!(
        parse_range("bytes=500-", 1000),
        Some(ByteRange {
            start: 500,
            end: 999
        })
    );
    assert_eq!(
        parse_range("bytes=-500", 1000),
        Some(ByteRange {
            start: 500,
            end: 999
        })
    );
}

/// Multi-range needs multipart/byteranges, which no common client asks for.
/// Returning None means the whole file is served, which is always correct.
#[test]
fn a_multi_range_request_falls_back_to_the_whole_file() {
    assert_eq!(parse_range("bytes=0-99,200-299", 1000), None);
}

#[test]
fn a_malformed_range_falls_back_to_the_whole_file() {
    for bad in ["", "items=0-1", "bytes=", "bytes=abc-def", "0-499"] {
        assert_eq!(parse_range(bad, 1000), None, "for `{bad}`");
    }
}

#[test]
fn a_suffix_longer_than_the_file_is_the_whole_file() {
    assert_eq!(
        parse_range("bytes=-5000", 1000),
        Some(ByteRange { start: 0, end: 999 })
    );
}

#[test]
fn a_range_covers_an_inclusive_span() {
    assert_eq!(ByteRange { start: 0, end: 0 }.len(), 1);
    assert_eq!(ByteRange { start: 10, end: 19 }.len(), 10);
}
