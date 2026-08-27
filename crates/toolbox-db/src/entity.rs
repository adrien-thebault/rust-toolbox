//! The traits `#[derive(Entity)]` implements and relies on.

/// A row type with an identity.
///
/// Implemented by `#[derive(Entity)]`. The generated inherent methods are the
/// API; this trait exists so generic code - the admin service, a component -
/// can name the id and table types.
pub trait Entity {
    /// The primary key type.
    type Id;
    /// The diesel table this maps to.
    type Table;

    /// The row's id, or `None` when it has not been inserted yet.
    fn id(&self) -> Option<&Self::Id>;
}

/// A timestamp type that can name the current instant.
///
/// This is how the derive populates `created_at` and `updated_at` without
/// naming a datetime crate: the entity's own field type decides which
/// clock is read. Adding `jiff` later is one more impl behind a feature, not a
/// macro change.
pub trait Now {
    /// The current instant.
    fn now() -> Self;
}

impl Now for std::time::SystemTime {
    fn now() -> Self {
        Self::now()
    }
}

#[cfg(feature = "chrono")]
impl Now for chrono::NaiveDateTime {
    fn now() -> Self {
        chrono::Utc::now().naive_utc()
    }
}

#[cfg(feature = "chrono")]
impl Now for chrono::DateTime<chrono::Utc> {
    fn now() -> Self {
        chrono::Utc::now()
    }
}

#[cfg(feature = "time")]
impl Now for time::OffsetDateTime {
    fn now() -> Self {
        Self::now_utc()
    }
}

#[cfg(feature = "time")]
impl Now for time::PrimitiveDateTime {
    fn now() -> Self {
        let now = time::OffsetDateTime::now_utc();
        Self::new(now.date(), now.time())
    }
}
