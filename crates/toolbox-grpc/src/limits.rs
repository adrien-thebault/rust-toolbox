//! Message size limits, the one value both ends of a connection read.

/// The largest messages a client or server will encode and decode.
///
/// Neither end can apply it for you - tonic puts `max_decoding_message_size` on
/// the generated client and server types, with no trait to reach it through -
/// so a caller passes it to both from the same `MessageLimits`. That turns a
/// silent drift into a visible one: the two ends differ only if somebody passes
/// different values, not by forgetting one.
#[derive(Debug, Clone, Copy)]
pub struct MessageLimits {
    /// The largest message this end will decode.
    pub max_decoding: usize,
    /// The largest message this end will encode.
    pub max_encoding: usize,
}

impl Default for MessageLimits {
    fn default() -> Self {
        // tonic's own default is 4 MiB for decoding and unlimited for
        // encoding, which is the asymmetry that produces "it works from the
        // gateway but not from the backend".
        Self {
            max_decoding: 4 * 1024 * 1024,
            max_encoding: 4 * 1024 * 1024,
        }
    }
}
