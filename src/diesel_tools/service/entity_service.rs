use crate::diesel_tools::{
    database::{DatabaseError, DatabasePooledConnection},
    pagination::{Page, PageRequest},
    repository::{Delete, Find, Repository, Save},
};
use std::fmt;
use tracing::{debug, info, instrument, warn};

/// higher-level, logged operations over a Repository
pub trait EntityService<Repo>
where
    Repo: Repository,
    Repo::Id: fmt::Debug,
{
    /// the error type returned by every method below; must be constructible
    /// from a raw [`DatabaseError`] so `?` works inside their default bodies
    type Error: From<DatabaseError>;

    /// counts the number of entities
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn count(&self, db: &mut DatabasePooledConnection) -> Result<i64, Self::Error>
    where
        Repo: Find,
    {
        Ok(Repo::count(db)?)
    }

    /// checks the existence of an entity using its id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn exists_by_id(
        &self,
        db: &mut DatabasePooledConnection,
        id: &Repo::Id,
    ) -> Result<bool, Self::Error>
    where
        Repo: Find,
    {
        debug!(?id);
        Ok(Repo::exists_by_id(db, id)?)
    }

    /// checks the existence of entities using their id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn exists_by_id_in(
        &self,
        db: &mut DatabasePooledConnection,
        ids: &[Repo::Id],
    ) -> Result<Vec<(Repo::Id, bool)>, Self::Error>
    where
        Repo: Find,
    {
        debug!(?ids);
        Ok(Repo::exists_by_id_in(db, ids)?)
    }

    /// retrieves one entity using its id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn find_by_id(
        &self,
        db: &mut DatabasePooledConnection,
        id: &Repo::Id,
    ) -> Result<Option<Repo::Entity>, Self::Error>
    where
        Repo: Find,
    {
        debug!(?id);

        let found = Repo::find_by_id(db, id)?;
        if found.is_none() {
            // a normal outcome (the gateway maps it to a routine 404), not
            // something worth an operator's attention
            debug!("could not find entity");
        } else {
            info!("found entity");
        }
        Ok(found)
    }

    /// retrieves multiple entities using their id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn find_by_id_in(
        &self,
        db: &mut DatabasePooledConnection,
        ids: &[Repo::Id],
    ) -> Result<Vec<Repo::Entity>, Self::Error>
    where
        Repo: Find,
    {
        debug!(?ids);

        let found = Repo::find_by_id_in(db, ids)?;
        if found.len() != ids.len() {
            warn!(
                "found {} entities out of {} requested",
                found.len(),
                ids.len()
            );
        } else {
            info!("found {} entities", found.len());
        }
        Ok(found)
    }

    /// lists all the entities, paginated
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn find_all(
        &self,
        db: &mut DatabasePooledConnection,
        page_request: PageRequest,
    ) -> Result<Page<Repo::Entity>, Self::Error>
    where
        Repo: Find,
    {
        debug!(?page_request);

        let found = Repo::find_all(db, page_request)?;
        info!("found {} entities", found.len());
        Ok(found)
    }

    /// persists one entity: creates it if its id is absent, updates it
    /// otherwise (see [`Save`])
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn save(
        &self,
        db: &mut DatabasePooledConnection,
        entity: &Repo::Entity,
    ) -> Result<Repo::Entity, Self::Error>
    where
        Repo: Save,
    {
        let saved = Repo::save(db, entity)?;
        info!("saved 1 entity");
        Ok(saved)
    }

    /// persists multiple entities
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn save_all(
        &self,
        db: &mut DatabasePooledConnection,
        entities: &[Repo::Entity],
    ) -> Result<Vec<Repo::Entity>, Self::Error>
    where
        Repo: Save,
    {
        let saved = Repo::save_all(db, entities)?;
        info!("saved {} entities", saved.len());
        Ok(saved)
    }

    /// deletes everything
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn clear(&self, db: &mut DatabasePooledConnection) -> Result<usize, Self::Error>
    where
        Repo: Delete,
    {
        let deleted = Repo::clear(db)?;
        info!("deleted {} entities", deleted);
        Ok(deleted)
    }

    /// deletes one entity by its id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn delete_by_id(
        &self,
        db: &mut DatabasePooledConnection,
        id: &Repo::Id,
    ) -> Result<usize, Self::Error>
    where
        Repo: Delete,
    {
        debug!(?id);

        let deleted = Repo::delete_by_id(db, id)?;
        if deleted != 1 {
            warn!("deleted {} entities out of 1 deletion requested", deleted);
        } else {
            info!("deleted {} entities", deleted);
        }
        Ok(deleted)
    }

    /// deletes multiple entities using their id
    #[instrument(skip_all, fields(entity = %std::any::type_name::<Repo::Entity>()))]
    fn delete_by_id_in(
        &self,
        db: &mut DatabasePooledConnection,
        ids: &[Repo::Id],
    ) -> Result<usize, Self::Error>
    where
        Repo: Delete,
    {
        debug!(?ids);

        let deleted = Repo::delete_by_id_in(db, ids)?;
        if deleted != ids.len() {
            warn!(
                "deleted {} entities out of {} deletions requested",
                deleted,
                ids.len()
            );
        } else {
            info!("deleted {} entities", deleted);
        }
        Ok(deleted)
    }
}
