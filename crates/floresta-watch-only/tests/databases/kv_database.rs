// SPDX-License-Identifier: MIT OR Apache-2.0

use bitcoin::Block;
use bitcoin::OutPoint;
use bitcoin::ScriptBuf;
use floresta_chain::BlockConsumer;
use floresta_chain::pruned_utreexo::merkle::ConsensusMerkle;
use floresta_common::get_spk_hash;
use floresta_watch_only::AddressCache;
use floresta_watch_only::kv_database::KvDatabase;

use super::deserialize_from_str;
use super::get_test_datadir;

/// How many addresses are derived for each descriptor on every derivation
/// round. Kept in sync with the crate's private `DERIVATION_COUNT`.
const DERIVATION_COUNT: usize = 100;

/// The script paid by the transactions in the blocks below
const TEST_SCRIPT: &str = "00142b6a2924aa9b1b115d1ac3098b0ba0e6ed510f2a";

/// The descriptor pushed on the persistence test
const TEST_DESCRIPTOR: &str = "wsh(sortedmulti(1,[54ff5a12/48h/1h/0h/2h]tpubDDw6pwZA3hYxcSN32q7a5ynsKmWr4BbkBNHydHPKkM4BZwUfiK7tQ26h7USm8kA1E2FvCy7f7Er7QXKF8RNptATywydARtzgrxuPDwyYv4x/<0;1>/*,[bcf969c0/48h/1h/0h/2h]tpubDEFdgZdCPgQBTNtGj4h6AehK79Jm4LH54JrYBJjAtHMLEAth7LuY87awx9ZMiCURFzFWhxToRJK6xp39aqeJWrG5nuW3eBnXeMJcvDeDxfp/<0;1>/*))#fuw35j0q";

/// A signet block that creates a 1_000_000 sat utxo for the test script
const BLOCK_FIRST_UTXO: &str = "00000020b4f594a390823c53557c5a449fa12413cbbae02be529c11c4eb320ff8e000000dd1211eb35ca09dc0ee519b0f79319fae6ed32c66f8bbf353c38513e2132c435474d81633c4b011e195a220002010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff0403edce01feffffff028df2052a0100000016001481113cad52683679a83e76f76f84a4cfe36f75010000000000000000776a24aa21a9ed67863b4f356b7b9f3aab7a2037615989ef844a0917fb0a1dcd6c23a383ee346b4c4fecc7daa2490047304402203768ff10a948a2dd1825cc5a3b0d336d819ea68b5711add1390b290bf3b1cba202201d15e73791b2df4c0904fc3f7c7b2f22ab77762958e9bc76c625138ad3a04d290100012000000000000000000000000000000000000000000000000000000000000000000000000002000000000101be07b18750559a418d144f1530be380aa5f28a68a0269d6b2d0e6ff3ff25f3200000000000feffffff0240420f00000000001600142b6a2924aa9b1b115d1ac3098b0ba0e6ed510f2a326f55d94c060000160014c2ed86a626ee74d854a12c9bb6a9b72a80c0ddc50247304402204c47f6783800831bd2c75f44d8430bf4d962175349dc04d690a617de6c1eaed502200ffe70188a6e5ad89871b2acb4d0f732c2256c7ed641d2934c6e84069c792abc012103ba174d9c66078cf813d0ac54f5b19b5fe75104596bdd6c1731d9436ad8776f41ecce0100";

/// A signet block that spends the utxo created above, paying 999_890 back
const BLOCK_SPEND: &str = "000000203ea734fa2c8dee7d3194878c9eaf6e83a629f79b3076ec857793995e01010000eb99c679c0305a1ac0f5eb2a07a9f080616105e605b92b8c06129a2451899225ab5481633c4b011e0b26720102020000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff0403efce01feffffff026ef2052a01000000225120a1a1b1376d5165617a50a6d2f59abc984ead8a92df2b25f94b53dbc2151824730000000000000000776a24aa21a9ed1b4c48a7220572ff3ab3d2d1c9231854cb62542fbb1e0a4b21ebbbcde8d652bc4c4fecc7daa2490047304402204b37c41fce11918df010cea4151737868111575df07f7f2945d372e32a6d11dd02201658873a8228d7982df6bdbfff5d0cad1d6f07ee400e2179e8eaad8d115b7ed001000120000000000000000000000000000000000000000000000000000000000000000000000000020000000001017ca523c5e6df0c014e837279ab49be1676a9fe7571c3989aeba1e5d534f4054a0000000000fdffffff01d2410f00000000001600142b6a2924aa9b1b115d1ac3098b0ba0e6ed510f2a02473044022071b8583ba1f10531b68cb5bd269fb0e75714c20c5a8bce49d8a2307d27a082df022069a978dac00dd9d5761aa48c7acc881617fa4d2573476b11685596b17d437595012103b193d06bd0533d053f959b50e3132861527e5a7a49ad59c5e80a265ff6a77605eece0100";

#[test]
fn test_persistence_round_trip() {
    let datadir = get_test_datadir();
    let spk = ScriptBuf::from_hex(TEST_SCRIPT).expect("Valid script");
    let script_hash = get_spk_hash(&spk);
    let block1: Block = deserialize_from_str(BLOCK_FIRST_UTXO);
    let block2: Block = deserialize_from_str(BLOCK_SPEND);
    let receive_txid = block1.txdata[1].compute_txid();
    let receive_outpoint = OutPoint {
        txid: receive_txid,
        vout: 0,
    };

    // First run: build up some wallet state, then drop the database
    {
        let database = KvDatabase::new(&datadir).unwrap();
        let cache = AddressCache::new(database, ConsensusMerkle);
        cache.cache_address(spk.clone());
        cache.on_block(&block1, 118511, None);
        cache.push_descriptor(TEST_DESCRIPTOR).unwrap();
        cache.bump_height(118511);
    } // the database is flushed and closed here

    // Second run: reopen the same datadir, everything must be back
    let database = KvDatabase::new(&datadir).unwrap();
    let cache = AddressCache::new(database, ConsensusMerkle);

    // the addresses and their balances are reloaded. `load` must skip the
    // internal "height" and "desc" keys, counting only the cached addresses
    assert_eq!(cache.n_cached_addresses(), 2 * DERIVATION_COUNT + 1);
    assert!(cache.is_address_cached(&script_hash));
    assert_eq!(cache.get_address_balance(&script_hash), Some(1_000_000));

    // the utxo index is rebuilt from the persisted utxos
    assert_eq!(
        cache.get_utxo(&receive_outpoint).unwrap().value.to_sat(),
        1_000_000
    );
    let history = cache.get_address_history(&script_hash).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].hash, receive_txid);

    // descriptors, cache height and stats are persisted too
    assert_eq!(cache.get_descriptors().unwrap(), vec![TEST_DESCRIPTOR]);
    assert_eq!(cache.get_cache_height(), 118511);
    assert_eq!(cache.get_stats().unwrap().derivation_index, 0);

    // and we can pick up right where we left off
    cache.on_block(&block2, 118512, None);
    assert_eq!(cache.get_address_balance(&script_hash), Some(999_890));
    assert!(cache.get_utxo(&receive_outpoint).is_none());
}

#[test]
fn test_new_invalid_path() {
    // A datadir whose parent is a regular file can't hold a database
    let parent = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{}.txt", rand::random::<u32>()));
    std::fs::write(&parent, "not a directory").unwrap();

    let datadir = parent.join("data");
    assert!(KvDatabase::new(datadir).is_err());
}
