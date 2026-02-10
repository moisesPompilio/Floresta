"""
Tests for node information exchanged between Floresta and other peers.
"""

import time
import re

from test_framework import FlorestaTestFramework
from test_framework.node import NodeType

VPUB = "vpub5Y6cjg78GGuNLsaPhmYsiw4gYX3HoQiRBiSwDaBXKUafCt9bNwWQiitDk5VZ5BVxYnQdwoTyXSs2JHRPAgjAvtbBrf8ZhDYe2jWAqvZVnsc"
TXID = [
    {
        "txid": "b5e537cd807ab5417c45ae5fa0b8b4095ab310733fbb3d80e36779a3dda581c4",
        "value": 0.00302654,
        "address": "tb1p4av4jmh4jxke2svn8fpcgx5z46kapl8u8muuutqss8st3eenrshqem8s9p",
        "blockhash": "000000103c25e2ce22b5e60a4f5c9bcc4d9343f758133aaaf426a7ea2623773a",
    },
    {
        "txid": "fbf8d6b6d6648cee13c27cf3c4f1cb2a79cbf1b51323dfd24e2ff07f08c40c39",
        "value": 0.03479628,
        "address": "tb1p4av4jmh4jxke2svn8fpcgx5z46kapl8u8muuutqss8st3eenrshqem8s9p",
        "blockhash": "00000007bfbb42c15539f01caaedd178c92074c1ac468ffc470bafe6d4d13487",
    },
]


class IBDSignetTest(FlorestaTestFramework):

    def set_test_params(self):
        """
        Setup a florestad in signet network
        """
        self.florestad = self.add_florestad_extra_args_and_network("signet", [f"--wallet-xpub={VPUB}"])

    def run_test(self):
        """
        Tests that the node information (e.g., version and subversion) sent by Floresta to other
        peers is correct.
        """
        self.run_node(self.florestad)

        time.sleep(5)

        self.log("=== Wait for the node to finish IBD...")
        info = self.florestad.rpc.get_blockchain_info()
        while info["height"] < 1 or info["ibd"]:
            time.sleep(10)
            info = self.florestad.rpc.get_blockchain_info()
            self.log(f"=== Current block count: {info['height']}, IBD: {info['ibd']}")

        for tx in TXID:
            self.log(f"=== Checking transaction {tx['txid']}...")
            result = self.florestad.rpc.get_transaction(tx['txid'])
            self.log(f"=== Transaction {tx['txid']} result: {result}")





if __name__ == "__main__":
    IBDSignetTest().main()
