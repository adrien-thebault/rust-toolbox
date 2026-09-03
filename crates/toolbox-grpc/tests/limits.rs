use toolbox_grpc::MessageLimits;

/// tonic defaults to 4 MiB decoding and *unlimited* encoding, which is the
/// asymmetry that produces "it works from the gateway but not the backend".
/// `MessageLimits` makes both ends symmetric.
#[test]
fn the_default_is_symmetric_unlike_tonics() {
    let limits = MessageLimits::default();
    assert_eq!(limits.max_decoding, 4 * 1024 * 1024);
    assert_eq!(limits.max_encoding, limits.max_decoding);
}

#[test]
fn the_two_ends_read_one_value() {
    let limits = MessageLimits {
        max_decoding: 16 * 1024 * 1024,
        max_encoding: 1024,
    };
    // The point of the type: a client and a server built from the same value
    // cannot drift.
    let client = limits;
    let server = limits;
    assert_eq!(client.max_decoding, server.max_decoding);
    assert_eq!(client.max_encoding, server.max_encoding);
}
