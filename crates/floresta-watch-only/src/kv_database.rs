// SPDX-License-Identifier: MIT OR Apache-2.0

use core::error::Error;
use core::fmt;
use core::fmt::Display;
use core::fmt::Formatter;
use std::path::Path;

use bitcoin::Txid;
use bitcoin::consensus::deserialize;
use bitcoin::consensus::encode::Error as EncodingError;
use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use floresta_common::impl_error_from;
use floresta_common::prelude::*;
use floresta_domain::wallet::error::WatchOnlyError;
use kv::Bucket;
use kv::Config;
use kv::Store;

use super::AddressCacheDatabase;
use super::CachedAddress;
use super::CachedTransaction;
use super::Stats;

/// A key-value database for the watch-only wallet.
pub struct KvDatabase(Store, Bucket<'static, String, Vec<u8>>);

impl KvDatabase {
    /// Create a new [`KvDatabase`] inside the `datadir`.
    pub fn new(datadir: impl AsRef<Path>) -> Result<Self> {
        let kv_db_config = Config::new(datadir);

        let kv_db_store = Store::new(kv_db_config)?;
        let kv_db_bucket = kv_db_store.bucket::<String, Vec<u8>>(Some("addresses"))?;

        Ok(Self(kv_db_store, kv_db_bucket))
    }
}

/// Errors related to the [`KvDatabase`].
#[derive(Debug)]
pub enum KvDatabaseError {
    /// A [`kv`] error.
    KvError(kv::Error),

    /// A [`serde_json`] error.
    SerdeJsonError(serde_json::Error),

    /// The wallet is not initialized.
    WalletNotInitialized,

    /// Failed to deseriliaze data.
    DeserializeError(EncodingError),

    /// The [`CachedTransaction`] was not found.
    TransactionNotFound,
}

impl Display for KvDatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::KvError(e) => write!(f, "kv error: {e}"),
            Self::SerdeJsonError(e) => write!(f, "JSON error: {e}"),
            Self::WalletNotInitialized => write!(f, "The wallet is not initialized"),
            Self::DeserializeError(e) => write!(f, "Error whilst deserilializing: {e}"),
            Self::TransactionNotFound => {
                write!(f, "The requested transaction was not found in the database")
            }
        }
    }
}

impl Error for KvDatabaseError {}

impl_error_from!(KvDatabaseError, serde_json::Error, SerdeJsonError);
impl_error_from!(KvDatabaseError, kv::Error, KvError);
impl_error_from!(KvDatabaseError, EncodingError, DeserializeError);

impl From<KvDatabaseError> for WatchOnlyError {
    fn from(e: KvDatabaseError) -> Self {
        Self::DatabaseError(e.to_string())
    }
}

type Result<T> = floresta_common::prelude::Result<T, KvDatabaseError>;

impl AddressCacheDatabase for KvDatabase {
    type Error = KvDatabaseError;

    /// Load [`CachedAddress`]es from the [`KvDatabase`].
    fn load(&self) -> Result<Vec<CachedAddress>> {
        let bucket = &self.1;
        let mut addresses = Vec::new();

        for item in bucket.iter() {
            let item = item?;

            let key = item.key::<String>()?;
            if *"height" == key || *"desc" == key {
                continue;
            }
            let value: Vec<u8> = item.value().unwrap();
            let value = serde_json::from_slice(&value)?;
            addresses.push(value);
        }
        Ok(addresses)
    }

    /// Save a [`CachedAddress`] to the [`KvDatabase`].
    fn save(&self, address: &CachedAddress) {
        let key = address.script_hash.to_string();
        let value = serde_json::to_vec(&address).expect("Invalid object serialization");
        let bucket = &self.1;

        bucket
            .set(&key, &value)
            .expect("Fatal: Database isn't working");
        bucket.flush().expect("Could not write to disk");
    }

    /// Update a [`CachedAddress`] in the [`KvDatabase`].
    fn update(&self, address: &CachedAddress) {
        self.save(address);
    }

    /// Get the height which [`CachedAddress`]es are cached to.
    fn get_cache_height(&self) -> Result<u32> {
        let bucket = &self.1;

        let cached_height = bucket.get(&String::from("height"))?;
        if let Some(cached_height) = cached_height {
            return Ok(deserialize(&cached_height)?);
        }

        Err(KvDatabaseError::WalletNotInitialized)
    }

    /// Set the height which [`CachedAddress`]es are cached to.
    fn set_cache_height(&self, height: u32) -> Result<()> {
        let bucket = &self.1;

        bucket.set(&String::from("height"), &serialize(&height))?;
        bucket.flush()?;

        Ok(())
    }

