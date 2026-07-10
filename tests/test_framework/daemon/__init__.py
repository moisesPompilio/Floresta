# SPDX-License-Identifier: MIT OR Apache-2.0

"""
Daemon test framework package
"""


# pylint: disable=too-few-public-methods
class ConfigP2P:
    """
    Configuration for P2P connection
    """

    def __init__(self, host: str, port: int, network):
        self.host = host
        self.port = port
        self.network = network
