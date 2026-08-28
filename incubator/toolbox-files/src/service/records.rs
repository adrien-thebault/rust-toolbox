//! Where file identity is stored.
//!
//! It holds state across requests, so it is a trait with adapters.
//!
//! # Why this is a trait rather than an entity
//!
//! Every other table in this workspace uses `#[derive(Entity)]`, which needs a
//! concrete backend token - `#[entity(backend = crate::Backend)]`. A component
//! *shipped as a library* cannot have one: the consumer picks the backend, and
//! a query generic over an arbitrary
//! `C::Backend` is not writable, because diesel's
//! `DieselReserveSpecialization` is sealed.
//!
//! So the component owns the schema, the migrations and the service logic, and
//! the consumer owns the four queries - generated for their backend by
//! [`diesel_file_records!`](crate::diesel_file_records). That macro is invoked
//! in the consumer's crate, where the backend *is* known.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

use crate::FileMeta;

/// Why a record operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecordError {
    /// The store failed.
    #[error("file records: {0}")]
    Backend(String),
}

impl ServiceError for RecordError {
    fn code(&self) -> &'static str {
        "FILE_RECORDS_FAILED"
    }
    fn domain(&self) -> &'static str {
        "files"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Internal
    }
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

/// Storing and looking up file identity.
#[tonic::async_trait]
pub trait FileRecords: Send + Sync + 'static {
    /// Record a file, or refresh the record if the key is already known.
    ///
    /// Called after the bytes are safely stored, so an interrupted upload
    /// leaves no record.
    ///
    /// # Arguments
    ///
    /// * `meta` - The file to record. Called only after the bytes are safely
    ///   stored, so an interrupted upload leaves no record.
    ///
    /// # Errors
    /// [`RecordError::Backend`] when the store fails.
    async fn record(&self, meta: &FileMeta) -> Result<(), RecordError>;

    /// Look one up.
    ///
    /// # Arguments
    ///
    /// * `key` - The content-addressed key to look up. A miss is `Ok(None)`.
    ///
    /// # Errors
    /// [`RecordError::Backend`] when the store fails.
    async fn get(&self, key: &str) -> Result<Option<FileMeta>, RecordError>;

    /// Mark one deleted. Returns whether anything changed.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to mark deleted. The bytes are not removed, because
    ///   another record may share the same content.
    ///
    /// # Errors
    /// [`RecordError::Backend`] when the store fails.
    async fn delete(&self, key: &str) -> Result<bool, RecordError>;
}

/// Generate a diesel-backed [`FileRecords`] for your backend.
///
/// Invoked in **your** crate, because that is where the backend is known:
///
/// ```ignore
/// toolbox_file_service::diesel_file_records!(
///     FileStore, crate::Backend, crate::Connection
/// );
/// // then: FileStore::new(db.clone())
/// ```
#[macro_export]
macro_rules! diesel_file_records {
    ($name:ident, $backend:ty, $conn:ty) => {
        /// A diesel-backed file record store.
        #[derive(Clone)]
        pub struct $name {
            db: ::toolbox_db::Db<$conn>,
        }

        impl $name {
            /// Build it over a pool.
            ///
            /// # Arguments
            ///
            /// * `db` - The pool holding the file table this macro generated
            ///   the queries for.
            #[must_use]
            pub fn new(db: ::toolbox_db::Db<$conn>) -> Self {
                Self { db }
            }
        }

        #[::tonic::async_trait]
        impl $crate::service::records::FileRecords for $name {
            async fn record(
                &self,
                meta: &$crate::FileMeta,
            ) -> ::core::result::Result<(), $crate::service::records::RecordError> {
                use ::diesel::prelude::*;
                use $crate::service::schema::toolbox_files as t;

                let key = meta.key.clone();
                let hash = meta.hash.clone();
                let filename = meta.filename.clone();
                let mime = meta.mime_type.clone();
                let size = i64::try_from(meta.size).unwrap_or(i64::MAX);
                let now = ::chrono::Utc::now().naive_utc();

                self.db
                    .query(move |c: &mut $conn| {
                        // Content-addressed, so an existing row describes the
                        // same bytes: re-recording only clears a soft delete.
                        let existing: i64 =
                            t::table.filter(t::key.eq(&key)).count().get_result(c)?;
                        if existing > 0 {
                            ::diesel::update(t::table.filter(t::key.eq(&key)))
                                .set((
                                    t::deleted_at.eq(None::<::chrono::NaiveDateTime>),
                                    t::updated_at.eq(now),
                                ))
                                .execute(c)?;
                        } else {
                            ::diesel::insert_into(t::table)
                                .values((
                                    t::key.eq(&key),
                                    t::hash.eq(&hash),
                                    t::filename.eq(&filename),
                                    t::mime_type.eq(&mime),
                                    t::size.eq(size),
                                    t::created_at.eq(now),
                                    t::updated_at.eq(now),
                                ))
                                .execute(c)?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| $crate::service::records::RecordError::Backend(e.to_string()))
            }

            async fn get(
                &self,
                key: &str,
            ) -> ::core::result::Result<
                ::core::option::Option<$crate::FileMeta>,
                $crate::service::records::RecordError,
            > {
                use ::diesel::prelude::*;
                use $crate::service::schema::toolbox_files as t;

                let key = key.to_owned();
                let row = self
                    .db
                    .query(move |c: &mut $conn| {
                        t::table
                            .filter(t::key.eq(&key))
                            .filter(t::deleted_at.is_null())
                            .select((t::key, t::hash, t::filename, t::mime_type, t::size))
                            .first::<(
                                ::std::string::String,
                                ::std::string::String,
                                ::core::option::Option<::std::string::String>,
                                ::std::string::String,
                                i64,
                            )>(c)
                            .optional()
                    })
                    .await
                    .map_err(|e| $crate::service::records::RecordError::Backend(e.to_string()))?;

                Ok(
                    row.map(|(key, hash, filename, mime_type, size)| $crate::FileMeta {
                        key,
                        hash,
                        filename,
                        mime_type,
                        size: u64::try_from(size).unwrap_or(0),
                    }),
                )
            }

            async fn delete(
                &self,
                key: &str,
            ) -> ::core::result::Result<bool, $crate::service::records::RecordError> {
                use ::diesel::prelude::*;
                use $crate::service::schema::toolbox_files as t;

                let key = key.to_owned();
                let now = ::chrono::Utc::now().naive_utc();
                let changed = self
                    .db
                    .query(move |c: &mut $conn| {
                        ::diesel::update(
                            t::table
                                .filter(t::key.eq(&key))
                                .filter(t::deleted_at.is_null()),
                        )
                        .set(t::deleted_at.eq(::core::option::Option::Some(now)))
                        .execute(c)
                    })
                    .await
                    .map_err(|e| $crate::service::records::RecordError::Backend(e.to_string()))?;
                Ok(changed > 0)
            }
        }
    };
}
