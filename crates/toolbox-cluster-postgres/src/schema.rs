//! This crate's own tables.
#![allow(missing_docs)]

diesel::table! {
    toolbox_outbox (id) {
        id -> BigInt,
        topic -> Text,
        event -> Jsonb,
        created_at -> Timestamptz,
        published_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    toolbox_kv (key) {
        key -> Text,
        value -> Binary,
        expires_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    toolbox_locks (key) {
        key -> Text,
        owner -> Text,
        expires_at -> Timestamptz,
    }
}
