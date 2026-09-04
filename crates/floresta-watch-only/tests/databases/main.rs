// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every [`AddressCacheDatabase`] implementation is held to the same rules: each
//! test below exercises one trait method at a time, against whichever database
//! [`setup_test`] instantiates. With the `memory-database` feature enabled we
//! test the [`MemoryDatabase`], otherwise a [`KvDatabase`] on a temporary
//! datadir. The CI feature matrix (`contrib/feature_matrix.sh`) runs both
//! configurations, so both backends get the full suite. What's specific to the
//! [`KvDatabase`] — that it persists to disk — lives in the [`kv_database`]
//! submodule.

mod kv_database;

use core::str::FromStr;

use bitcoin::Address;
use bitcoin::Transaction;
use bitcoin::consensus::Decodable;
use bitcoin::consensus::deserialize;
use bitcoin::hashes::hex::FromHex;
use floresta_common::get_spk_hash;
use floresta_watch_only::AddressCacheDatabase;
use floresta_watch_only::CachedAddress;
use floresta_watch_only::CachedTransaction;
use floresta_watch_only::Stats;
#[cfg(not(feature = "memory-database"))]
use floresta_watch_only::kv_database::KvDatabase;
#[cfg(not(feature = "memory-database"))]
use floresta_watch_only::kv_database::KvDatabaseError;
#[cfg(feature = "memory-database")]
use floresta_watch_only::memory_database::MemoryDatabase;
#[cfg(feature = "memory-database")]
use floresta_watch_only::memory_database::MemoryDatabaseError;
use floresta_watch_only::merkle::MerkleProof;

/// The database under test. With the `memory-database` feature enabled, we
/// test the [`MemoryDatabase`]; otherwise we instantiate a [`KvDatabase`].
#[cfg(feature = "memory-database")]
type TestDatabase = Box<dyn AddressCacheDatabase<Error = MemoryDatabaseError>>;
#[cfg(not(feature = "memory-database"))]
type TestDatabase = Box<dyn AddressCacheDatabase<Error = KvDatabaseError>>;

/// Sets up a database instance to test against, as selected by the features.
fn setup_test() -> TestDatabase {
    #[cfg(feature = "memory-database")]
    {
        let database = MemoryDatabase::new();

        Box::new(database)
    }

    #[cfg(not(feature = "memory-database"))]
    {
        let database =
            KvDatabase::new(get_test_datadir()).expect("Could not create the test database");

        Box::new(database)
    }
}

/// A unique datadir under cargo's target tmpdir, so tests don't litter the repo
pub(crate) fn get_test_datadir() -> std::path::PathBuf {
    let test_id = rand::random::<u32>();
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{test_id}.floresta"))
}

fn deserialize_from_str<T: Decodable>(thing: &str) -> T {
    let hex = Vec::from_hex(thing).unwrap();
    deserialize(&hex).unwrap()
}

/// Two descriptors to hold the databases' descriptor accumulation to account
const TEST_DESCRIPTORS: [&str; 2] = [
    "pkh(xpub6CPimhNogJosVzpueNmrWEfSHc2YTXG1ZyE6TBV4Nx6UxZ7zKSGYv9hKxNjiFY5o1vz7QeZa2m6vQmyndDrkECk8cShWYWxe1gqa1xJEkgs/0/*)#32jmvyn7",
    "pkh(xpub6CPimhNogJosVzpueNmrWEfSHc2YTXG1ZyE6TBV4Nx6UxZ7zKSGYv9hKxNjiFY5o1vz7QeZa2m6vQmyndDrkECk8cShWYWxe1gqa1xJEkgs/1/*)#q7h633rx",
];

/// A test address, built from a real bech32 address
fn get_test_cached_address() -> CachedAddress {
    let address = Address::from_str("tb1q9d4zjf92nvd3zhg6cvyckzaqumk4zre26x02q9")
        .unwrap()
        .assume_checked();

    CachedAddress {
        script_hash: get_spk_hash(&address.script_pubkey()),
        balance: 0,
        script: address.script_pubkey(),
        transactions: Vec::new(),
        utxos: Vec::new(),
    }
}

