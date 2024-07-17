use std::sync::Arc;

use rockbound::cache::cache_db::CacheDb;
use rockbound::SchemaBatch;

use crate::schema::tables::{ModuleAccessoryState, ACCESSORY_TABLES};
use crate::schema::types::AccessoryKey;
use crate::DbOptions;

/// Specifies a particular version of the Accessory state.
pub type Version = u64;

/// Typesafe transformer for data, that is not part of the provable state.
#[derive(Clone, Debug)]
pub struct AccessoryDb {
    /// Pointer to [`CacheDb`] for up to date state
    db: Arc<CacheDb>,
}

impl AccessoryDb {
    const DB_PATH_SUFFIX: &'static str = "accessory";
    const DB_NAME: &'static str = "accessory-db";

    /// Get [`DbOptions`] for [`AccessoryDb`]
    pub fn get_rockbound_options() -> DbOptions {
        DbOptions {
            name: Self::DB_NAME,
            path_suffix: Self::DB_PATH_SUFFIX,
            columns: ACCESSORY_TABLES.to_vec(),
        }
    }

    /// Create instance of [`AccessoryDb`] from [`CacheDb`]
    pub fn with_cache_db(db: CacheDb) -> anyhow::Result<Self> {
        // We keep Result type, just for future archival state integration
        Ok(Self { db: Arc::new(db) })
    }

    /// Queries for a value in the [`AccessoryDb`], given a key.
    pub fn get_value_option(
        &self,
        key: &AccessoryKey,
        version: Version,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let found = self
            .db
            .get_prev::<ModuleAccessoryState>(&(key.to_vec(), version))?;
        match found {
            Some(((found_key, found_version), value)) => {
                if &found_key == key {
                    anyhow::ensure!(found_version <= version, "Bug! iterator isn't returning expected values. expected a version <= {version:} but found {found_version:}");
                    Ok(value)
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Collects a sequence of key-value pairs into [`SchemaBatch`].
    pub fn materialize_values(
        &self,
        key_value_pairs: impl IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>,
        version: Version,
    ) -> anyhow::Result<SchemaBatch> {
        let mut batch = SchemaBatch::default();
        for (key, value) in key_value_pairs {
            batch.put::<ModuleAccessoryState>(&(key, version), &value)?;
        }
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{commit_changes_through, setup_cache_db_with_container};

    #[test]
    fn get_after_set() {
        let tempdir = tempfile::tempdir().unwrap();
        let (cache_db, cache_container) =
            setup_cache_db_with_container(tempdir.path(), AccessoryDb::get_rockbound_options());
        let db = AccessoryDb::with_cache_db(cache_db).unwrap();

        let key = b"foo".to_vec();
        let value = b"bar".to_vec();
        let changes1 = db
            .materialize_values(vec![(key.clone(), Some(value.clone()))], 0)
            .unwrap();
        commit_changes_through(&cache_container, changes1);
        assert_eq!(db.get_value_option(&key, 0).unwrap(), Some(value.clone()));

        let value2 = b"baz".to_vec();
        let changes2 = db
            .materialize_values(vec![(key.clone(), Some(value2.clone()))], 1)
            .unwrap();
        commit_changes_through(&cache_container, changes2);
        assert_eq!(db.get_value_option(&key, 0).unwrap(), Some(value));
    }

    #[test]
    fn get_after_delete() {
        let tempdir = tempfile::tempdir().unwrap();
        let (cache_db, cache_container) =
            setup_cache_db_with_container(tempdir.path(), AccessoryDb::get_rockbound_options());
        let db = AccessoryDb::with_cache_db(cache_db).unwrap();

        let key = b"deleted".to_vec();
        let value = b"baz".to_vec();
        let changes1 = db
            .materialize_values(vec![(key.clone(), Some(value.clone()))], 0)
            .unwrap();
        commit_changes_through(&cache_container, changes1);
        assert_eq!(db.get_value_option(&key, 0).unwrap(), Some(value.clone()));

        let changes2 = db.materialize_values(vec![(key.clone(), None)], 0).unwrap();
        commit_changes_through(&cache_container, changes2);
        assert_eq!(db.get_value_option(&key, 0).unwrap(), None);
    }

    #[test]
    fn get_nonexistent() {
        let tempdir = tempfile::tempdir().unwrap();
        let (cache_db, _cache_container) =
            setup_cache_db_with_container(tempdir.path(), AccessoryDb::get_rockbound_options());
        let db = AccessoryDb::with_cache_db(cache_db).unwrap();

        let key = b"spam".to_vec();
        assert_eq!(db.get_value_option(&key, 0).unwrap(), None);
    }
}
