//! What may be uploaded.

use serde::{Deserialize, Serialize};

/// Which media types an upload route accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MimePolicy {
    /// Anything. Only for a route whose callers are already trusted.
    Any,
    /// Exactly these types, matched against the **sniffed** type, never the
    /// declared one.
    Allowlist(&'static [&'static str]),
    /// The image types the browser can display.
    ImagesOnly,
}

/// The image types every current browser renders.
const IMAGE_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
];

impl MimePolicy {
    /// Whether a sniffed media type is permitted.
    ///
    /// # Arguments
    ///
    /// * `mime` - The sniffed media type, never the declared one.
    #[must_use]
    pub fn permits(&self, mime: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Allowlist(allowed) => allowed.contains(&mime),
            Self::ImagesOnly => IMAGE_TYPES.contains(&mime),
        }
    }

    /// The permitted types, for an error message that says what would work.
    #[must_use]
    pub fn allowed(&self) -> &'static [&'static str] {
        match self {
            Self::Any => &[],
            Self::Allowlist(allowed) => allowed,
            Self::ImagesOnly => IMAGE_TYPES,
        }
    }
}

/// A per-owner storage cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quota {
    /// The most bytes one owner may store.
    pub max_total_bytes: u64,
    /// The most files one owner may store.
    pub max_files: u64,
}

/// What an upload route accepts.
#[derive(Debug, Clone)]
pub struct UploadPolicy {
    /// The largest single upload.
    ///
    /// Enforced **as bytes flow**, not after buffering: a cap checked after
    /// reading the body is a cap that has already cost you the memory.
    pub max_bytes: u64,
    /// Which media types are permitted.
    pub allowed: MimePolicy,
    /// A per-owner cap, if the caller enforces one.
    pub quota: Option<Quota>,
}

impl Default for UploadPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,
            allowed: MimePolicy::Any,
            quota: None,
        }
    }
}

impl UploadPolicy {
    /// A policy accepting only images, up to `max_bytes`.
    ///
    /// # Arguments
    ///
    /// * `max_bytes` - The size cap. The allowlist is the image types every
    ///   current browser renders.
    #[must_use]
    pub fn images(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            allowed: MimePolicy::ImagesOnly,
            quota: None,
        }
    }

    /// Set the size cap.
    ///
    /// # Arguments
    ///
    /// * `max` - The largest upload accepted, checked as the bytes flow rather
    ///   than after they land.
    #[must_use]
    pub fn max_bytes(mut self, max: u64) -> Self {
        self.max_bytes = max;
        self
    }

    /// Set the media-type policy.
    ///
    /// # Arguments
    ///
    /// * `allowed` - Which media types this route accepts. A route-level
    ///   allowlist, because an avatar and an attachment do not accept the same
    ///   things.
    #[must_use]
    pub fn allowed(mut self, allowed: MimePolicy) -> Self {
        self.allowed = allowed;
        self
    }

    /// Set the per-owner quota.
    ///
    /// # Arguments
    ///
    /// * `quota` - The per-owner storage cap, so one curious user cannot fill
    ///   the disk.
    #[must_use]
    pub fn quota(mut self, quota: Quota) -> Self {
        self.quota = Some(quota);
        self
    }
}