/// A signet transaction, with its merkle proof
fn get_test_cached_transaction() -> CachedTransaction {
    let transaction = "020000000001017ca523c5e6df0c014e837279ab49be1676a9fe7571c3989aeba1e5d534f4054a0000000000fdffffff01d2410f00000000001600142b6a2924aa9b1b115d1ac3098b0ba0e6ed510f2a02473044022071b8583ba1f10531b68cb5bd269fb0e75714c20c5a8bce49d8a2307d27a082df022069a978dac00dd9d5761aa48c7acc881617fa4d2573476b11685596b17d437595012103b193d06bd0533d053f959b50e3132861527e5a7a49ad59c5e80a265ff6a77605eece0100";
    let transaction: Transaction = deserialize_from_str(transaction);

    let merkle_block = "0100000000000000ea530307089e3e6f6e8997a0ae48e1dc2bee84635bc4e6c6ecdcc7225166b06b010000000000000034086ef398efcdec47b37241221c8f4613e02bc31026cc74d07ddb3092e6d6e7";
    let merkle_block: MerkleProof = deserialize_from_str(merkle_block);

    CachedTransaction {
        hash: transaction.compute_txid(),
        height: 118511,
        merkle_block: Some(merkle_block),
        position: 1,
        tx: transaction,
    }
}

fn assert_cached_address(value: &CachedAddress, expect: &CachedAddress) {
    assert_eq!(value.script_hash, expect.script_hash);
    assert_eq!(value.balance, expect.balance);
    assert_eq!(value.script, expect.script);
    assert_eq!(value.transactions, expect.transactions);
    assert_eq!(value.utxos, expect.utxos);
}

fn assert_stats(value: &Stats, expect: &Stats) {
    assert_eq!(value.address_count, expect.address_count);
    assert_eq!(value.transaction_count, expect.transaction_count);
    assert_eq!(value.utxo_count, expect.utxo_count);
    assert_eq!(value.cache_height, expect.cache_height);
    assert_eq!(value.txo_count, expect.txo_count);
    assert_eq!(value.balance, expect.balance);
    assert_eq!(value.derivation_index, expect.derivation_index);
}

#[test]
fn test_save_and_load_address() {
    let database = setup_test();
    let address = get_test_cached_address();

    // a new database has no addresses
    assert!(database.load().unwrap().is_empty());

    // saving and loading an address preserves every field
    database.save(&address);
    let loaded = database.load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_cached_address(&loaded[0], &address);
}

#[test]
fn test_update_address() {
    let database = setup_test();
    let address = get_test_cached_address();

    // updating an address we never saved inserts it
    database.update(&address);
    assert_eq!(database.load().unwrap().len(), 1);

    // updating a saved address overwrites it
    let mut modified = address.clone();
    modified.balance = 999_890;
    database.update(&modified);
    let loaded = database.load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_cached_address(&loaded[0], &modified);
}

/// A fresh database starts empty. Stats and cache height are the one divergence
/// between the backends: [`KvDatabase`] tells us the wallet isn't initialized
/// yet, while [`MemoryDatabase`] hands out zeroes.
#[test]
fn test_fresh_database_is_empty() {
    let database = setup_test();

    assert!(database.load().unwrap().is_empty());
    assert!(database.get_descriptors().unwrap().is_empty());
    assert!(database.list_transactions().unwrap().is_empty());

    #[cfg(feature = "memory-database")]
    {
        let stats = database.get_stats().unwrap();
        assert_eq!(stats.address_count, 0);
        assert_eq!(stats.transaction_count, 0);
        assert_eq!(stats.utxo_count, 0);
        assert_eq!(stats.cache_height, 0);
        assert_eq!(stats.txo_count, 0);
        assert_eq!(stats.balance, 0);
        assert_eq!(stats.derivation_index, 0);
        assert_eq!(database.get_cache_height().unwrap(), 0);
    }

    #[cfg(not(feature = "memory-database"))]
    {
        assert!(matches!(
            database.get_stats(),
            Err(KvDatabaseError::WalletNotInitialized)
        ));
        assert!(matches!(
            database.get_cache_height(),
            Err(KvDatabaseError::WalletNotInitialized)
        ));
    }
}

