//! The entities this domain owns.
//!
//! One file each, so a `TodoList` regrouping several todos is a new file rather
//! than a longer one, and the two can be read without reading each other.

pub mod todo;

pub use todo::Todo;
