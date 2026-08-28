//! The component's own table.
#![allow(missing_docs)]

diesel::table! {
    toolbox_files (key) {
        key -> Text,
        hash -> Text,
        filename -> Nullable<Text>,
        mime_type -> Text,
        size -> BigInt,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        deleted_at -> Nullable<Timestamp>,
    }
}