#[test]
fn test_save_and_get_stats() {
    let database = setup_test();

    let stats = Stats {
        address_count: 11,
        transaction_count: 22,
        utxo_count: 33,
        cache_height: 44,
        txo_count: 55,
        balance: 66,
        derivation_index: 77,
    };
    database.save_stats(&stats).unwrap();
    let loaded_stats = database.get_stats().unwrap();
    assert_stats(&stats, &loaded_stats);

    // saving again overwrites the previous stats
    let stats = Stats {
        address_count: 1,
        transaction_count: 2,
        utxo_count: 3,
        cache_height: 4,
        txo_count: 5,
        balance: 6,
        derivation_index: 7,
    };
    database.save_stats(&stats).unwrap();
    let loaded_stats = database.get_stats().unwrap();
    assert_stats(&stats, &loaded_stats);
}

#[test]
fn test_set_and_get_cache_height() {
    let database = setup_test();

    let mut height = 118511;
    database.set_cache_height(height).unwrap();
    assert_eq!(database.get_cache_height().unwrap(), height);

    // setting it again overwrites the previous height
    height = 118512;
    database.set_cache_height(height).unwrap();
    assert_eq!(database.get_cache_height().unwrap(), height);
}

#[test]
fn test_save_and_get_descriptors() {
    let database = setup_test();

    // descriptors accumulate, in insertion order
    database.save_descriptor(TEST_DESCRIPTORS[0]).unwrap();
    assert_eq!(
        database.get_descriptors().unwrap(),
        vec![TEST_DESCRIPTORS[0]]
    );
    database.save_descriptor(TEST_DESCRIPTORS[1]).unwrap();
    assert_eq!(
        database.get_descriptors().unwrap(),
        TEST_DESCRIPTORS.to_vec()
    );
}

#[test]
fn test_get_transaction_not_found() {
    let database = setup_test();
    let cached_tx = get_test_cached_transaction();

    // looking up a transaction we never saved tells us it's not found
    #[cfg(feature = "memory-database")]
    assert!(matches!(
        database.get_transaction(&cached_tx.hash),
        Err(MemoryDatabaseError::PoisonedLock)
    ));
    #[cfg(not(feature = "memory-database"))]
    assert!(matches!(
        database.get_transaction(&cached_tx.hash),
        Err(KvDatabaseError::TransactionNotFound)
    ));
}

#[test]
fn test_save_and_get_transaction() {
    let database = setup_test();
    let cached_tx = get_test_cached_transaction();

    database.save_transaction(&cached_tx).unwrap();
    let loaded_tx = database.get_transaction(&cached_tx.hash).unwrap();
    assert_eq!(loaded_tx.height, cached_tx.height);
    assert_eq!(loaded_tx.position, cached_tx.position);
    assert_eq!(loaded_tx.hash, cached_tx.hash);
    assert_eq!(loaded_tx.tx.compute_txid(), cached_tx.tx.compute_txid());
    assert_eq!(loaded_tx.merkle_block, cached_tx.merkle_block);

    // saving the same txid again overwrites it, like when a mempool
    // transaction confirms
    let mut confirmed = cached_tx.clone();
    confirmed.height = 118512;
    confirmed.merkle_block = None;
    database.save_transaction(&confirmed).unwrap();

    let loaded_tx = database.get_transaction(&cached_tx.hash).unwrap();
    assert_eq!(loaded_tx.height, 118512);
    assert!(loaded_tx.merkle_block.is_none());
}

#[test]
fn test_list_transactions() {
    let database = setup_test();
    let cached_tx = get_test_cached_transaction();

    assert!(database.list_transactions().unwrap().is_empty());

    database.save_transaction(&cached_tx).unwrap();
    assert_eq!(database.list_transactions().unwrap(), vec![cached_tx.hash]);

    // saving the same transaction again doesn't duplicate the listing
    database.save_transaction(&cached_tx).unwrap();
    assert_eq!(database.list_transactions().unwrap(), vec![cached_tx.hash]);
}