    /// Add a new descriptor to the [`KvDatabase`].
    fn save_descriptor(&self, descriptor: &str) -> Result<()> {
        let bucket = &self.1;
        let mut descriptors = self.get_descriptors()?;

        descriptors.push(String::from(descriptor));

        let serialized = serde_json::to_vec(&descriptors)?;
        bucket.set(&String::from("desc"), &serialized)?;
        bucket.flush()?;

        Ok(())
    }

    /// Get the [`KvDatabase`]'s descriptors.
    fn get_descriptors(&self) -> Result<Vec<String>> {
        let descriptor = self.1.get(&String::from("desc"))?;

        if let Some(descriptor) = descriptor {
            return Ok(serde_json::de::from_slice(&descriptor)?);
        }
        Ok(Vec::new())
    }

    /// Get a [`CachedTransaction`] from the [`KvDatabase`], given its [`Txid`].
    fn get_transaction(&self, txid: &Txid) -> Result<CachedTransaction> {
        let tx_store = self.0.bucket::<&[u8], Vec<u8>>(Some("transactions"))?;

        let transaction = tx_store.get(&txid.as_byte_array().to_vec().as_slice())?;
        if let Some(transaction) = transaction {
            return Ok(serde_json::de::from_slice(&transaction)?);
        }
        Err(KvDatabaseError::TransactionNotFound)
    }

    /// Save a [`CachedTransaction`] to the [`KvDatabase`].
    fn save_transaction(&self, tx: &CachedTransaction) -> Result<()> {
        let tx_store = self.0.bucket::<&[u8], Vec<u8>>(Some("transactions"))?;
        let bucket = &self.1;

        let txid = tx.tx.compute_txid().as_byte_array().to_vec();
        let serialized_tx = serde_json::to_vec(&tx)?;
        tx_store.set(&txid.as_slice(), &serialized_tx)?;
        bucket.flush()?;

        Ok(())
    }

    /// List the [`CachedTransaction`]s [`Txid`]s.
    fn list_transactions(&self) -> Result<Vec<Txid>> {
        let mut transactions = Vec::new();
        let tx_store = self.0.bucket::<&[u8], Vec<u8>>(Some("transactions"))?;

        for item in tx_store.iter() {
            let item = item?;
            let key = item.key::<&[u8]>()?;
            transactions.push(deserialize(key)?);
        }
        Ok(transactions)
    }

    /// Get [`Stats`] about the [`KvDatabase`].
    fn get_stats(&self) -> Result<Stats> {
        let stats_store = self.0.bucket::<&[u8], Vec<u8>>(Some("stats"))?;

        let stats = stats_store.get(&String::from("stats").as_bytes())?;
        if let Some(stats) = stats {
            return Ok(serde_json::de::from_slice(&stats)?);
        }
        Err(KvDatabaseError::WalletNotInitialized)
    }

