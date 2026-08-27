//! Window-function pagination: one statement, one consistent count.
//!
//! The obvious implementation runs a `COUNT(*)` and then a `SELECT`, which can
//! disagree under a concurrent write and only works on queries the toolbox
//! generated. This composes onto *any* diesel query, so adding a `WHERE` clause
//! no longer costs you pagination.

use diesel::{
    QueryResult, RunQueryDsl,
    backend::Backend,
    query_builder::{AstPass, Query, QueryFragment, QueryId},
    sql_types::BigInt,
};
use toolbox_core::{Page, PageRequest};

use crate::error::DbResult;

/// The alias the window count is loaded under.
const TOTAL: &str = "__toolbox_total";

/// Attach a [`PageRequest`] to any diesel query.
pub trait Paginate: Sized {
    /// Wrap this query so it loads one page plus the total that matched.
    ///
    /// # Arguments
    ///
    /// * `request` - The window and ordering to apply. An unpaged request still
    ///   goes through, so a caller has one code path.
    fn paginate(self, request: &PageRequest) -> Paginated<Self>;
}

impl<T> Paginate for T {
    fn paginate(self, request: &PageRequest) -> Paginated<Self> {
        Paginated {
            query: self,
            request: request.clone(),
        }
    }
}

/// A query wrapped to return one page and the total row count.
///
/// The count comes from `COUNT(*) OVER ()` in the same statement, so it cannot
/// disagree with the rows returned - which a separate `COUNT` query can, and
/// does, under concurrent writes.
#[derive(Debug, Clone, QueryId)]
pub struct Paginated<Q> {
    query: Q,
    request: PageRequest,
}

impl<Q: Query> Query for Paginated<Q> {
    type SqlType = (Q::SqlType, BigInt);
}

impl<Q, C> RunQueryDsl<C> for Paginated<Q> {}

/// One `QueryFragment` impl for every backend, not one per backend.
///
/// This is what lets `toolbox-db` declare no `sqlite`/`postgres`/`mysql`
/// feature at all: the SQL is identical everywhere, so the consumer's own
/// diesel dependency picks the backend. Window functions require PostgreSQL,
/// SQLite 3.25+ or MySQL 8.0+; MySQL 5.7 is not supported, and a two-query
/// fallback is deliberately not offered because the SQL would have to differ
/// per backend, which is the property this impl exists to avoid.
impl<Q, DB> QueryFragment<DB> for Paginated<Q>
where
    DB: Backend,
    Q: QueryFragment<DB>,
    i64: diesel::serialize::ToSql<BigInt, DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("SELECT *, COUNT(*) OVER () AS ");
        out.push_sql(TOTAL);
        out.push_sql(" FROM (");
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(") AS __toolbox_page");
        if let PageRequest::Paged { offset, limit, .. } = &self.request {
            out.push_sql(" LIMIT ");
            out.push_bind_param::<BigInt, _>(limit)?;
            out.push_sql(" OFFSET ");
            out.push_bind_param::<BigInt, _>(offset)?;
        }
        Ok(())
    }
}

impl<Q> Paginated<Q> {
    /// Load the page and its total.
    ///
    /// # Arguments
    ///
    /// * `conn` - The connection to run the statement on.
    ///
    /// # Errors
    /// Any [`crate::DbError::Query`] the underlying statement produces.
    pub fn load_page<'query, U, C>(self, conn: &mut C) -> DbResult<Page<U>>
    where
        C: diesel::connection::LoadConnection,
        Self: diesel::query_dsl::methods::LoadQuery<'query, C, (U, i64)>,
    {
        let request = self.request.clone();
        let rows: Vec<(U, i64)> = self.load(conn)?;
        // Every row carries the same window count; an empty page means zero
        // matched, which is exactly what the window would have reported.
        let total = rows.first().map_or(0, |row| row.1);
        Ok(Page::new(
            rows.into_iter().map(|row| row.0).collect(),
            request,
            total,
        ))
    }
}
