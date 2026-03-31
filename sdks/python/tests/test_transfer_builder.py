import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from aether_sdk import AetherClient


@contextmanager
def rpc_server():
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):  # noqa: N802
            content_len = int(self.headers.get("content-length", 0))
            payload = json.loads(self.rfile.read(content_len).decode("utf-8"))
            requests.append(payload)

            if payload.get("method") == "aeth_sendTransaction":
                response = {
                    "jsonrpc": "2.0",
                    "id": payload.get("id", 1),
                    "result": "0x" + "ab" * 32,
                }
            elif payload.get("method") == "aeth_getSlotNumber":
                response = {
                    "jsonrpc": "2.0",
                    "id": payload.get("id", 1),
                    "result": 123,
                }
            else:
                response = {
                    "jsonrpc": "2.0",
                    "id": payload.get("id", 1),
                    "error": {"code": -32601, "message": "method not found"},
                }

            encoded = json.dumps(response).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, format, *args):  # noqa: A003
            return

    try:
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    except PermissionError:
        pytest.skip("socket binding is not permitted in this environment")
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    try:
        endpoint = f"http://127.0.0.1:{server.server_port}"
        yield endpoint, requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=1)


def test_transfer_builder_submits_over_rpc():
    with rpc_server() as (endpoint, requests):
        client = AetherClient(endpoint)
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
                signature="0x" + "b2" * 64,
                nonce=42,
            )
        )

        response = client.submit(tx)
        assert response.accepted is True
        assert response.tx_hash == "0x" + "ab" * 32

        assert requests, "expected JSON-RPC request to be emitted"
        payload = requests[0]
        assert payload["method"] == "aeth_sendTransaction"
        assert payload["params"][0]["recipient"] == tx.recipient


def test_get_slot_number_reads_rpc():
    with rpc_server() as (endpoint, _requests):
        client = AetherClient(endpoint)
        assert client.get_slot_number() == 123


def test_transfer_builder_constructs_transaction():
    client = AetherClient("http://127.0.0.1:8545")
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

    rpc_payload = tx.to_rpc_transaction()
    assert rpc_payload["recipient"] == tx.recipient
    assert rpc_payload["amount"] == "1000000"


def test_transfer_builder_requires_recipient():
    client = AetherClient("http://127.0.0.1:8545")
    with pytest.raises(ValueError):
        client.transfer().amount(1_000).build(
            sender="0x1111111111111111111111111111111111111111",
            sender_public_key="0x" + "a1" * 32,
            signature="0x" + "b2" * 32,
            nonce=0,
        )