    /// Save [`Stats`] to the [`KvDatabase`].
    fn save_stats(&self, stats: &Stats) -> Result<()> {
        let stats_store = self.0.bucket::<&[u8], Vec<u8>>(Some("stats"))?;

        let ser_stats = serde_json::to_vec(&stats)?;
        stats_store.set(&String::from("stats").as_bytes(), &ser_stats)?;
        self.1.flush()?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use bitcoin::Txid;
    use bitcoin::consensus::deserialize;
    use bitcoin::hashes::Hash;
    use floresta_domain::wallet::error::WatchOnlyError;

    use super::KvDatabase;
    use super::KvDatabaseError;
    use crate::AddressCacheDatabase;

    /// Removes the test datadir when dropped, even if the test panics
    struct TestDir(PathBuf);

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Creates a database inside a fresh datadir under the system temp dir.
    ///
    /// Bind the result as `let (_dir, db) = get_test_db();`: bindings drop in reverse
    /// order, so the database is closed before the guard removes its datadir.
    fn get_test_db() -> (TestDir, KvDatabase) {
        let test_id = rand::random::<u32>();
        let dir = TestDir(std::env::temp_dir().join(format!("{test_id}.floresta")));
        let db = KvDatabase::new(&dir.0).unwrap();

        (dir, db)
    }

    #[test]
    fn test_get_cache_height_deserialize_error() {
        // a corrupted height is reported instead of panicking
        let (_dir, db) = get_test_db();
        db.1.set(&String::from("height"), &vec![0xff]).unwrap();

        let error = db.get_cache_height().unwrap_err();

        assert!(matches!(error, KvDatabaseError::DeserializeError(_)));
    }

    #[test]
    fn test_corrupted_descriptor_list_serde_json_error() {
        // a corrupted descriptor list is reported instead of panicking
        let (_dir, db) = get_test_db();
        db.1.set(&String::from("desc"), &b"not a json".to_vec())
            .unwrap();

        let error = db.get_descriptors().unwrap_err();
        assert!(matches!(error, KvDatabaseError::SerdeJsonError(_)));

        // saving a descriptor reads the current list first, so it fails the same way
        let error = db.save_descriptor("some descriptor").unwrap_err();
        assert!(matches!(error, KvDatabaseError::SerdeJsonError(_)));
    }

    #[test]
    fn test_load_corrupted_address_error() {
        // a cached address that can't be deserialized is reported
        let (_dir, db) = get_test_db();
        db.1.set(&String::from("corrupted-address"), &b"not a json".to_vec())
            .unwrap();

        let error = db.load().unwrap_err();

        assert!(matches!(error, KvDatabaseError::SerdeJsonError(_)));
    }

    #[test]
    fn test_load_invalid_key_error() {
        // a key that isn't valid utf-8 is reported
        let (_dir, db) = get_test_db();
        let raw = db.0.bucket::<&[u8], Vec<u8>>(Some("addresses")).unwrap();
        let key: &[u8] = &[0xff, 0xfe, 0xfd];
        raw.set(&key, &b"not a json".to_vec()).unwrap();

        assert!(matches!(
            db.load().unwrap_err(),
            KvDatabaseError::KvError(_)
        ));
    }

    #[test]
    fn test_list_transactions_invalid_key_error() {
        // a key that isn't a 32-byte txid is reported instead of panicking
        let (_dir, db) = get_test_db();
        let tx_store = db.0.bucket::<&[u8], Vec<u8>>(Some("transactions")).unwrap();
        let key: &[u8] = &[0x01, 0x02, 0x03];
        tx_store.set(&key, &b"not a json".to_vec()).unwrap();

        assert!(matches!(
            db.list_transactions().unwrap_err(),
            KvDatabaseError::DeserializeError(_)
        ));
    }

    #[test]
    fn test_get_stats_serde_json_error() {
        // corrupted stats are reported instead of panicking
        let (_dir, db) = get_test_db();
        let stats = db.0.bucket::<&[u8], Vec<u8>>(Some("stats")).unwrap();
        stats
            .set(&String::from("stats").as_bytes(), &b"not a json".to_vec())
            .unwrap();

        let error = db.get_stats().unwrap_err();

        assert!(matches!(error, KvDatabaseError::SerdeJsonError(_)));
    }

    #[test]
    fn test_get_transaction_serde_json_error() {
        // a cached transaction that can't be deserialized is reported
        let (_dir, db) = get_test_db();
        let txid = Txid::all_zeros();
        let tx_store = db.0.bucket::<&[u8], Vec<u8>>(Some("transactions")).unwrap();
        tx_store
            .set(
                &txid.as_byte_array().to_vec().as_slice(),
                &b"not a json".to_vec(),
            )
            .unwrap();

        let error = db.get_transaction(&txid).unwrap_err();

        assert!(matches!(error, KvDatabaseError::SerdeJsonError(_)));
    }

    #[test]
    fn test_kv_database_error_display() {
        let kv_inner = kv::Error::Message(String::from("boom"));
        let json_inner = serde_json::from_slice::<u8>(b"not a json").unwrap_err();
        let encoding_inner = deserialize::<Txid>(&[0x01]).unwrap_err();
        let encoding_inner_message = encoding_inner.to_string();

        let kv = KvDatabaseError::KvError(kv_inner).to_string();
        let json = KvDatabaseError::SerdeJsonError(json_inner).to_string();
        let not_initialized = KvDatabaseError::WalletNotInitialized.to_string();
        let encoding = KvDatabaseError::DeserializeError(encoding_inner).to_string();
        let not_found = KvDatabaseError::TransactionNotFound.to_string();

        // wrapping variants carry their inner error's message
        assert!(kv.contains("boom"));
        assert!(json.contains("expected"));
        assert!(not_initialized.contains("initialized"));
        assert!(encoding.contains(&encoding_inner_message));
        assert!(not_found.contains("not found"));

        // and every variant renders differently
        let messages = [kv, json, not_initialized, encoding, not_found];
        for (i, a) in messages.iter().enumerate() {
            for b in &messages[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_kv_database_error_into_watch_only_error() {
        let (_dir, db) = get_test_db();
        let error = db.get_cache_height().unwrap_err();
        let message = error.to_string();

        assert!(matches!(
            WatchOnlyError::from(error),
            WatchOnlyError::DatabaseError(inner) if inner == message
        ));
    }
}
