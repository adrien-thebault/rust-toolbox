use toolbox_files::{CHUNK_SIZE, proto::upload_request, upload_chunk, upload_info};

#[test]
fn the_first_message_carries_the_info_and_the_rest_carry_chunks() {
    let head = upload_info("report.pdf", "application/pdf");
    match head.payload.unwrap() {
        upload_request::Payload::Info(info) => {
            assert_eq!(info.filename, "report.pdf");
            assert_eq!(info.declared_mime, "application/pdf");
        }
        upload_request::Payload::Chunk(_) => panic!("the first message must be info"),
    }

    let body = upload_chunk(bytes::Bytes::from_static(b"data"));
    match body.payload.unwrap() {
        upload_request::Payload::Chunk(data) => assert_eq!(&data[..], b"data"),
        upload_request::Payload::Info(_) => panic!("a body message must be a chunk"),
    }
}

/// Large enough that per-message overhead is negligible, small enough that a
/// hundred concurrent uploads cost megabytes rather than gigabytes.
#[test]
fn the_chunk_size_is_bounded_and_sensible() {
    assert_eq!(CHUNK_SIZE, 64 * 1024);
}

/// `bytes` fields are `Bytes`, not `Vec<u8>`, so a chunk is not copied out of
/// the wire buffer on the way through.
#[test]
fn chunks_are_zero_copy_bytes() {
    let data = bytes::Bytes::from_static(b"abc");
    let message = upload_chunk(data.clone());
    let upload_request::Payload::Chunk(out) = message.payload.unwrap() else {
        panic!("a chunk");
    };
    let _: bytes::Bytes = out;
}
