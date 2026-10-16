// SPDX-License-Identifier: MIT OR Apache-2.0

//! An in-memory database to store addresses data. Being in-memory means this database is
//! volatile, and all data is lost after the database is dropped or the process is terminated.
//! It's not meant to use in production, but for the integrated testing framework
//!
//! For actual databases that can be used for production code, see [KvDatabase](crate::kv_database::KvDatabase).
use core::fmt;
use core::fmt::Display;
use core::fmt::Formatter;

use bitcoin::Txid;
use bitcoin::hashes::sha256;
use floresta_common::prelude::sync::RwLock;
use floresta_common::prelude::*;
use floresta_domain::wallet::error::WatchOnlyError;

use super::AddressCacheDatabase;
use super::CachedAddress;
use super::CachedTransaction;
use super::Stats;

#[derive(Debug, Default)]
struct Inner {
    addresses: HashMap<sha256::Hash, CachedAddress>,
    transactions: HashMap<Txid, CachedTransaction>,
    stats: Stats,
    height: u32,
    descriptors: Vec<String>,
}

#[derive(Debug)]
/// Errors related to the [`MemoryDatabase`].
pub enum MemoryDatabaseError {
    /// The lock is poisoned.
    PoisonedLock,
    /// The [`CachedTransaction`] was not found.
    TransactionNotFound,
}

#[derive(Debug, Default)]
/// An in-memory database for the watch-only wallet.
pub struct MemoryDatabase {
    inner: RwLock<Inner>,
}

type Result<T> = floresta_common::prelude::Result<T, MemoryDatabaseError>;

impl Display for MemoryDatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PoisonedLock => write!(f, "Poisoned lock"),
            Self::TransactionNotFound => {
                write!(f, "The requested transaction was not found in the database")
            }
        }
    }
}

impl From<MemoryDatabaseError> for WatchOnlyError {
    fn from(e: MemoryDatabaseError) -> Self {
        Self::DatabaseError(e.to_string())
    }
}

impl MemoryDatabase {
    /// Create a new [`MemoryDatabase`].
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    /// Get the [`MemoryDatabase`]'s [`Inner`] for read operations.
    fn get_inner(&self) -> Result<sync::RwLockReadGuard<'_, Inner>> {
        self.inner
            .read()
            .map_err(|_| MemoryDatabaseError::PoisonedLock)
    }

    /// Get the [`MemoryDatabase`]'s [`Inner`] for write operations.
    fn get_inner_mut(&self) -> Result<sync::RwLockWriteGuard<'_, Inner>> {
        self.inner
            .write()
            .map_err(|_| MemoryDatabaseError::PoisonedLock)
    }
}

impl AddressCacheDatabase for MemoryDatabase {
    type Error = MemoryDatabaseError;

    /// Load [`CachedAddress`]es from the [`MemoryDatabase`].
    fn load(&self) -> Result<Vec<CachedAddress>> {
        Ok(self.get_inner()?.addresses.values().cloned().collect())
    }

    /// Save a [`CachedAddress`] to the [`MemoryDatabase`].
    fn save(&self, address: &CachedAddress) {
        self.get_inner_mut()
            .map(|mut inner| {
                inner
                    .addresses
                    .insert(address.script_hash, address.to_owned())
            })
            .unwrap();
    }

    /// Update a [`CachedAddress`] in the [`MemoryDatabase`].
    fn update(&self, address: &CachedAddress) {
        self.get_inner_mut()
            .map(|mut inner| {
                inner
                    .addresses
                    .entry(address.script_hash)
                    .and_modify(|addr| addr.clone_from(address));
            })
            .unwrap();
    }

    /// Get the height which [`CachedAddress`]es are cached to.
    fn get_cache_height(&self) -> Result<u32> {
        Ok(self.get_inner()?.height)
    }

    /// Set the height which [`CachedAddress`]es are cached to.
    fn set_cache_height(&self, height: u32) -> Result<()> {
        self.get_inner_mut()?.height = height;
        Ok(())
    }

    /// Add a new descriptor to the [`MemoryDatabase`].
    fn save_descriptor(&self, descriptor: &str) -> Result<()> {
        self.get_inner_mut().map(|mut inner| {
            inner.descriptors.push(descriptor.into());
        })
    }

