import pytest

from aether_sdk import AetherClient


def test_transfer_builder_constructs_transaction():
    client = AetherClient("https://rpc.aether.local")
    tx = (
        client.transfer()
        .to("0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90")
        .amount(1_000_000)
        .memo("phase7-sdk")
        .fee(2_500_000)
        .gas_limit(750_000)
        .build(
            sender="0x1111111111111111111111111111111111111111",
            sender_public_key="0x" + "a1" * 32,
            signature="0x" + "b2" * 32,
            nonce=42,
        )
    )

    response = client.submit(tx)
    assert response.accepted is True
    assert response.tx_hash.startswith("0x")
    assert response.tx_hash == tx.hash()


def test_transfer_builder_requires_recipient():
    client = AetherClient("https://rpc.aether.local")
    with pytest.raises(ValueError):
        client.transfer().amount(1_000).build(
            sender="0x1111111111111111111111111111111111111111",
            sender_public_key="0x" + "a1" * 32,
            signature="0x" + "b2" * 32,
            nonce=0,
        )
