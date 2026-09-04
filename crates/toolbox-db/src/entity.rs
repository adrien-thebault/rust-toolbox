//! The trait `#[derive(Entity)]` implements.

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