    /// Get the [`MemoryDatabase`]'s descriptors.
    fn get_descriptors(&self) -> Result<Vec<String>> {
        Ok(self.get_inner()?.descriptors.to_owned())
    }

    /// Get a [`CachedTransaction`] from the [`MemoryDatabase`].
    fn get_transaction(&self, txid: &bitcoin::Txid) -> Result<CachedTransaction> {
        if let Some(tx) = self.get_inner()?.transactions.get(txid) {
            return Ok(tx.clone());
        }
        Err(MemoryDatabaseError::TransactionNotFound)
    }

    /// Save a [`CachedTransaction`] to the [`MemoryDatabase`].
    fn save_transaction(&self, tx: &CachedTransaction) -> Result<()> {
        self.get_inner_mut()?
            .transactions
            .insert(tx.hash, tx.to_owned());
        Ok(())
    }

    /// List the [`CachedTransaction`]s [`Txid`]s.
    fn list_transactions(&self) -> Result<Vec<Txid>> {
        Ok(self.get_inner()?.transactions.keys().copied().collect())
    }

    /// Get [`Stats`] about the [`MemoryDatabase`].
    fn get_stats(&self) -> Result<Stats> {
        Ok(self.get_inner()?.stats.to_owned())
    }

    /// Save [`Stats`] to the [`MemoryDatabase`].
    fn save_stats(&self, stats: &Stats) -> Result<()> {
        self.get_inner_mut().map(|mut inner| {
            inner.stats.clone_from(stats);
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use bitcoin::Txid;
    use bitcoin::hashes::Hash;
    use floresta_domain::wallet::error::WatchOnlyError;

    use super::AddressCacheDatabase;
    use super::CachedTransaction;
    use super::MemoryDatabase;
    use super::MemoryDatabaseError;
    use super::Stats;

    /// Builds a database whose lock was poisoned, by panicking while holding it
    fn poisoned_database() -> MemoryDatabase {
        let database = MemoryDatabase::new();

        let poisoned = std::panic::catch_unwind(|| {
            let _guard = database.inner.write().unwrap();
            panic!("poisoning the lock on purpose");
        });
        assert!(poisoned.is_err());

        database
    }

    fn get_test_cached_transaction() -> CachedTransaction {
        CachedTransaction {
            tx: bitcoin::Transaction {
                version: bitcoin::transaction::Version::TWO,
                lock_time: bitcoin::absolute::LockTime::ZERO,
                input: Vec::new(),
                output: Vec::new(),
            },
            hash: Txid::all_zeros(),
            height: 0,
            merkle_block: None,
            position: 0,
        }
    }

    #[test]
    fn test_memory_database_error_display() {
        let poisoned = MemoryDatabaseError::PoisonedLock.to_string();
        let not_found = MemoryDatabaseError::TransactionNotFound.to_string();

        assert!(poisoned.to_lowercase().contains("lock"));
        assert!(not_found.to_lowercase().contains("not found"));
        assert_ne!(poisoned, not_found);
    }

    #[test]
    fn test_memory_database_error_into_watch_only_error() {
        let error = MemoryDatabaseError::PoisonedLock;
        let expected = error.to_string();

        assert!(matches!(
            WatchOnlyError::from(error),
            WatchOnlyError::DatabaseError(message) if message == expected
        ));
    }

    #[test]
    fn test_poisoned_lock_every_method_reports_poisoned() {
        // `save` and `update` are excluded: their trait signatures are infallible
        // (return `()`) and panic on a poisoned lock by design.
        let database = poisoned_database();
        let transaction = get_test_cached_transaction();

        // read path, through `get_inner`
        assert!(matches!(
            database.load(),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.get_cache_height(),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.get_descriptors(),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        // the lock fails before any lookup, so this is `PoisonedLock` rather
        // than the `TransactionNotFound` a healthy database would return.
        assert!(matches!(
            database.get_transaction(&Txid::all_zeros()),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.list_transactions(),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.get_stats(),
            Err(MemoryDatabaseError::PoisonedLock)
        ));

        // write path, through `get_inner_mut`
        assert!(matches!(
            database.set_cache_height(1),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.save_descriptor("desc"),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.save_transaction(&transaction),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
        assert!(matches!(
            database.save_stats(&Stats::default()),
            Err(MemoryDatabaseError::PoisonedLock)
        ));
    }
}
