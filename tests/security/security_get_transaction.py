"""Security regression tests for gettransaction with descriptor-scoped transactions.

These tests verify that Floresta can load a known descriptor, sync enough chain data,
and retrieve the expected raw transaction bytes for a known txid on signet and mainnet.
The goal is to guard against regressions in synchronization and transaction lookup.
"""

import pytest

from test_framework.node import NodeType
from test_framework.util import wait_until

TXID_SIGNET = "04715f6d60207ce876aa45bc46b3dd349b37823d7e53f6d79ee4fd2b1015065e"

TX_SIGNET = (
    "020000000001019f01c05d5a0ef579fa606691770a4e11007411a8692434bbe297c9faaabfe8ae0000000000fd"
    "ffffff0174090c0000000000160014475a6caaddc196ead79351eac85873d39464437b0247304402204101615c"
    "0cdab166ab8cabd9a32780d81533acc1589e5b6833cc2c4d8940857b0220786df9db5ff70004fb4eb480ea87e7"
    "c5ae12610d05e3de44756767e804f091ba012103d93fb2319de69cc22820accfb55338cf2cd1e56c9de82d59de"
    "5296a8cb496b1a62a70400"
)

# mnemonics: abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon
# abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon
# abandon abandon abandon art

DESC_SIGNET = (
    "wpkh([5436d724/84h/1h/0h]tpubDCWivZp6qaqCALCt8MyLqAb3awnWm4hfbBPjdZqirYFXYeZ5YsfbWVaPacULZ"
    "TGtK1RPBSZ92UWNjnhL4fB9UVrF2FjgW8cgmBjxPBmB4iB/<0;1>/*)#v40r3qp9"
)

# mnemonics: abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon
# abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon
# abandon abandon abandon art

DESC_MAIN = (
    "wpkh([5436d724/84h/0h/0h]xpub6Bner3L3tdQW367NmmMsWKtMfP7hbu4JxdtbSGdWWjSzLkSUEnT7G9h5GFWUX"
    "tifeRhHiUXJuek1qeaTJqnXkveWpiHp8rmt53E8HTMshg9/<0;1>/*)#c8zgkcfy"
)

TXID_MAIN = "ea3aba4e07d839a520aac86023b4abbdf28abd25811dad793e4a71ef6da67bf5"

TX_MAIN = (
    "01000000000101fab1445a5ced66795f97bda496784512d4d672dfad8a03376facdc0a0ec742fc0200000000ff"
    "ffffff1e652900000000000016001416d630413cea75fd5ad88433cd815ae5ba1ba6456d300100000000001600"
    "14d4f54b07e865a9b7369aa30ff4dbf1760576688d679d26000000000017a9141a2b31f3d1d5f6abca5cd8bee1"
    "6d9d9ed57916f18700420100000000001976a914f1b295b3cb1f8aa40c9190ae134ebc03f0f9f3e788aca02402"
    "000000000016001472c02f38a7b11973f98900e204981b054e001a8b0ea10600000000002251205546f488b4e6"
    "b552b9f1eb38dbb30900cc8d2d2cde331806e2e7e21e140461ca618f020000000000160014fbc07631d50714b2"
    "120d4d06e3df78245344a77ee9490400000000001600143430663da42c81e1205117cfcb497747af570ab5e632"
    "01000000000017a91404abe149c2c42e49e8376049764f3d102b907dff877440000000000000160014c4d3e06f"
    "90841e6f06be48eaec0aaa25fd0f0580618f020000000000160014bdb1f3ca51138fe47bde8d1246d392c7b680"
    "81160429000000000000160014eb93936815cf557ae1e9f57dc932cd0c82088ab6c4b40200000000001976a914"
    "3d8a4083de1fe0f684c48df9770e479c049b047288ac082a020000000000160014b3c325a0f92f8d473602381b"
    "6df90f2c0da6a920ebc143000000000016001464ad4bd34432dda6892bfbc5710845e0370fd80b444800000000"
    "00001976a914af7b57fdde3852986bb36c5ad7e513163696f13c88acc41f260000000000160014e61b80a212db"
    "83ddf477cd966fa97d3d491e6b544f91000000000000160014598b23f35a1763216b429410bf7c16a54365a4c2"
    "74bd00000000000016001413e89ffd53bffee3d18bfc9ff2e6bd3843db936249ca13010000000017a914471f0b"
    "6247f0028f1a6eaf187c5a8cc84300ba6d87655a00000000000017a9148a5d87d8ec3a33cff25da873d9c2bdb4"
    "c78a902e87bbf70a0000000000160014a7490e99e7680de2f2e3292939254a6785f3ff513fc90a000000000022"
    "5120d9d86909e385fb1ede6f04bd727dda2e7680c2479a7a8b6efdd02e559ddd44a7df19020000000000160014"
    "24a0539d6294a5b8b4ec353e30a834352e439a51fcc1030000000000160014d61172f3f2b72978e468da4e59db"
    "30f7f168024773370000000000001600146263fc6ab4ea2d84ae1324680691c3ca9695ee691ba2000000000000"
    "160014ce31473b0a4373dc147fc9470a6870c4359a21b8f1390800000000001600142c1f2a41ff2f9646154113"
    "495717d98117b48b01029e050000000000220020dc0a58a73e807381baa694bd74728ba0a12f4b997598b90c92"
    "b7186a2ef93fce36fa192801000000160014dc6bf86354105de2fcd9868a2b0376d6731cb92f02483045022100"
    "b0878a789d69dd47b6eb872c9e2719850e284ac72a018aaea705cfaef6304d0c02201de1975764a32c762d4644"
    "a4e2e21ddb0e3fe6b873380b37e7378049b18ae38a012102174ee672429ff94304321cdae1fc1e487edf658b34"
    "bd1d36da03761658a2bb0900000000"
)


@pytest.mark.security
def test_get_transaction_security_signet(setup_logging, add_signet_node):
    """
    Validate gettransaction on signet for a transaction tied to a loaded descriptor.

    This test ensures Floresta keeps syncing and can still fetch the exact expected raw
    transaction after the descriptor is loaded.
    """
    log = setup_logging
    node = add_signet_node(variant=NodeType.FLORESTAD)

    # Run the security test
    assert_descriptor_transaction_security(
        log, node, DESC_SIGNET, TXID_SIGNET, TX_SIGNET
    )


@pytest.mark.security
def test_get_transaction_security_mainnet(setup_logging, add_mainnet_node):
    """
    Validate gettransaction on mainnet for a transaction tied to a loaded descriptor.

    This test ensures Floresta keeps syncing and can still fetch the exact expected raw
    transaction after the descriptor is loaded.
    """
    log = setup_logging
    node = add_mainnet_node(variant=NodeType.FLORESTAD)

    # Run the security test
    assert_descriptor_transaction_security(log, node, DESC_MAIN, TXID_MAIN, TX_MAIN)


def assert_descriptor_transaction_security(log, node, desc, txid, tx_hex):
    """Assert that a descriptor-associated transaction can be fetched as expected.

    The function loads a descriptor, then polls the node until synchronization makes
    the transaction available. It validates that get_raw_transaction returns the exact
    expected serialized transaction hex.
    """
    node.rpc.load_descriptor(desc)

    def check_transaction():
        try:
            tx = node.rpc.get_raw_transaction(txid, False)
            assert tx == tx_hex
            return True
        except Exception as e:
            log.error(f"Error while checking transaction: {e}")
            return False

    wait_until(check_transaction, timeout=7200, interval=300)
